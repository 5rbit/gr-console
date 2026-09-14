//! `/api/record/*` — high-rate measurement recorder for on-site tests (laser profile, torque inner diameter, grip
//! judgement, Z-offset calibration).
//!
//! A session reads the WEBMON areas the analyses need every `rate_ms` with direct S7 reads (the webmon tier is too slow
//! for a Z profile) and appends one JSON line per sample to `<data_dir>/records/<id>/samples.jsonl`; `mark` lines
//! annotate test steps. `meta.json` carries start / stop snapshots — laser and torque PARA members, LASERDIAG state and
//! the entries added during the session, MEASLOG total and the per-kind last entries recorded during the session — so a
//! session can be analysed without the PLC.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use tokio::sync::{Mutex, mpsc, watch};

use crate::error::{ApiError, ApiResult};
use crate::plc::PlcHandle;
use crate::state::AppState;
use crate::util::now_str;

const DB: &str = "WEBMON";
/// WEBMON member path -> sample key path (dot = nested object). Sku / Floor / event codes are left out to keep lines small.
const AREAS: [(&str, &str); 12] = [
    ("Mode", "Mode"),
    ("Stat.Task.Now", "Task"),
    ("Proc.Step", "Step"),
    ("Alarm.Fault", "Alarm.Fault"),
    ("Alarm.Warn", "Alarm.Warn"),
    ("Alarm.FaultCode", "Alarm.FaultCode"),
    ("Alarm.WarnCode", "Alarm.WarnCode"),
    ("Axis", "Axis"),
    ("Gripper", "Gripper"),
    ("Measure.Item", "Measure.Item"),
    ("Measure.LastBead", "Measure.LastBead"),
    ("MeasLog", "MeasLog"),
];
/// Byte gap below which two read ranges are fetched as one S7 request.
const MERGE_GAP: u32 = 300;
const PARA_TASK_KEYS: [&str; 11] = [
    "G_PickTorq_Factor",
    "G_DropTorq_Factor",
    "G_TorqLimitAllowDistance",
    "G_TorqDetect_OnRatio",
    "G_TorqDetect_OffRatio",
    "G_TorqDetect_DwellTime",
    "G_MeasureTorq_Factor",
    "pickRetryCountMax",
    "Pick_GripExtra",
    "G_MeasureTorq_Offset",
    "G_MeasureTorq_OffsetManual",
];
const PARA_SENSOR_KEYS: [&str; 22] = [
    "ItemDetectDistance_H",
    "GIDL_Offset",
    "GIDF_Offset",
    "GIDR_Offset",
    "GIDB_Offset",
    "GIDL_ZOffset",
    "GIDF_ZOffset",
    "GIDR_ZOffset",
    "GIDB_ZOffset",
    "LaserDiag_SpreadLimit",
    "LaserDiag_PlanarLimit",
    "LaserDiag_JumpDist",
    "LaserDiag_JumpCountLimit",
    "LaserDiag_ConsecLimit",
    "LaserDiag_BiasLimit",
    "LaserDiag_BiasMinSamples",
    "LaserDiag_BiasAlpha",
    "LaserZCal_SampleCount",
    "LaserZCal_MaxStdDev",
    "LaserZCal_MaxCorrection",
    "LaserInnerDia_Offset",
    "LaserFit_TbrTolerance",
];
const RATE_MIN_MS: u64 = 20;
const RATE_DEFAULT_MS: u64 = 50;
/// A forgotten session stops itself.
const MAX_MINUTES: u64 = 30;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub note: String,
    pub plc: String,
    pub rate_ms: u64,
    pub started_at: String,
    pub stopped_at: Option<String>,
    pub samples: u64,
    pub marks: u64,
    pub read_errors: u64,
    pub start: Json,
    pub end: Json,
}

#[derive(Deserialize)]
pub struct StartReq {
    pub label: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub note: String,
    pub rate_ms: Option<u64>,
}

#[derive(Default)]
struct Counters {
    samples: AtomicU64,
    marks: AtomicU64,
    errors: AtomicU64,
}

struct Active {
    meta: SessionMeta,
    plc: PlcHandle,
    stop: watch::Sender<bool>,
    marks: mpsc::UnboundedSender<String>,
    counters: Arc<Counters>,
    task: tokio::task::JoinHandle<()>,
}

pub struct Recorder {
    dir: PathBuf,
    active: Mutex<Option<Active>>,
}

