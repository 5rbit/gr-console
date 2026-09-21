//! 팔렛 API.
//!
//! - `GET /api/pallet/spec[?seed=1]` — 편집 저장소를 사양 문서 모양으로(`seed: false`), `seed=1` 이면 내장 사양서 R4(`seed: true`)
//! - `GET /api/pallet/profiles` · `PUT /api/pallet/profiles` · `DELETE /api/pallet/profiles/{station}`
//! - `GET /api/pallet/plan?station=&center_x=&center_y=&floor_z=&item_code=&od=&gap=&flow=&pattern=&rotation=&mirror_x=&mirror_y=&pallet_size=&levels=&type=&grip=&robot=`
//! - 패턴 편집: `GET|POST /api/pallet/flows` · `PUT|DELETE /api/pallet/flows/{id}` · `PUT|DELETE /api/pallet/flows/{id}/patterns/{pattern}`
//!   · `POST /api/pallet/flows/{id}/reset[?pattern=]` · `GET /api/pallet/flows/{id}/diff`
//! - `GET /api/pallet/export.json` · `POST /api/pallet/import?dry_run=` (본문 = 사양 문서, 흐름 id + 패턴 번호로 병합)
//!
//! plan 값의 우선순위: 쿼리 → 스테이션 프로파일 → 기본값(Flow HP_IN · Gap 50 · Rotation 0 · PalletSize 1600).
//! 중심은 스테이션을 고르면 그 Info.Position, 아니면 `center_x/center_y`(기본 0,0).

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::compose::level_stock;
use super::edit::{self, CreateFlow, FlowDiff, FlowPatch, ImportReport, PatternBody};
use super::profiles::{Profile, Profiles};
use super::store::PalletStore;
use super::{AreaCheck, DEFAULT_FLOW, DEFAULT_GAP, DEFAULT_PALLET_SIZE, GenInput, Generated, Spec, Transform, area_check, generate, r1, seed};
use crate::error::{ApiError, ApiResult};
use crate::issue::parse_task_type;
use crate::registry::spec::stack_z_with;
use crate::state::AppState;
use crate::util::now_str;

fn store(st: &AppState) -> Profiles {
    Profiles::new(st.db.clone())
}

fn patterns(st: &AppState) -> PalletStore {
    PalletStore::new(st.db.clone())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SpecQuery {
    seed: Option<String>,
}

async fn get_spec(State(st): State<AppState>, Query(q): Query<SpecQuery>) -> ApiResult<Json> {
    if flag("seed", q.seed.as_deref())?.unwrap_or(false) {
        let mut v = edit::clean_json(seed());
        v["seed"] = json!(true);
        return Ok(axum::Json(v));
    }
    let lib = patterns(&st).library()?;
    let mut v = edit::clean_json(&edit::to_doc(&lib, &now_str()));
    v["seed"] = json!(false);
    Ok(axum::Json(v))
}

// ── 패턴 편집 ───────────────────────────────────────────────────────────────

fn view_of(st: &AppState, id: &str) -> Result<Json, ApiError> {
    let snap = patterns(st).snapshot()?;
    let f = snap.lib.flow(id).ok_or_else(|| ApiError::NotFound(format!("Flow {id} 가 없습니다")))?;
    Ok(edit::flow_view(f, &snap.used_by(&f.id)))
}

async fn list_flows(State(st): State<AppState>) -> ApiResult<Vec<Json>> {
    let snap = patterns(&st).snapshot()?;
    Ok(axum::Json(snap.lib.flows.iter().map(|f| edit::flow_view(f, &snap.used_by(&f.id))).collect()))
}

async fn create_flow(State(st): State<AppState>, axum::Json(req): axum::Json<CreateFlow>) -> ApiResult<Json> {
    let (f, warnings) = patterns(&st).create(&req)?;
    Ok(axum::Json(json!({ "flow": view_of(&st, &f.id)?, "warnings": warnings })))
}

async fn update_flow(State(st): State<AppState>, Path(id): Path<String>, axum::Json(patch): axum::Json<FlowPatch>) -> ApiResult<Json> {
    let (f, warnings) = patterns(&st).update_flow(&id, &patch)?;
    Ok(axum::Json(json!({ "flow": view_of(&st, &f.id)?, "warnings": warnings })))
}

async fn delete_flow(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "removed": patterns(&st).delete_flow(&id)? })))
}

