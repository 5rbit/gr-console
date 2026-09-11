//! `/api/scenarios*` — CRUD, import/export, validation, run control, history and the run stream.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::runner::RunOptions;
use super::{RunState, Scenario, io};
use crate::error::{ApiError, ApiResult};
use crate::sse::broadcast_sse;
use crate::state::AppState;

const BODY_LIMIT: usize = 8 * 1024 * 1024;

fn not_found(id: &str) -> ApiError {
    ApiError::NotFound(format!("scenario {id}"))
}

// ---- CRUD

async fn list(State(st): State<AppState>) -> ApiResult<Vec<Scenario>> {
    Ok(axum::Json(st.scenario.list()?))
}
async fn one(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Scenario> {
    st.scenario.get(&id)?.map(axum::Json).ok_or_else(|| not_found(&id))
}
async fn create(State(st): State<AppState>, axum::Json(s): axum::Json<Scenario>) -> ApiResult<Scenario> {
    Ok(axum::Json(st.scenario.save(Scenario { id: String::new(), ..s })?))
}
async fn update(State(st): State<AppState>, Path(id): Path<String>, axum::Json(s): axum::Json<Scenario>) -> ApiResult<Scenario> {
    let existing = st.scenario.get(&id)?.ok_or_else(|| not_found(&id))?;
    Ok(axum::Json(st.scenario.save(Scenario { id, created_at: existing.created_at, ..s })?))
}
async fn delete(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.scenario.delete(&id)? })))
}

// ---- import / export

#[derive(Deserialize)]
struct ImportQuery {
    format: Option<String>,
    name: Option<String>,
}

/// `POST /api/scenarios/import` — multipart `file` or a raw JSON/CSV body. `?format=json|csv`
/// (default: from the file name, else sniffed from the content). Saves and returns the scenario.
async fn import(State(st): State<AppState>, Query(q): Query<ImportQuery>, req: Request) -> ApiResult<Scenario> {
    let ct = req.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let (bytes, filename): (Vec<u8>, Option<String>) = if ct.starts_with("multipart/form-data") {
        let mut mp = Multipart::from_request(req, &()).await.map_err(|e| ApiError::BadRequest(format!("multipart: {e}")))?;
        let mut found = None;
        while let Some(field) = mp.next_field().await.map_err(|e| ApiError::BadRequest(format!("multipart: {e}")))? {
            let is_file = matches!(field.name(), Some("file")) || field.file_name().is_some();
            if is_file {
                let name = field.file_name().map(str::to_string);
                let data = field.bytes().await.map_err(|e| ApiError::BadRequest(format!("multipart: {e}")))?;
                found = Some((data.to_vec(), name));
                break;
            }
        }
        found.ok_or_else(|| ApiError::BadRequest("multipart: no 'file' field".into()))?
    } else {
        let b = axum::body::to_bytes(req.into_body(), BODY_LIMIT).await.map_err(|e| ApiError::BadRequest(format!("body: {e}")))?;
        (b.to_vec(), None)
    };
    if bytes.is_empty() {
        return Err(ApiError::BadRequest("empty import".into()));
    }
    let stem = filename.as_deref().map(|f| std::path::Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or(f).to_string());
    let ext = filename.as_deref().and_then(|f| std::path::Path::new(f).extension().and_then(|e| e.to_str())).map(|e| e.to_ascii_lowercase());
    let format = match q.format.as_deref().map(str::to_ascii_lowercase) {
        Some(f) if f == "json" || f == "csv" => f,
        Some(other) => return Err(ApiError::BadRequest(format!("unknown format {other}"))),
        None => match ext.as_deref() {
            Some("json") => "json".into(),
            Some("csv") => "csv".into(),
            _ => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]).trim_start_matches('\u{feff}').trim_start().to_string();
                if head.starts_with('{') || head.starts_with('[') || ct.contains("json") { "json".into() } else { "csv".into() }
            }
        },
    };
    let mut s = if format == "json" {
        io::from_json(&bytes)?
    } else {
        let text = String::from_utf8(bytes).map_err(|e| ApiError::BadRequest(format!("csv: not utf-8 ({e})")))?;
        io::from_csv(&text, stem.as_deref().unwrap_or("imported"))?
    };
    if let Some(n) = q.name.filter(|n| !n.trim().is_empty()) {
        s.name = n;
    }
    if s.name.trim().is_empty() {
        s.name = stem.unwrap_or_else(|| "imported".into());
    }
    Ok(axum::Json(st.scenario.save(s)?))
}

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,
}

