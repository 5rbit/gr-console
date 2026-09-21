//! Measurement history (M2, lead): mirrors MEASLOG / MEASLOG_HIST into sqlite on change and serves
//! snapshot / entries / stats / CSV. One store per robot — rows are keyed `(plc, seq)` because every GR
//! PLC counts its own `Seq` from 1 (migration 0006; before it GR1 and GR2 rows overwrote each other).

pub mod routes;

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::MeasureLogEntry;
use serde_json::{Value as Json, json};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::plc::{PlcHandle, Tier, ensure_db};
use crate::registry::{Registry, beads};

pub struct MeasureStore {
    db: Db,
    /// Config name of the status PLC this store mirrors (the `plc` column).
    plc_name: String,
    /// SKU 측정을 품목 규격으로 흘려보내는 곳(`registry::beads`).
    registry: Arc<Registry>,
    plc: Mutex<Option<PlcHandle>>,
    last_total: Mutex<u32>,
    pub events: broadcast::Sender<Json>,
}

impl MeasureStore {
    pub fn new(db: Db, plc_name: &str, registry: Arc<Registry>) -> Arc<MeasureStore> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(MeasureStore { db, plc_name: plc_name.to_string(), registry, plc: Mutex::new(None), last_total: Mutex::new(0), events: tx })
    }

    /// MEASLOG 의 SKU 항목(`Kind = 2`)을 비드 표본으로 적재하고 — 품목 스위치가 켜져 있으면 — 규격까지
    /// 갱신한다. 이미 적재한 표본은 조용히 건너뛴다. 실패해도 미러링은 막지 않는다.
    fn ingest_sku(&self, entry: &Json) -> Option<beads::Ingested> {
        let sample = beads::sample_from_entry(&self.plc_name, entry)?;
        match self.registry.ingest_bead_sample(&sample) {
            Ok(got) => {
                if let Some(g) = &got {
                    tracing::info!(plc = %self.plc_name, code = g.code, seq = g.seq, applied = g.applied, "SKU bead sample: {}", g.reason);
                }
                got
            }
            Err(e) => {
                tracing::warn!(plc = %self.plc_name, seq = sample.seq, "bead sample ingest: {e}");
                None
            }
        }
    }

    /// 아직 표본으로 안 읽은 SKU 항목을 뒤늦게 적재한다(이 기능이 생기기 전에 들어온 기록 · 화면이 처음
    /// 품목을 열 때). 바뀐 표본 수를 돌려준다.
    pub fn backfill_sku(&self, code: Option<u32>, limit: usize) -> Result<Vec<beads::Ingested>, ApiError> {
        let (rows, _) = self.entries(None, Some(gr_proto::MEAS_LOG_KIND_SKU), code, limit)?;
        // 옛것부터 넣어야 마지막에 반영되는 것이 최신 표본이다.
        Ok(rows.iter().rev().filter_map(|e| self.ingest_sku(e)).collect())
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
                            tracing::warn!(plc = %me.plc_name, "measure sync: {e}");
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
        Ok(self.db.with(|c| c.query_row("SELECT COALESCE(MAX(seq), 0) FROM meas_entries WHERE plc = ?1", [&self.plc_name], |r| r.get::<_, i64>(0)))? as u32)
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
        let layout = plc.layout("MEASLOG_HIST").ok_or_else(|| ApiError::BadRequest(format!("{} 설정에 MEASLOG_HIST 가 없습니다", plc.name())))?;
        let capacity = layout.range_of("Entry").map(|(lo, hi)| (hi - lo) / entry_size(layout).max(1)).unwrap_or(200);
        let missing = (total - have).min(capacity);
        let mut n = 0;
        let mut fresh: Vec<Json> = Vec::new();
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
                    "INSERT OR REPLACE INTO meas_entries (plc, seq, ts, kind, status, code, entry_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    (&self.plc_name, entry.seq, &entry.time_stamp, entry.kind, entry.status, entry.cmd.item.code, json.to_string()),
                )
            })?;
            if entry.kind == gr_proto::MEAS_LOG_KIND_SKU {
                fresh.push(json);
            }
            n += 1;
        }
        // 새로 받은 SKU 측정을 옛것부터 품목 규격으로 흘려보낸다(결정: measureSKU 의 비드 높이를 콘솔이 쓴다).
        let ingested: Vec<beads::Ingested> = fresh.iter().rev().filter_map(|e| self.ingest_sku(e)).collect();
        if n > 0 {
            let _ = self.events.send(json!({ "kind": "measlog", "plc": self.plc_name, "total": total, "added": n }));
        }
        for g in ingested {
            let _ = self.events.send(json!({ "kind": "bead_sample", "plc": self.plc_name, "code": g.code, "seq": g.seq, "applied": g.applied, "reason": g.reason }));
        }
        Ok(n)
    }

    pub fn entries(&self, since: Option<u32>, kind: Option<u8>, code: Option<u32>, limit: usize) -> Result<(Vec<Json>, u32), ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st =
                c.prepare("SELECT entry_json FROM meas_entries WHERE plc = ?1 AND (?2 IS NULL OR seq > ?2) AND (?3 IS NULL OR kind = ?3) AND (?4 IS NULL OR code = ?4) ORDER BY seq DESC LIMIT ?5")?;
            let it = st.query_map((&self.plc_name, since.map(|s| s as i64), kind.map(|k| k as i64), code.map(|c| c as i64), limit as i64), |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok((rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect(), self.max_seq()?))
    }

    pub fn snapshot(&self) -> Result<Json, ApiError> {
        let plc = self.plc.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(plc) = plc else { return Err(ApiError::PlcUnavailable(format!("{}: measure store not attached", self.plc_name))) };
        ensure_db(&plc, "MEASLOG")?;
        let snap = plc.snap();
        let m = snap.db("MEASLOG").ok_or_else(|| ApiError::PlcUnavailable(format!("{}.MEASLOG not read yet", plc.name())))?;
        let j = &m.json;
        let by_code: Vec<Json> = j["ByCode"].as_array().cloned().unwrap_or_default().into_iter().filter(|b| b["Code"].as_u64().unwrap_or(0) != 0).collect();
        Ok(
            json!({ "plc": plc.name(), "at": m.at, "head": j["Head"], "count": j["Count"], "total": j["Total"], "capacity": 200, "stat": j["Stat"], "last": j["Last"], "by_code": by_code, "mirrored": self.max_seq()? }),
        )
    }
}