impl Recorder {
    pub fn new(dir: PathBuf) -> Arc<Recorder> {
        Arc::new(Recorder { dir, active: Mutex::new(None) })
    }

    fn session_dir(&self, id: &str) -> Result<PathBuf, ApiError> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(ApiError::BadRequest(format!("bad session id {id:?}")));
        }
        Ok(self.dir.join(id))
    }

    /// Active session with live counters. A session whose task ended on its own (time limit) is finalized here.
    pub async fn status(&self) -> Option<SessionMeta> {
        let mut g = self.active.lock().await;
        if g.as_ref().is_some_and(|a| a.task.is_finished()) {
            let a = g.take()?;
            drop(g);
            finish(&self.dir, a).await;
            return None;
        }
        g.as_ref().map(|a| live(&a.meta, &a.counters))
    }

    pub async fn start(&self, plc: PlcHandle, req: StartReq) -> Result<SessionMeta, ApiError> {
        let mut g = self.active.lock().await;
        if let Some(a) = g.as_ref()
            && !a.task.is_finished()
        {
            return Err(ApiError::Conflict(format!("recording '{}' is running", a.meta.label)));
        }
        if let Some(a) = g.take() {
            finish(&self.dir, a).await;
        }
        let rate_ms = req.rate_ms.unwrap_or(RATE_DEFAULT_MS).max(RATE_MIN_MS);
        let started_at = now_str();
        let id = session_id(&started_at);
        let sdir = self.dir.join(&id);
        std::fs::create_dir_all(&sdir).map_err(|e| ApiError::Internal(format!("{}: {e}", sdir.display())))?;
        let meta = SessionMeta {
            id,
            label: req.label.trim().to_string(),
            kind: req.kind,
            note: req.note,
            plc: plc.name().to_string(),
            rate_ms,
            started_at,
            start: snapshot(&plc, None, None).await,
            ..SessionMeta::default()
        };
        write_meta(&self.dir, &meta);
        let (stop, stop_rx) = watch::channel(false);
        let (marks, marks_rx) = mpsc::unbounded_channel();
        let counters = Arc::new(Counters::default());
        let task = tokio::spawn(run(plc.clone(), sdir.join("samples.jsonl"), Duration::from_millis(rate_ms), stop_rx, marks_rx, counters.clone()));
        *g = Some(Active { meta: meta.clone(), plc, stop, marks, counters, task });
        Ok(meta)
    }

    pub async fn stop(&self) -> Result<SessionMeta, ApiError> {
        let a = self.active.lock().await.take().ok_or_else(|| ApiError::Conflict("no recording is running".into()))?;
        Ok(finish(&self.dir, a).await)
    }

    pub async fn mark(&self, text: String) -> Result<(), ApiError> {
        let g = self.active.lock().await;
        let a = g.as_ref().ok_or_else(|| ApiError::Conflict("no recording is running".into()))?;
        a.marks.send(text).map_err(|_| ApiError::Conflict("recording task ended".into()))
    }

    /// Finished sessions, newest first (the active one is reported separately).
    pub async fn list(&self) -> Vec<SessionMeta> {
        let active = self.status().await.map(|m| m.id);
        let mut v: Vec<SessionMeta> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path().join("meta.json")).ok())
            .filter_map(|t| serde_json::from_str::<SessionMeta>(&t).ok())
            .filter(|m| active.as_ref() != Some(&m.id))
            .collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    /// Metadata and samples (marks always kept, data thinned to at most `max`).
    pub fn load(&self, id: &str, max: usize) -> Result<(SessionMeta, Vec<Json>), ApiError> {
        let sdir = self.session_dir(id)?;
        let meta: SessionMeta = std::fs::read_to_string(sdir.join("meta.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;
        Ok((meta, thin(read_lines(&sdir.join("samples.jsonl")), max)))
    }

    pub async fn delete(&self, id: &str) -> Result<(), ApiError> {
        let sdir = self.session_dir(id)?;
        if self.status().await.is_some_and(|m| m.id == id) {
            return Err(ApiError::Conflict("recording in progress".into()));
        }
        std::fs::remove_dir_all(&sdir).map_err(|e| ApiError::NotFound(format!("session {id}: {e}")))
    }
}

/// Stops the task, takes the end snapshot and writes the final `meta.json`.
async fn finish(dir: &Path, a: Active) -> SessionMeta {
    let Active { meta, plc, stop, marks: _, counters, task } = a;
    let _ = stop.send(true);
    let _ = task.await;
    let mut end = live(&meta, &counters);
    end.stopped_at = Some(now_str());
    let since_laser = meta.start["laser"]["Total"].as_u64();
    let since_meas = meta.start["measlog_total"].as_u64();
    end.end = snapshot(&plc, Some(since_laser.unwrap_or(0)), Some(since_meas.unwrap_or(0))).await;
    write_meta(dir, &end);
    end
}

fn live(meta: &SessionMeta, n: &Counters) -> SessionMeta {
    SessionMeta { samples: n.samples.load(Ordering::Relaxed), marks: n.marks.load(Ordering::Relaxed), read_errors: n.errors.load(Ordering::Relaxed), ..meta.clone() }
}

async fn run(plc: PlcHandle, path: PathBuf, rate: Duration, mut stop: watch::Receiver<bool>, mut marks: mpsc::UnboundedReceiver<String>, n: Arc<Counters>) {
    let Some(layout) = plc.layout(DB).cloned() else {
        tracing::error!(plc = %plc.name(), "record: {DB} not in contract");
        return;
    };
    let plan = merge_ranges(AREAS.iter().filter_map(|(p, _)| layout.range_of(p)).collect(), MERGE_GAP);
    let mut w = match std::fs::File::create(&path) {
        Ok(f) => BufWriter::new(f),
        Err(e) => {
            tracing::error!(path = %path.display(), "record: {e}");
            return;
        }
    };
    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs(MAX_MINUTES * 60);
    let mut tick = tokio::time::interval(rate);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut unflushed = 0u32;
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            Some(text) = marks.recv() => {
                let line = json!({ "t": t0.elapsed().as_millis() as u64, "at": now_str(), "mark": text });
                let _ = writeln!(w, "{line}");
                let _ = w.flush();
                n.marks.fetch_add(1, Ordering::Relaxed);
            }
            _ = tick.tick() => {
                if Instant::now() >= deadline {
                    tracing::warn!("record: stopped after {MAX_MINUTES} min");
                    break;
                }
                match read_sample(&plc, &plan, layout.size as usize).await {
                    Ok(mut s) => {
                        s["t"] = json!(t0.elapsed().as_millis() as u64);
                        s["at"] = json!(now_str());
                        let _ = writeln!(w, "{s}");
                        n.samples.fetch_add(1, Ordering::Relaxed);
                        unflushed += 1;
                        if unflushed >= 20 {
                            let _ = w.flush();
                            unflushed = 0;
                        }
                    }
                    Err(e) => {
                        if n.errors.fetch_add(1, Ordering::Relaxed).is_multiple_of(50) {
                            tracing::warn!("record read: {e}");
                        }
                    }
                }
            }
        }
    }
    let _ = w.flush();
}

