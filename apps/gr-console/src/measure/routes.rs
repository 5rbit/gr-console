//! `/api/measlog/*`

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
    since: Option<u32>,
    kind: Option<u8>,
    code: Option<u32>,
    limit: Option<usize>,
}

async fn snapshot(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(st.measure.snapshot()?))
}

async fn entries(State(st): State<AppState>, Query(q): Query<EntriesQuery>) -> ApiResult<Json> {
    let (entries, total) = st.measure.entries(q.since, q.kind, q.code, q.limit.unwrap_or(200).clamp(1, 5000))?;
    Ok(axum::Json(json!({ "total": total, "entries": entries })))
}

async fn reload(State(st): State<AppState>) -> ApiResult<Json> {
    let h = st.status_plc()?;
    let n = st.measure.sync(h).await?;
    Ok(axum::Json(json!({ "added": n, "snapshot": st.measure.snapshot()? })))
}

async fn export_csv(State(st): State<AppState>, Query(q): Query<EntriesQuery>) -> Result<impl IntoResponse, ApiError> {
    let (entries, _) = st.measure.entries(None, q.kind, q.code, 100000)?;
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
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"measlog.csv\"")], out))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/measlog/snapshot", get(snapshot)).route("/api/measlog/entries", get(entries)).route("/api/measlog/reload", post(reload)).route("/api/measlog/export.csv", get(export_csv))
}