fn entry_size(layout: &plc_layout::Layout) -> u32 {
    layout.range_of("Entry[0]").map(|(lo, hi)| hi - lo).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two robots count `Seq` from 1 independently: the same seq under two PLCs must be two rows, and each
    /// store sees only its own.
    #[test]
    fn stores_are_isolated_per_plc() {
        let db = Db::open_memory().unwrap();
        for (plc, seq, kind) in [("GR1", 1, 1), ("GR2", 1, 4), ("GR2", 2, 4)] {
            db.with(|c| {
                c.execute("INSERT INTO meas_entries (plc, seq, ts, kind, status, code, entry_json) VALUES (?1,?2,'t',?3,2,0,?4)", (plc, seq, kind, format!("{{\"Seq\":{seq},\"plc\":\"{plc}\"}}")))
            })
            .unwrap();
        }
        let gr1 = MeasureStore::new(db.clone(), "GR1", Registry::new(db.clone()));
        let gr2 = MeasureStore::new(db.clone(), "GR2", Registry::new(db.clone()));
        assert_eq!(gr1.max_seq().unwrap(), 1);
        assert_eq!(gr2.max_seq().unwrap(), 2);
        let (e1, _) = gr1.entries(None, None, None, 10).unwrap();
        let (e2, _) = gr2.entries(None, Some(4), None, 10).unwrap();
        assert_eq!(e1.len(), 1);
        assert_eq!(e1[0]["plc"], "GR1");
        assert_eq!(e2.iter().map(|e| e["Seq"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 1]);
    }
}