fn safe_filename(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() { "scenario".into() } else { s }
}

/// `GET /api/scenarios/{id}/export?format=json|csv`
async fn export(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<ExportQuery>) -> Result<Response, ApiError> {
    let s = st.scenario.get(&id)?.ok_or_else(|| not_found(&id))?;
    let format = q.format.as_deref().unwrap_or("json").to_ascii_lowercase();
    let (body, ctype, ext) = match format.as_str() {
        "json" => (serde_json::to_string_pretty(&io::to_json(&s))?, "application/json; charset=utf-8", "json"),
        "csv" => (format!("\u{feff}{}", io::to_csv(&s)?), "text/csv; charset=utf-8", "csv"),
        other => return Err(ApiError::BadRequest(format!("unknown format {other}"))),
    };
    let ascii = safe_filename(&s.name);
    let utf8: String = s.name.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let disposition = format!("attachment; filename=\"{ascii}.{ext}\"; filename*=UTF-8''{utf8}.{ext}");
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

// ---- validation

/// `POST /api/scenarios/{id}/validate` — validates the stored scenario, or the document in the
/// (optional) JSON body so the editor can check an unsaved draft.
async fn validate(State(st): State<AppState>, Path(id): Path<String>, body: Bytes) -> ApiResult<Vec<io::Issue>> {
    let s = if body.iter().any(|b| !b.is_ascii_whitespace()) { serde_json::from_slice::<Scenario>(&body)? } else { st.scenario.get(&id)?.ok_or_else(|| not_found(&id))? };
    Ok(axum::Json(io::validate(&st, &s)?))
}

// ---- run control

async fn run_start(State(st): State<AppState>, Path(id): Path<String>, body: Bytes) -> ApiResult<RunState> {
    let opts: RunOptions = if body.iter().any(|b| !b.is_ascii_whitespace()) { serde_json::from_slice(&body)? } else { RunOptions::default() };
    let s = st.scenario.get(&id)?.ok_or_else(|| not_found(&id))?;
    let runner = st.scenario.clone();
    Ok(axum::Json(runner.start(st.clone(), s, opts)?))
}
async fn run_now(State(st): State<AppState>) -> ApiResult<RunState> {
    Ok(axum::Json(st.scenario.current()))
}
async fn run_pause(State(st): State<AppState>) -> ApiResult<RunState> {
    Ok(axum::Json(st.scenario.pause()?))
}
async fn run_resume(State(st): State<AppState>) -> ApiResult<RunState> {
    Ok(axum::Json(st.scenario.resume()?))
}
async fn run_stop(State(st): State<AppState>) -> ApiResult<RunState> {
    Ok(axum::Json(st.scenario.stop()?))
}

#[derive(Deserialize)]
struct RunsQuery {
    limit: Option<usize>,
}
async fn runs(State(st): State<AppState>, Query(q): Query<RunsQuery>) -> ApiResult<Vec<RunState>> {
    Ok(axum::Json(st.scenario.runs(q.limit.unwrap_or(50).clamp(1, 500))?))
}
async fn runs_stream(State(st): State<AppState>) -> impl IntoResponse {
    broadcast_sse(st.scenario.events.subscribe(), "run", Some(st.scenario.current()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/scenarios", get(list).post(create))
        .route("/api/scenarios/import", post(import))
        .route("/api/scenarios/run", get(run_now))
        .route("/api/scenarios/run/pause", post(run_pause))
        .route("/api/scenarios/run/resume", post(run_resume))
        .route("/api/scenarios/run/stop", post(run_stop))
        .route("/api/scenarios/runs", get(runs))
        .route("/api/scenarios/runs/stream", get(runs_stream))
        .route("/api/scenarios/{id}", get(one).put(update).delete(delete))
        .route("/api/scenarios/{id}/export", get(export))
        .route("/api/scenarios/{id}/validate", post(validate))
        .route("/api/scenarios/{id}/run", post(run_start))
}
