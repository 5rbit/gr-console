//! Measurement history (M2, lead): mirrors MEASLOG / MEASLOG_HIST into sqlite on change and serves
//! snapshot / entries / stats / CSV. M1 ships the store shell + on-change sync.

pub mod routes;

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::MeasureLogEntry;
use serde_json::{Value as Json, json};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::plc::{PlcHandle, Tier};

pub struct MeasureStore {
    db: Db,
    plc: Mutex<Option<PlcHandle>>,
    last_total: Mutex<u32>,
    pub events: broadcast::Sender<Json>,
}

impl MeasureStore {
    pub fn new(db: Db) -> Arc<MeasureStore> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(MeasureStore { db, plc: Mutex::new(None), last_total: Mutex::new(0), events: tx })
    }

    pub fn attach(self: &Arc<Self>, plc: PlcHandle) {
        *self.plc.lock().unwrap_or_else(PoisonError::into_inner) = Some(plc.clone());
        let me = self.clone();
        tokio::spawn(async move {
            let mut rx = plc.events.subscribe();
            loop {
                match rx.recv().await {
                    Ok(ev) if ev.tier == Tier::Slow && ev.dbs.iter().any(|d| d == "MEASLOG") => {
                        if let Err(e) = me.sync(&plc).await {
                            tracing::warn!("measure sync: {e}");
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    pub fn max_seq(&self) -> Result<u32, ApiError> {
        Ok(self.db.with(|c| c.query_row("SELECT COALESCE(MAX(seq), 0) FROM meas_entries", [], |r| r.get::<_, i64>(0)))? as u32)
    }

    /// Reads new HIST entries (Total changed) on demand and stores them.
    pub async fn sync(&self, plc: &PlcHandle) -> Result<usize, ApiError> {
        let snap = plc.snap();
        let Some(m) = snap.db("MEASLOG") else { return Ok(0) };
        let total = m.json["Total"].as_u64().unwrap_or(0) as u32;
        let head = m.json["Head"].as_i64().unwrap_or(0);
        let have = self.max_seq()?;
        {
            let mut lt = self.last_total.lock().unwrap_or_else(PoisonError::into_inner);
            if total == *lt && total <= have {
                return Ok(0);
            }
            *lt = total;
        }
        if total == 0 || total <= have {
            return Ok(0);
        }
        let layout = plc.layout("MEASLOG_HIST").ok_or_else(|| ApiError::Internal("MEASLOG_HIST not in contract".into()))?;
        let capacity = layout.range_of("Entry").map(|(lo, hi)| (hi - lo) / entry_size(layout).max(1)).unwrap_or(200) as u32;
        let missing = (total - have).min(capacity);
        let mut n = 0;
        for k in 0..missing {
            // newest first: index = (head - 1 - k) mod capacity
            let idx = ((head - 1 - k as i64).rem_euclid(capacity as i64)) as u32;
            let Some((lo, hi)) = layout.range_of(&format!("Entry[{idx}]")) else { continue };
            let raw = plc.read("MEASLOG_HIST", lo, (hi - lo) as usize).await.map_err(ApiError::PlcUnavailable)?;
            let mut buf = vec![0u8; hi as usize];
            buf[lo as usize..hi as usize].copy_from_slice(&raw);
            let json = plc.contract.decode_path("MEASLOG_HIST", &format!("Entry[{idx}]"), &buf)?;
            let entry: MeasureLogEntry = serde_json::from_value(json.clone())?;
            if entry.seq == 0 {
                continue;
            }
            self.db.with(|c| {
                c.execute(
                    "INSERT OR REPLACE INTO meas_entries (seq, plc, ts, kind, status, code, entry_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    (entry.seq, plc.name(), &entry.time_stamp, entry.kind, entry.status, entry.cmd.item.code, json.to_string()),
                )
            })?;
            n += 1;
        }
        if n > 0 {
            let _ = self.events.send(json!({ "kind": "measlog", "total": total, "added": n }));
        }
        Ok(n)
    }

    pub fn entries(&self, since: Option<u32>, kind: Option<u8>, code: Option<u32>, limit: usize) -> Result<(Vec<Json>, u32), ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT entry_json FROM meas_entries WHERE (?1 IS NULL OR seq > ?1) AND (?2 IS NULL OR kind = ?2) AND (?3 IS NULL OR code = ?3) ORDER BY seq DESC LIMIT ?4")?;
            let it = st.query_map((since.map(|s| s as i64), kind.map(|k| k as i64), code.map(|c| c as i64), limit as i64), |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok((rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect(), self.max_seq()?))
    }

    pub fn snapshot(&self) -> Result<Json, ApiError> {
        let plc = self.plc.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(plc) = plc else { return Err(ApiError::PlcUnavailable("measure store not attached".into())) };
        let snap = plc.snap();
        let m = snap.db("MEASLOG").ok_or_else(|| ApiError::PlcUnavailable("MEASLOG not read yet".into()))?;
        let j = &m.json;
        let by_code: Vec<Json> = j["ByCode"].as_array().cloned().unwrap_or_default().into_iter().filter(|b| b["Code"].as_u64().unwrap_or(0) != 0).collect();
        Ok(json!({ "at": m.at, "head": j["Head"], "count": j["Count"], "total": j["Total"], "capacity": 200, "stat": j["Stat"], "last": j["Last"], "by_code": by_code, "mirrored": self.max_seq()? }))
    }
}

fn entry_size(layout: &plc_layout::Layout) -> u32 {
    layout.range_of("Entry[0]").map(|(lo, hi)| hi - lo).unwrap_or(0)
}