async fn put_pattern(State(st): State<AppState>, Path((id, pattern)): Path<(String, u8)>, axum::Json(body): axum::Json<PatternBody>) -> ApiResult<Json> {
    let (f, p, warnings) = patterns(&st).upsert_pattern(&id, pattern, &body)?;
    Ok(axum::Json(json!({ "flow": view_of(&st, &f.id)?, "pattern": edit::clean_json(&p), "warnings": warnings })))
}

async fn delete_pattern(State(st): State<AppState>, Path((id, pattern)): Path<(String, u8)>) -> ApiResult<Json> {
    let f = patterns(&st).delete_pattern(&id, pattern)?;
    Ok(axum::Json(json!({ "flow": view_of(&st, &f.id)? })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResetQuery {
    pattern: Option<u8>,
}

async fn reset_flow(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<ResetQuery>) -> ApiResult<Json> {
    let f = patterns(&st).reset(&id, q.pattern)?;
    Ok(axum::Json(json!({ "flow": view_of(&st, &f.id)?, "diff": edit::clean_json(&edit::diff_flow(&f)) })))
}

async fn diff_flow(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<FlowDiff> {
    let lib = patterns(&st).library()?;
    let f = lib.flow(&id).ok_or_else(|| ApiError::NotFound(format!("Flow {id} 가 없습니다")))?;
    Ok(axum::Json(edit::diff_flow(f)))
}

async fn export_json(State(st): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let doc = edit::to_doc(&patterns(&st).library()?, &now_str());
    Ok(([(header::CONTENT_DISPOSITION, "attachment; filename=\"pallet-patterns.json\"")], axum::Json(doc)))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ImportQuery {
    dry_run: Option<String>,
}

async fn import_json(State(st): State<AppState>, Query(q): Query<ImportQuery>, body: String) -> ApiResult<ImportReport> {
    let dry = flag("dry_run", q.dry_run.as_deref())?.unwrap_or(false);
    let doc: Spec = serde_json::from_str(&body).map_err(|e| ApiError::BadRequest(format!("가져오기 JSON 을 읽지 못했습니다 — {e}")))?;
    Ok(axum::Json(patterns(&st).import(&doc, dry)?))
}

// ── 프로파일 ────────────────────────────────────────────────────────────────

async fn list(State(st): State<AppState>) -> ApiResult<Vec<Profile>> {
    Ok(axum::Json(store(&st).list()?))
}

async fn put_profile(State(st): State<AppState>, axum::Json(p): axum::Json<Profile>) -> ApiResult<Profile> {
    if st.registry.station(p.station_id)?.is_none() {
        return Err(ApiError::NotFound(format!("station {} not registered", p.station_id)));
    }
    let lib = patterns(&st).library()?;
    p.validate(&lib).map_err(ApiError::BadRequest)?;
    Ok(axum::Json(store(&st).upsert(p, &lib)?))
}

async fn delete_profile(State(st): State<AppState>, Path(station): Path<u16>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "removed": store(&st).remove(station)? })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PlanQuery {
    station: Option<u16>,
    center_x: Option<f32>,
    center_y: Option<f32>,
    floor_z: Option<f32>,
    item_code: Option<u32>,
    od: Option<f32>,
    gap: Option<f32>,
    flow: Option<String>,
    /// 숫자 또는 `auto`.
    pattern: Option<String>,
    rotation: Option<u16>,
    mirror_x: Option<String>,
    mirror_y: Option<String>,
    pallet_size: Option<f32>,
    levels: Option<u32>,
    #[serde(rename = "type")]
    task_type: Option<String>,
    grip: Option<String>,
    robot: Option<u8>,
}

fn flag(name: &str, v: Option<&str>) -> Result<Option<bool>, ApiError> {
    match v.map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) if matches!(s.as_str(), "1" | "true" | "yes" | "on") => Ok(Some(true)),
        Some(s) if matches!(s.as_str(), "0" | "false" | "no" | "off") => Ok(Some(false)),
        Some(s) => Err(ApiError::BadRequest(format!("{name}={s} must be a boolean"))),
    }
}

fn nonempty(s: &Option<String>) -> Option<String> {
    s.as_ref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[derive(Serialize)]
struct LevelZ {
    level: u32,
    /// `stack_z_with` 에 넘긴 스택 개수.
    n: u32,
    z: Option<f32>,
    z_source: Option<&'static str>,
}

#[derive(Serialize)]
struct PlanView {
    #[serde(flatten)]
    plan: Generated,
    #[serde(rename = "type")]
    task_type: &'static str,
    levels: u32,
    z_levels: Vec<LevelZ>,
    floor_z: Option<f32>,
    grip_ref: String,
    /// `station` | `manual`
    center_source: &'static str,
    station: Option<Json>,
    profile: Option<Profile>,
    item: Option<Json>,
    area: Option<AreaCheck>,
    /// 각 값이 어디서 왔나(`query` | `profile` | `default`).
    sources: Json,
}

fn src<T>(q: &Option<T>, p: bool) -> &'static str {
    if q.is_some() {
        "query"
    } else if p {
        "profile"
    } else {
        "default"
    }
}

async fn plan(State(st): State<AppState>, Query(q): Query<PlanQuery>) -> ApiResult<PlanView> {
    let station = match q.station {
        Some(id) => Some(st.registry.station(id)?.ok_or_else(|| ApiError::NotFound(format!("station {id} not registered")))?),
        None => None,
    };
    let profile = match q.station {
        Some(id) => store(&st).get(id)?,
        None => None,
    };
    let entry = match q.item_code.filter(|c| *c != 0) {
        Some(c) => Some(st.registry.item(c)?.ok_or_else(|| ApiError::BadRequest(format!("item {c} not registered")))?),
        None => None,
    };
    let od =
        q.od.filter(|v| *v > 0.0)
            .or_else(|| entry.as_ref().map(|e| e.item.outer_diameter).filter(|v| *v > 0.0))
            .ok_or_else(|| ApiError::BadRequest("OuterDiameter(od) 또는 OuterDiameter 가 있는 item_code 가 필요합니다".into()))?;
    let p = profile.as_ref();
    let has_p = p.is_some();
    let flow_q = nonempty(&q.flow);
    let flow = flow_q.clone().or_else(|| p.map(|p| p.flow.clone())).unwrap_or_else(|| DEFAULT_FLOW.into());
    let gap = q.gap.or(p.map(|p| p.gap)).unwrap_or(DEFAULT_GAP);
    let mirror_x_q = flag("mirror_x", q.mirror_x.as_deref())?;
    let mirror_y_q = flag("mirror_y", q.mirror_y.as_deref())?;
    let transform = Transform {
        rotation: q.rotation.or(p.map(|p| p.rotation)).unwrap_or(0),
        mirror_x: mirror_x_q.or(p.map(|p| p.mirror_x)).unwrap_or(false),
        mirror_y: mirror_y_q.or(p.map(|p| p.mirror_y)).unwrap_or(false),
    };
    let pallet_size = q.pallet_size.or(p.map(|p| p.pallet_size)).unwrap_or(DEFAULT_PALLET_SIZE);
    let pattern = match nonempty(&q.pattern).as_deref() {
        None | Some("auto") | Some("AUTO") => None,
        Some(s) => Some(s.parse::<u8>().map_err(|_| ApiError::BadRequest(format!("pattern={s} must be a number or auto")))?),
    };
    let (center, floor_z, center_source) = match &station {
        Some(s) => ([s.para.info.position[0], s.para.info.position[1]], Some(s.para.info.position[2]), "station"),
        None => ([q.center_x.unwrap_or(0.0), q.center_y.unwrap_or(0.0)], q.floor_z, "manual"),
    };

    let lib = patterns(&st).library()?;
    let mut g = generate(&lib, &GenInput { flow: &flow, pattern, od, gap, transform, center, pallet_size }).map_err(ApiError::BadRequest)?;
    let tt = match nonempty(&q.task_type) {
        Some(s) => parse_task_type(&s)?,
        None => g.drag_kind.task_type(),
    };
    let levels = q.levels.unwrap_or(1).clamp(1, 20);
    let defaults = st.registry.defaults()?;
    let grip_ref = nonempty(&q.grip).unwrap_or_else(|| defaults.grip_ref.clone());
    let spec_ref = entry.as_ref().map(|e| &e.spec);
    let item = entry.as_ref().map(|e| &e.item).filter(|it| it.height > 0.0);
    let z_levels: Vec<LevelZ> = (1..=levels)
        .map(|level| {
            let n = level_stock(tt, level, 1);
            match (item, floor_z) {
                (Some(it), Some(f)) => {
                    let z = stack_z_with(tt, f, it, spec_ref, &grip_ref, n, 1);
                    LevelZ { level, n, z: Some(r1(z.z)), z_source: Some(z.z_source) }
                }
                _ => LevelZ { level, n, z: None, z_source: None },
            }
        })
        .collect();
    if floor_z.is_none() {
        g.warnings.push("Z: 스테이션을 고르거나 floor_z 를 주면 단별 Z 를 계산합니다".into());
    } else if item.is_none() {
        g.warnings.push("Z: Height 가 있는 품목(item_code)이 없어 단별 Z 를 계산하지 않았습니다".into());
    }
    if let Some(p) = p
        && !p.enabled
    {
        g.warnings.push(format!("스테이션 {} 프로파일이 꺼져 있습니다 — compose 는 이 스테이션에 팔렛 슬롯을 쓰지 않습니다", p.station_id));
    }

    let area = station.as_ref().and_then(|s| {
        let (gr2, note) = crate::issue::gr2_station(&st, q.robot, s.id, &s.para);
        if let Some(n) = note {
            g.warnings.push(n);
        }
        gr2.map(|gr2| area_check(&g.slots, &gr2, s.id, od))
    });
    if let Some(a) = &area
        && !a.rejects.is_empty()
    {
        let slots: std::collections::BTreeSet<&str> = a.rejects.iter().map(|r| r.slot.as_str()).collect();
        g.warnings.push(format!("GR2 isValidTaskArea: {}개 슬롯이 마진 {} 밖 → 411/412 거부 예상 (기대 중심 {:?}, TaskType {})", slots.len(), a.margin, a.expected_xy, a.task_type));
    }

    let sources = json!({
        "flow": src(&flow_q, has_p),
        "gap": src(&q.gap, has_p),
        "rotation": src(&q.rotation, has_p),
        "mirror_x": src(&mirror_x_q, has_p),
        "mirror_y": src(&mirror_y_q, has_p),
        "pallet_size": src(&q.pallet_size, has_p),
        "od": if q.od.filter(|v| *v > 0.0).is_some() { "query" } else { "item" },
        "profile_enabled": p.map(|p| p.enabled),
    });
    let station_view = station
        .as_ref()
        .map(|s| json!({ "id": s.id, "task_type": s.para.task_type, "rotate_type": s.para.rotate_type, "position": s.para.info.position, "width": s.para.info.width, "length": s.para.info.length }));
    let item_view = entry.as_ref().map(|e| json!({ "code": e.code, "name": e.name, "outer_diameter": e.item.outer_diameter, "inner_diameter": e.item.inner_diameter, "height": e.item.height }));
    Ok(axum::Json(PlanView { plan: g, task_type: tt.name(), levels, z_levels, floor_z, grip_ref, center_source, station: station_view, profile, item: item_view, area, sources }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/pallet/spec", get(get_spec))
        .route("/api/pallet/plan", get(plan))
        .route("/api/pallet/profiles", get(list).put(put_profile))
        .route("/api/pallet/profiles/{station}", delete(delete_profile))
        .route("/api/pallet/flows", get(list_flows).post(create_flow))
        .route("/api/pallet/flows/{id}", put(update_flow).delete(delete_flow))
        .route("/api/pallet/flows/{id}/patterns/{pattern}", put(put_pattern).delete(delete_pattern))
        .route("/api/pallet/flows/{id}/reset", post(reset_flow))
        .route("/api/pallet/flows/{id}/diff", get(diff_flow))
        .route("/api/pallet/export.json", get(export_json))
        .route("/api/pallet/import", post(import_json))
}
