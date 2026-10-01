//! `GET /api/taskgen` (설정 · 후보와 점수 근거 · 예정 큐 · 지표) · `PUT /api/taskgen/config` (버전 +1)
//! · `POST /api/taskgen/rules/{id}/request` (수동 요청 +1) · `DELETE /api/taskgen/queue/{id}` (안 보낸 예정 지우기)
//! · `GET/PUT /api/params` · `GET /api/params/history` · `POST /api/params/reset` (스케줄링 파라미터 한 곳)

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::GenConfig;
use super::run::engine;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

fn eng() -> Result<std::sync::Arc<super::run::Engine>, ApiError> {
    engine().ok_or_else(|| ApiError::Internal("task generator not started".into()))
}

async fn get_all(State(st): State<AppState>) -> ApiResult<Json> {
    let e = eng()?;
    let sel = e.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let queue = e.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let note = e.note.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let metrics = e.metrics.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let p = crate::params::current(&st.db);
    let robot_name = |id: u8| st.robots.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| format!("로봇 {id}"));
    let derived = e.derived.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let cand = |c: &super::Candidate, why: Option<&String>| cand_json(c, why, &derived, &robot_name);
    let candidates: Vec<Json> = sel.generate.iter().map(|c| cand(c, None)).chain(sel.waiting.iter().map(|(c, w)| cand(c, Some(w)))).collect();
    let skipped: Vec<Json> = sel.skipped.iter().map(|(r, w)| json!({ "rule": r, "reason": w })).collect();
    let rules = e.rules.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let inputs = e.inputs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let hand_alerts = e.hand_alerts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let reasons = e.request_reasons.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let requests: Vec<super::requests::TransferRequest> = e
        .open_requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .cloned()
        .map(|mut r| {
            r.reason = reasons.get(&r.id).cloned();
            r
        })
        .collect();
    Ok(axum::Json(json!({
        "config": e.config(), "rules": rules, "derived": derived, "inputs": inputs, "candidates": candidates, "skipped": skipped, "queue": queue,
        "note": note, "metrics": metrics, "separation_mm": p.anticol_separation_mm, "cooldowns": e.cooldown_rows(), "hand_alerts": hand_alerts, "requests": requests,
    })))
}

/// 후보 한 줄(화면) — 출처(`origin`)는 규칙 상태에서 찾는다.
fn cand_json(c: &super::Candidate, why: Option<&String>, statuses: &[super::RuleStatus], robot_name: &dyn Fn(u8) -> String) -> Json {
    let origin = statuses.iter().find(|d| d.rule_id == c.rule_id).map(|d| d.origin).unwrap_or_default();
    json!({ "rule_id": c.rule_id, "rule_name": c.rule_name, "origin": origin, "robot": c.robot, "robot_name": robot_name(c.robot), "score": c.score,
        "breakdown": c.breakdown, "area": c.area, "first": c.first, "second": c.second, "item": c.item, "age_min": c.age_min, "order": c.order, "reason": why })
}

async fn put_config(axum::Json(c): axum::Json<GenConfig>) -> ApiResult<GenConfig> {
    let e = eng()?;
    // 다른 화면이 먼저 저장했으면 덮어쓰지 않는다(0 = 버전 모름, 예전 화면).
    let cur = e.config().version;
    if c.version != 0 && c.version != cur {
        return Err(ApiError::Conflict(format!("설정이 v{cur} 로 바뀜 (보낸 것 v{}) — 다시 읽고 저장하세요", c.version)));
    }
    let mut ids = std::collections::HashSet::new();
    for id in c.rules.iter().map(|r| &r.id).chain(c.robot_rules.iter().map(|r| &r.id)) {
        if id.trim().is_empty() || !ids.insert(id.clone()) {
            return Err(ApiError::BadRequest(format!("규칙 id '{id}' 가 비었거나 겹침(작업 규칙 · 로봇 규칙 통틀어)")));
        }
    }
    let mut set_ids = std::collections::HashSet::new();
    for s in &c.rule_sets {
        if s.id.trim().is_empty() || !set_ids.insert(s.id.clone()) {
            return Err(ApiError::BadRequest(format!("규칙 세트 id '{}' 가 비었거나 겹침", s.id)));
        }
    }
    Ok(axum::Json(e.save(c)?))
}

