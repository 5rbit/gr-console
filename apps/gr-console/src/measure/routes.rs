//! `/api/measlog/*` — every route takes `?robot=<id>` (absent = default robot).

use axum::Router;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Deserialize)]
struct EntriesQuery {
    robot: Option<u8>,
    since: Option<u32>,
    kind: Option<u8>,
    code: Option<u32>,
    limit: Option<usize>,
    /// 내보내기 기간(`YYYY-MM-DD`, 양끝 포함).
    from: Option<String>,
    to: Option<String>,
}

#[derive(Deserialize)]
struct RobotQuery {
    robot: Option<u8>,
}

async fn snapshot(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    Ok(axum::Json(st.robot(q.robot)?.measure.snapshot()?))
}

async fn entries(State(st): State<AppState>, Query(q): Query<EntriesQuery>) -> ApiResult<Json> {
    let r = st.robot(q.robot)?;
    let (entries, total) = r.measure.entries(q.since, q.kind, q.code, q.limit.unwrap_or(200).clamp(1, 5000))?;
    Ok(axum::Json(json!({ "plc": r.plc, "total": total, "entries": entries })))
}

async fn reload(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let (r, h) = st.robot_and_plc(q.robot)?;
    crate::plc::ensure_db(h, "MEASLOG_HIST")?;
    let n = r.measure.sync(h).await?;
    Ok(axum::Json(json!({ "added": n, "snapshot": r.measure.snapshot()? })))
}

async fn export_csv(State(st): State<AppState>, Query(q): Query<EntriesQuery>) -> Result<impl IntoResponse, ApiError> {
    let r = st.robot(q.robot)?;
    let (entries, _) = r.measure.entries(None, q.kind, q.code, 100000)?;
    let mut out =
        String::from("\u{feff}seq,time,kind,status,work_id,task_id,task_type,cell_id,code,cmd_count,cmd_id,cmd_od,cmd_height,cmd_x,cmd_y,cmd_z,cmd_g,cell_z,d_inner_dia,d_height,d_z,d_offset,d_count");
    for i in 0..20 {
        out.push_str(&format!(",data{i}"));
    }
    out.push('\n');
    for e in entries {
        let c = &e["Cmd"];
        let it = &c["Item"];
        let pos = c["Position"].as_array().cloned().unwrap_or_default();
        let cp = c["Cell"]["Position"].as_array().cloned().unwrap_or_default();
        let d = &e["Delta"];
        let f = |v: Option<&Json>| v.map(|x| x.to_string()).unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            e["Seq"],
            e["TimeStamp"].as_str().unwrap_or(""),
            e["Kind"],
            e["Status"],
            c["WorkId"],
            c["TaskId"],
            c["TaskType"],
            c["Cell"]["Id"],
            it["Code"],
            it["Count"],
            it["InnerDiameter"],
            it["OuterDiameter"],
            it["Height"],
            f(pos.first()),
            f(pos.get(1)),
            f(pos.get(2)),
            f(pos.get(3)),
            f(cp.get(2)),
            d["InnerDia"],
            d["Height"],
            d["Z"],
            d["Offset"],
            d["Count"]
        ));
        for i in 0..20 {
            out.push(',');
            out.push_str(&f(e["Data"].as_array().and_then(|a| a.get(i))));
        }
        out.push('\n');
    }
    let safe: String = r.plc.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    let disposition = format!("attachment; filename=\"measlog-{safe}.csv\"");
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, disposition)], out))
}

/// 행 한도 — 넘으면 Info 시트에 Truncated 로 남긴다(PLC 순환 버퍼는 200 건이라 평소엔 닿지 않는다).
const XLSX_MAX_ROWS: usize = 100_000;

fn date_arg(v: Option<&str>) -> Result<Option<String>, ApiError> {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if s.len() == 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' && s.chars().filter(char::is_ascii_digit).count() == 8 => Ok(Some(s.to_string())),
        Some(s) => Err(ApiError::BadRequest(format!("날짜는 YYYY-MM-DD: {s}"))),
    }
}

/// 규격 Excel(`measure::xlsx`, docs/measure-export.md) — 콘솔 DB 에 쌓인 전체 이력에서 조건에 맞는 것.
async fn export_xlsx(State(st): State<AppState>, Query(q): Query<EntriesQuery>) -> Result<axum::response::Response, ApiError> {
    use crate::registry::routes::{stamp, xlsx_response};
    let r = st.robot(q.robot)?;
    let (from, to) = (date_arg(q.from.as_deref())?, date_arg(q.to.as_deref())?);
    let entries = r.measure.entries_between(q.kind, q.code, from.as_deref(), to.as_deref(), XLSX_MAX_ROWS + 1)?;
    let truncated = entries.len() > XLSX_MAX_ROWS;
    let entries = &entries[..entries.len().min(XLSX_MAX_ROWS)];
    let meta = super::xlsx::Meta { plc: r.plc.clone(), robot: r.name.clone(), exported_at: crate::util::now_str(), kind: q.kind, code: q.code, from, to, truncated };
    let bytes = super::xlsx::build(entries, &meta).map_err(|e| ApiError::Internal(format!("xlsx: {e}")))?;
    let safe: String = r.plc.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    Ok(xlsx_response(bytes, &format!("measlog_{safe}_{}.xlsx", stamp())))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/measlog/snapshot", get(snapshot))
        .route("/api/measlog/entries", get(entries))
        .route("/api/measlog/reload", post(reload))
        .route("/api/measlog/export.csv", get(export_csv))
        .route("/api/measlog/export.xlsx", get(export_xlsx))
}