/// Sorted, merged `[start, end)` ranges; neighbours closer than `gap` bytes become one range.
fn merge_ranges(mut ranges: Vec<(u32, u32)>, gap: u32) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (lo, hi) in ranges {
        match out.last_mut() {
            Some(last) if lo <= last.1 + gap => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

async fn read_sample(plc: &PlcHandle, plan: &[(u32, u32)], size: usize) -> Result<Json, String> {
    let mut buf = vec![0u8; size];
    for &(lo, hi) in plan {
        let bytes = plc.read(DB, lo, (hi - lo) as usize).await?;
        let start = lo as usize;
        let end = (start + bytes.len()).min(size);
        buf[start..end].copy_from_slice(&bytes[..end - start]);
    }
    let mut obj = json!({});
    for (path, key) in AREAS {
        let v = plc.contract.decode_path(DB, path, &buf).map_err(|e| format!("{path}: {e}"))?;
        insert_path(&mut obj, key, v);
    }
    Ok(obj)
}

/// Sets `root.a.b = v`, creating the objects on the way.
fn insert_path(root: &mut Json, key: &str, v: Json) {
    let mut cur = root;
    let mut parts = key.split('.').peekable();
    while let Some(p) = parts.next() {
        let Json::Object(map) = cur else { return };
        if parts.peek().is_none() {
            map.insert(p.to_string(), v);
            return;
        }
        cur = map.entry(p.to_string()).or_insert_with(|| json!({}));
    }
}

/// Whole DB read now (the slow tier can be seconds old); falls back to the snapshot. `None` when the DB is not verified.
async fn fresh(plc: &PlcHandle, db: &str) -> Option<Json> {
    if !plc.health_now().db_ok(db) {
        return None;
    }
    let size = plc.layout(db)?.size as usize;
    match plc.read(db, 0, size).await {
        Ok(b) => plc.contract.decode_db(db, &b).ok(),
        Err(_) => plc.snap().db(db).map(|d| (*d.json).clone()),
    }
}

/// Start (`since_* = None`) or end snapshot. The end one adds what was recorded during the session.
async fn snapshot(plc: &PlcHandle, since_laser: Option<u64>, since_meas: Option<u64>) -> Json {
    let para = fresh(plc, "PARA").await;
    let laser = fresh(plc, "LASERDIAG").await;
    let meas = fresh(plc, "MEASLOG").await;
    json!({
        "at": now_str(),
        "para_task": pick(para.as_ref().and_then(|p| p.get("Task")), &PARA_TASK_KEYS),
        "para_sensor": pick(para.as_ref().and_then(|p| p.get("Sensor")), &PARA_SENSOR_KEYS),
        "laser": laser.as_ref().map(|l| json!({ "Total": l["Total"], "Sensor": l["Sensor"], "ZCal": l["ZCal"] })),
        "laser_entries": since_laser.map(|t| entries_since(laser.as_ref().map(|l| &l["Entry"]), t)),
        "measlog_total": meas.as_ref().map(|m| m["Total"].clone()),
        "measlog_last": since_meas.map(|t| entries_since(meas.as_ref().map(|m| &m["Last"]), t)),
    })
}

fn pick(group: Option<&Json>, keys: &[&str]) -> Json {
    let mut out = serde_json::Map::new();
    if let Some(g) = group {
        for k in keys {
            if let Some(v) = g.get(*k) {
                out.insert((*k).to_string(), v.clone());
            }
        }
    }
    Json::Object(out)
}

/// Entries of a ring / per-kind array with `Seq` greater than `seq`, oldest first.
fn entries_since(arr: Option<&Json>, seq: u64) -> Vec<Json> {
    let mut v: Vec<Json> = arr.and_then(Json::as_array).map(|a| a.iter().filter(|e| e["Seq"].as_u64().is_some_and(|s| s > seq)).cloned().collect()).unwrap_or_default();
    v.sort_by_key(|e| e["Seq"].as_u64().unwrap_or(0));
    v
}

fn session_id(started_at: &str) -> String {
    let stamp: String = started_at.chars().take(19).filter(char::is_ascii_digit).collect();
    let uid = uuid::Uuid::new_v4().simple().to_string();
    format!("{stamp}-{}", &uid[..6])
}

fn write_meta(dir: &Path, meta: &SessionMeta) {
    let p = dir.join(&meta.id).join("meta.json");
    match serde_json::to_string_pretty(meta) {
        Ok(t) => {
            if let Err(e) = std::fs::write(&p, t) {
                tracing::error!(path = %p.display(), "record meta: {e}");
            }
        }
        Err(e) => tracing::error!("record meta: {e}"),
    }
}

fn read_lines(path: &Path) -> Vec<Json> {
    let Ok(f) = std::fs::File::open(path) else { return Vec::new() };
    BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect()
}

/// Keeps every mark and at most `max` data samples (even stride).
fn thin(lines: Vec<Json>, max: usize) -> Vec<Json> {
    let data = lines.iter().filter(|l| l.get("mark").is_none()).count();
    let stride = data.div_ceil(max.max(1)).max(1);
    let mut i = 0usize;
    lines
        .into_iter()
        .filter(|l| {
            if l.get("mark").is_some() {
                return true;
            }
            i += 1;
            (i - 1).is_multiple_of(stride)
        })
        .collect()
}

const CSV_HEAD: &str = "t_ms,at,mark,mode,step,task_type,code,id,od,height,cell_z,z,z_target,z_speed,g,g_target,g_torque,gid_l,gid_f,gid_r,gid_b,fld,item_detect,commanded,stopped,at_command,torque_reached,torque_stop,stall,stall_no_torque,stall_time,item_busy,in_busy,out_busy,pick_busy,last_bid_pos,fault0,warn0";

fn csv_row(s: &Json) -> String {
    let n = |v: &Json| if v.is_null() { String::new() } else { v.to_string() };
    let b = |v: &Json| match v.as_bool() {
        Some(true) => "1".to_string(),
        Some(false) => "0".to_string(),
        None => String::new(),
    };
    let st = &s["Gripper"]["State"];
    let item = &s["Task"]["Item"];
    let mark = s["mark"].as_str().map(|m| format!("\"{}\"", m.replace('"', "\"\""))).unwrap_or_default();
    let cols = [
        n(&s["t"]),
        s["at"].as_str().unwrap_or("").to_string(),
        mark,
        n(&s["Mode"]),
        n(&s["Step"]["Now"]),
        n(&s["Task"]["TaskType"]),
        n(&item["Code"]),
        n(&item["InnerDiameter"]),
        n(&item["OuterDiameter"]),
        n(&item["Height"]),
        n(&s["Task"]["Cell"]["Position"][2]),
        n(&s["Axis"][2]["Position"]),
        n(&s["Axis"][2]["Target"]),
        n(&s["Axis"][2]["Speed"]),
        n(&s["Axis"][3]["Position"]),
        n(&s["Axis"][3]["Target"]),
        n(&s["Axis"][3]["Torque"]),
        n(&s["Gripper"]["GID"][0]),
        n(&s["Gripper"]["GID"][1]),
        n(&s["Gripper"]["GID"][2]),
        n(&s["Gripper"]["GID"][3]),
        n(&s["Gripper"]["FLD"]),
        b(&s["Gripper"]["ItemDetect"]),
        b(&st["Commanded"]),
        b(&st["Stopped"]),
        b(&st["AtCommand"]),
        b(&st["TorqueReached"]),
        b(&st["TorqueStop"]),
        b(&st["Stall"]),
        b(&st["StallNoTorque"]),
        n(&st["StallTime"]),
        b(&s["Measure"]["Item"]["Busy"]),
        b(&s["Measure"]["Item"]["InBusy"]),
        b(&s["Measure"]["Item"]["OutBusy"]),
        b(&s["Measure"]["LastBead"]["Busy"]),
        n(&s["Measure"]["LastBead"]["LastBidPos"]),
        n(&s["Alarm"]["FaultCode"][0]),
        n(&s["Alarm"]["WarnCode"][0]),
    ];
    cols.join(",")
}

#[derive(Deserialize)]
struct LoadQuery {
    max: Option<usize>,
}

#[derive(Deserialize)]
struct MarkReq {
    text: String,
}

async fn overview(State(st): State<AppState>) -> ApiResult<Json> {
    let active = st.recorder.status().await;
    let sessions = st.recorder.list().await;
    Ok(axum::Json(json!({ "active": active, "sessions": sessions })))
}

async fn start(State(st): State<AppState>, axum::Json(req): axum::Json<StartReq>) -> ApiResult<Json> {
    if req.label.trim().is_empty() {
        return Err(ApiError::BadRequest("label is required".into()));
    }
    let h = st.status_plc()?.clone();
    let hl = h.health_now();
    if !hl.connected {
        return Err(ApiError::PlcUnavailable(format!("{} S7 not connected", h.name())));
    }
    if !hl.db_ok(DB) {
        return Err(ApiError::LayoutMismatch { plc: h.name().into(), db: DB.into(), detail: hl.mismatch_detail(DB) });
    }
    let meta = st.recorder.start(h, req).await?;
    Ok(axum::Json(json!(meta)))
}

async fn stop(State(st): State<AppState>) -> ApiResult<Json> {
    let meta = st.recorder.stop().await?;
    Ok(axum::Json(json!(meta)))
}

async fn mark(State(st): State<AppState>, axum::Json(req): axum::Json<MarkReq>) -> ApiResult<Json> {
    st.recorder.mark(req.text).await?;
    Ok(axum::Json(json!({ "ok": true })))
}

async fn session(State(st): State<AppState>, UrlPath(id): UrlPath<String>, Query(q): Query<LoadQuery>) -> ApiResult<Json> {
    let (meta, samples) = st.recorder.load(&id, q.max.unwrap_or(20_000).clamp(100, 200_000))?;
    Ok(axum::Json(json!({ "meta": meta, "samples": samples })))
}

async fn remove(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> ApiResult<Json> {
    st.recorder.delete(&id).await?;
    Ok(axum::Json(json!({ "ok": true })))
}

async fn export_csv(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> Result<impl IntoResponse, ApiError> {
    let (meta, samples) = st.recorder.load(&id, usize::MAX)?;
    let mut out = String::from("\u{feff}");
    out.push_str(CSV_HEAD);
    out.push('\n');
    for s in &samples {
        out.push_str(&csv_row(s));
        out.push('\n');
    }
    let disposition = format!("attachment; filename=\"record-{}.csv\"", meta.id);
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, disposition)], out))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/record", get(overview))
        .route("/api/record/start", post(start))
        .route("/api/record/stop", post(stop))
        .route("/api/record/mark", post(mark))
        .route("/api/record/{id}", get(session).delete(remove))
        .route("/api/record/{id}/export.csv", get(export_csv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_merge_when_close() {
        assert_eq!(merge_ranges(vec![(4886, 5048), (16, 18), (224, 340), (5048, 5346), (4620, 4626)], 300), vec![(16, 340), (4620, 5346)]);
        assert_eq!(merge_ranges(vec![(0, 10), (400, 410)], 300), vec![(0, 10), (400, 410)]);
        assert_eq!(merge_ranges(vec![], 300), Vec::<(u32, u32)>::new());
    }

    #[test]
    fn insert_path_nests() {
        let mut o = json!({});
        insert_path(&mut o, "Alarm.Fault", json!(true));
        insert_path(&mut o, "Alarm.FaultCode", json!([1, 0]));
        insert_path(&mut o, "Mode", json!(32));
        assert_eq!(o, json!({ "Alarm": { "Fault": true, "FaultCode": [1, 0] }, "Mode": 32 }));
    }

    #[test]
    fn entries_since_filters_and_sorts() {
        let arr = json!([{ "Seq": 5 }, { "Seq": 2 }, { "Seq": 9 }, { "Seq": 0 }]);
        let seqs: Vec<u64> = entries_since(Some(&arr), 2).iter().map(|e| e["Seq"].as_u64().unwrap()).collect();
        assert_eq!(seqs, vec![5, 9]);
        assert!(entries_since(None, 0).is_empty());
    }

    #[test]
    fn thin_keeps_marks_and_strides_data() {
        let mut lines: Vec<Json> = (0..10).map(|i| json!({ "t": i })).collect();
        lines.insert(3, json!({ "t": 3, "mark": "rotate" }));
        let out = thin(lines, 5);
        assert_eq!(out.iter().filter(|l| l.get("mark").is_some()).count(), 1);
        assert_eq!(out.iter().filter(|l| l.get("mark").is_none()).count(), 5);
        assert_eq!(thin((0..3).map(|i| json!({ "t": i })).collect(), usize::MAX).len(), 3);
    }

    #[test]
    fn session_ids_are_path_safe() {
        let id = session_id("2026-09-13T21:30:05+09:00");
        assert!(id.starts_with("20260913213005-"), "{id}");
        let r = Recorder::new(std::env::temp_dir().join("gr-record-test"));
        assert!(r.session_dir(&id).is_ok());
        assert!(r.session_dir("../etc").is_err());
        assert!(r.session_dir("").is_err());
    }

    #[test]
    fn csv_row_flattens_sample() {
        let s = json!({
            "t": 150, "at": "2026-09-13T21:30:05+09:00", "Mode": 32, "Step": { "Now": 500 },
            "Task": { "TaskType": 65, "Item": { "Code": 7, "InnerDiameter": 406.0, "OuterDiameter": 632.0, "Height": 205.0 }, "Cell": { "Position": [1.0, 2.0, 1500.0] } },
            "Axis": [{}, {}, { "Position": 1710.5, "Target": 1700.0, "Speed": -30.0 }, { "Position": 371.0, "Target": 376.0, "Torque": 48.5 }],
            "Gripper": { "GID": [15.0, 16.0, 14.0, 0.0], "FLD": 210.0, "ItemDetect": true, "State": { "TorqueStop": true, "StallTime": 0.4 } },
            "Measure": { "Item": { "Busy": true }, "LastBead": { "Busy": false, "LastBidPos": 24.5 } },
            "Alarm": { "FaultCode": [4025], "WarnCode": [0] }
        });
        let row = csv_row(&s);
        assert_eq!(row.split(',').count(), CSV_HEAD.split(',').count());
        assert!(row.starts_with("150,2026-09-13T21:30:05+09:00,,32,500,65,7,406.0,632.0,205.0,1500.0,1710.5,1700.0,-30.0,371.0,376.0,48.5,15.0,16.0,14.0,0.0,210.0,1"), "{row}");
        assert!(row.ends_with(",0.4,1,,,0,24.5,4025,0"), "{row}");
        assert!(csv_row(&json!({ "t": 3, "mark": "a,\"b\"" })).starts_with("3,,\"a,\"\"b\"\"\""));
    }
}