/// `POST /api/taskgen/rule-sets/{id}/apply` — 세트에 든 규칙만 켠다(작업 규칙 · 로봇 규칙). 이미 대기열에 든 작업은 그대로.
async fn apply_set(Path(id): Path<String>) -> ApiResult<GenConfig> {
    let e = eng()?;
    let mut c = e.config();
    c.apply_set(&id, &crate::util::now_str()).map_err(ApiError::NotFound)?;
    tracing::info!(set = %id, "taskgen: 규칙 세트 적용");
    Ok(axum::Json(e.save(c)?))
}

async fn request(Path(id): Path<String>) -> ApiResult<Json> {
    let e = eng()?;
    if !e.config().rules.iter().any(|r| r.id == id) {
        return Err(ApiError::NotFound(format!("rule {id}")));
    }
    Ok(axum::Json(json!({ "rule": id, "requests": e.request(&id) })))
}

async fn remove(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let e = eng()?;
    let order = e.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|g| g.id == id).and_then(|g| g.transfer_order_id.clone());
    e.remove(&id)?;
    // 한 번도 안 보낸 짝이 이미 지시를 열었으면(제출 실패 재시도 중) 닫는다.
    if let Some(o) = order {
        st.stock.abort_order(&o, "예정 지우기 — 보내지 않음", false)?;
    }
    Ok(axum::Json(json!({ "removed": id })))
}

/// PLC 계산 간격(두 로봇 중 큰 값) — 콘솔 간격의 하한.
pub fn plc_max(st: &AppState) -> Option<f64> {
    crate::params::plc_anticol(st).iter().filter_map(|p| p.separation).fold(None, |a, x| Some(a.map_or(x, |a: f64| a.max(x))))
}

async fn params_get(State(st): State<AppState>) -> ApiResult<Json> {
    let p = crate::params::current(&st.db);
    Ok(axum::Json(json!({
        "version": crate::params::version(&st.db),
        "params": p,
        "defaults": crate::params::Params::default(),
        "spec": crate::params::spec(),
        "plc": crate::params::plc_anticol(&st),
        "plc_separation_max": plc_max(&st),
        "warnings": crate::params::warnings(&st.db, &p),
        "config": { "echo_timeout_ms": st.cfg.cmd.echo_timeout_ms },
        "robots": st.robots.iter().map(|r| json!({ "id": r.id, "name": r.name })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct PutParams {
    params: crate::params::Params,
    #[serde(default)]
    by: String,
}

async fn params_put(State(st): State<AppState>, axum::Json(b): axum::Json<PutParams>) -> ApiResult<Json> {
    let by = if b.by.trim().is_empty() { "console" } else { b.by.trim() };
    crate::params::check_plc_floor(b.params.anticol_separation_mm, plc_max(&st)).map_err(ApiError::BadRequest)?;
    let (v, p, ch) = crate::params::save(&st.db, b.params, by)?;
    Ok(axum::Json(json!({ "version": v, "params": p, "changes": ch.iter().map(|(k, a, n)| json!({ "key": k, "old": a, "new": n })).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct ResetBody {
    /// 되돌릴 키(비면 전부).
    #[serde(default)]
    keys: Vec<String>,
    #[serde(default)]
    by: String,
}

async fn params_reset(State(st): State<AppState>, axum::Json(b): axum::Json<ResetBody>) -> ApiResult<Json> {
    let cur = serde_json::to_value(crate::params::current(&st.db))?;
    let def = serde_json::to_value(crate::params::Params::default())?;
    let mut next = cur.clone();
    for sp in crate::params::spec() {
        if b.keys.is_empty() || b.keys.iter().any(|k| k == sp.key) {
            next[sp.key] = def[sp.key].clone();
        }
    }
    let p: crate::params::Params = serde_json::from_value(next)?;
    let by = if b.by.trim().is_empty() { "console (reset)" } else { b.by.trim() };
    let (v, p, ch) = crate::params::save(&st.db, p, by)?;
    Ok(axum::Json(json!({ "version": v, "params": p, "changes": ch.len() })))
}

#[derive(Deserialize)]
struct HistQ {
    limit: Option<u32>,
}

async fn params_history(State(st): State<AppState>, Query(q): Query<HistQ>) -> ApiResult<Json> {
    Ok(axum::Json(Json::Array(crate::params::history(&st.db, q.limit.unwrap_or(50).clamp(1, 500))?)))
}

#[derive(Deserialize)]
struct DefaultsBody {
    /// 출고 스테이션 — 출고 우선 규칙과 "출고에 가까운 셀" 의 기준.
    out_station: u16,
    /// 입고 스테이션(비면 출고를 뺀 등록 스테이션 전부).
    #[serde(default)]
    in_stations: Vec<u16>,
    /// 미리보기만(저장하지 않음).
    #[serde(default)]
    dry_run: bool,
    /// 지금 규칙에 **덧붙인다**(기본: 덧붙임). false 면 규칙을 이 한 벌로 바꾼다.
    #[serde(default = "yes")]
    append: bool,
}

fn yes() -> bool {
    true
}

/// 기본 규칙 한 벌을 만든다 — 현장에서 고쳐 쓰는 시작점(운전자 규칙 다섯 가지).
async fn defaults(State(st): State<AppState>, axum::Json(b): axum::Json<DefaultsBody>) -> ApiResult<Json> {
    let e = eng()?;
    let known: Vec<u16> = st.registry.stations()?.iter().map(|s| s.id).collect();
    if !known.contains(&b.out_station) {
        return Err(ApiError::BadRequest(format!("출고 스테이션 {} 이 등록돼 있지 않습니다 (등록: {:?})", b.out_station, known)));
    }
    let ins: Vec<u16> = if b.in_stations.is_empty() { known.iter().copied().filter(|id| *id != b.out_station).collect() } else { b.in_stations.clone() };
    if let Some(bad) = ins.iter().find(|id| !known.contains(id)) {
        return Err(ApiError::BadRequest(format!("입고 스테이션 {bad} 이 등록돼 있지 않습니다")));
    }
    let seed = super::default_rules(b.out_station, &ins);
    let cur = e.config();
    let mut next = cur.clone();
    next.weights.target.extend(seed.weights.target.clone());
    if b.append {
        // 같은 id 는 새 것으로 바꾼다(다시 만들어도 두 벌이 되지 않게).
        next.rules.retain(|r| !seed.rules.iter().any(|s| s.id == r.id));
        next.rules.extend(seed.rules.clone());
    } else {
        next.rules = seed.rules.clone();
    }
    if b.dry_run {
        return Ok(axum::Json(json!({ "dry_run": true, "rules": seed.rules, "weights": seed.weights, "result": next })));
    }
    let saved = e.save(next)?;
    Ok(axum::Json(json!({ "dry_run": false, "added": seed.rules.len(), "config": saved })))
}

#[derive(Deserialize)]
struct SimBody {
    config: GenConfig,
}

/// 저장 전 설정으로 한 번 판정 — 만들지도 저장하지도 않는다.
async fn simulate(State(st): State<AppState>, axum::Json(b): axum::Json<SimBody>) -> ApiResult<Json> {
    let e = eng()?;
    let st2 = st.clone();
    let plan = tokio::task::spawn_blocking(move || e.simulate(&st2, &b.config)).await.map_err(|x| ApiError::Internal(x.to_string()))?;
    let robot_name = |id: u8| st.robots.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| format!("로봇 {id}"));
    let candidates: Vec<Json> =
        plan.sel.generate.iter().map(|c| cand_json(c, None, &plan.statuses, &robot_name)).chain(plan.sel.waiting.iter().map(|(c, w)| cand_json(c, Some(w), &plan.statuses, &robot_name))).collect();
    let skipped: Vec<Json> = plan.sel.skipped.iter().map(|(r, w)| json!({ "rule": r, "reason": w })).collect();
    Ok(axum::Json(json!({ "derived": plan.statuses, "candidates": candidates, "skipped": skipped })))
}

async fn ready_get() -> ApiResult<super::ready::ReadySnapshot> {
    Ok(axum::Json(super::ready::snapshot()))
}

/// 등록 스테이션마다 저장된 프로파일 · 추정(GRM 연결, 없으면 레지스트리 연결) · 팔렛 여부.
async fn stations_get(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(json!(station_rows(&st)?)))
}

fn station_rows(st: &AppState) -> Result<Vec<Json>, ApiError> {
    let saved: std::collections::BTreeMap<u16, super::profile::StationProfile> = super::profile::list(&st.db)?.into_iter().map(|p| (p.station_id, p)).collect();
    let live: std::collections::BTreeMap<u16, (u8, u8)> =
        crate::issue::station_live::station_live_rows(st).unwrap_or_default().into_iter().filter(|l| l.live).map(|l| (l.id, (l.prev, l.next))).collect();
    let mut out = Vec::new();
    for s in st.registry.stations()? {
        let pallet = crate::pallet::compose::is_pallet_station(st, s.id).unwrap_or(false);
        let (prev, next) = live.get(&s.id).copied().unwrap_or((s.para.connection_prev, s.para.connection_next));
        let (guess, why) = super::profile::guess(s.id, prev, next, pallet);
        out.push(json!({ "id": s.id, "conv_no": s.para.conv_no, "profile": saved.get(&s.id), "guess": guess, "guess_why": why, "pallet": pallet }));
    }
    Ok(out)
}

/// 프로파일 저장 — 팔렛 방식은 팔렛 스테이션 설정과 같이 맞춘다(compose 가 그것으로 슬롯을 고른다).
fn save_profile(st: &AppState, p: super::profile::StationProfile) -> Result<super::profile::StationProfile, ApiError> {
    let saved = super::profile::save(&st.db, p)?;
    let profiles = crate::pallet::profiles::Profiles::new(st.db.clone());
    let want = saved.drop_mode == super::profile::DropMode::Pallet && saved.role.drops();
    let cur = profiles.station(saved.station_id)?;
    if cur.as_ref().map(|c| c.enabled) != Some(want) && (want || cur.is_some()) {
        profiles.upsert_station(crate::pallet::profiles::PalletStation {
            station_id: saved.station_id,
            enabled: want,
            note: cur.map(|c| c.note).unwrap_or_else(|| "스케줄러 프로파일".into()),
            updated_at: String::new(),
        })?;
    }
    Ok(saved)
}

async fn station_put(State(st): State<AppState>, Path(id): Path<u16>, axum::Json(b): axum::Json<Json>) -> ApiResult<Json> {
    if !st.registry.stations()?.iter().any(|s| s.id == id) {
        return Err(ApiError::NotFound(format!("station {id}")));
    }
    if b.get("role").is_some_and(Json::is_null) {
        super::profile::remove(&st.db, id)?;
    } else {
        let mut p: super::profile::StationProfile = serde_json::from_value(b).map_err(|e| ApiError::BadRequest(format!("프로파일: {e}")))?;
        p.station_id = id;
        if p.max_height_mm < 0.0 || !p.max_height_mm.is_finite() {
            return Err(ApiError::BadRequest("max_height_mm 은 0 이상".into()));
        }
        save_profile(&st, p)?;
    }
    let row = station_rows(&st)?.into_iter().find(|r| r["id"] == json!(id)).unwrap_or(Json::Null);
    Ok(axum::Json(row))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GuessBody {
    ids: Option<Vec<u16>>,
}

/// 추정 적용 — `ids` 가 없으면 프로파일 없는 스테이션 전부.
async fn apply_guess(State(st): State<AppState>, axum::Json(b): axum::Json<GuessBody>) -> ApiResult<Json> {
    let mut applied = Vec::new();
    for r in station_rows(&st)? {
        let id = r["id"].as_u64().unwrap_or(0) as u16;
        let wanted = match &b.ids {
            Some(v) => v.contains(&id),
            None => r["profile"].is_null(),
        };
        if !wanted {
            continue;
        }
        let g: super::profile::StationProfile = serde_json::from_value(r["guess"].clone())?;
        save_profile(&st, g)?;
        applied.push(id);
    }
    Ok(axum::Json(json!({ "applied": applied })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MigrateBody {
    dry_run: bool,
}

/// 기본 규칙 한 벌 → 스테이션 프로파일 · 정책(그 규칙은 끈다). 대상 가중은 프로파일 weight 로.
async fn migrate_rules(State(st): State<AppState>, axum::Json(b): axum::Json<MigrateBody>) -> ApiResult<Json> {
    let e = eng()?;
    let cfg = e.config();
    let known = super::profile::list(&st.db)?.into_iter().map(|p| (p.station_id, p)).collect();
    let (mut profiles, off) = super::policy::migrate(&cfg.rules, &known);
    for p in &mut profiles {
        if let Some(w) = cfg.weights.target.get(&p.station_id) {
            p.weight = *w;
        }
    }
    let mut policy = cfg.policy.clone();
    // 출고 규칙(ship-*)은 요청 없이 내보냈다 — 같은 동작은 정책의 출고 자동.
    if off.iter().any(|id| id.starts_with("ship-")) {
        policy.outbound_auto = true;
    }
    if b.dry_run {
        return Ok(axum::Json(json!({ "dry_run": true, "profiles": profiles, "disabled_rules": off, "policy": policy })));
    }
    for p in &profiles {
        save_profile(&st, p.clone())?;
    }
    let mut next = cfg.clone();
    for r in &mut next.rules {
        if off.contains(&r.id) {
            r.enabled = false;
        }
    }
    next.policy = policy.clone();
    e.save(next)?;
    Ok(axum::Json(json!({ "dry_run": false, "profiles": profiles, "disabled_rules": off, "policy": policy })))
}

async fn cooldown_clear(Path(key): Path<String>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "cleared": eng()?.clear_cooldown(&key) })))
}

async fn hand_remove(State(st): State<AppState>, Path(robot): Path<u8>) -> ApiResult<Json> {
    let (canceled, order) = super::run::hand_remove(&st, robot).await?;
    Ok(axum::Json(json!({ "canceled": canceled, "order": order })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LogQ {
    limit: Option<u32>,
    kind: Option<String>,
}

async fn log_get(State(st): State<AppState>, Query(q): Query<LogQ>) -> ApiResult<Vec<super::log::LogRow>> {
    Ok(axum::Json(super::log::list(&st.db, q.limit.unwrap_or(200), q.kind.as_deref().filter(|k| !k.is_empty()))?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct KpiQ {
    hours: Option<u32>,
}

async fn kpi_get(State(st): State<AppState>, Query(q): Query<KpiQ>) -> ApiResult<super::log::Kpi> {
    let names: Vec<(u8, String)> = st.robots.iter().map(|r| (r.id, r.name.clone())).collect();
    Ok(axum::Json(super::log::kpi(&st.db, q.hours.unwrap_or(24), &names)?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/taskgen", get(get_all))
        .route("/api/taskgen/simulate", post(simulate))
        .route("/api/taskgen/ready", get(ready_get))
        .route("/api/taskgen/stations", get(stations_get))
        .route("/api/taskgen/stations/apply-guess", post(apply_guess))
        .route("/api/taskgen/stations/{id}", put(station_put))
        .route("/api/taskgen/migrate-rules", post(migrate_rules))
        .route("/api/taskgen/cooldown/{key}", delete(cooldown_clear))
        .route("/api/taskgen/hand/{robot}/remove", post(hand_remove))
        .route("/api/taskgen/log", get(log_get))
        .route("/api/taskgen/kpi", get(kpi_get))
        .route("/api/taskgen/config", put(put_config))
        .route("/api/taskgen/rule-sets/{id}/apply", post(apply_set))
        .route("/api/taskgen/defaults", post(defaults))
        .route("/api/taskgen/rules/{id}/request", post(request))
        .route("/api/taskgen/queue/{id}", delete(remove))
        .route("/api/params", get(params_get).put(params_put))
        .route("/api/params/history", get(params_history))
        .route("/api/params/reset", post(params_reset))
}
