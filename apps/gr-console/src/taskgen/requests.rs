//! 이송 요청 목록 — 상위(호스트)와 사용자가 넣는 입고 · 출고 · 이동 요청. 스케줄러는 이것을 정책 수요보다 먼저 한다.
//! 요청 하나는 수량만큼 이송 지시(PICK/DROP 짝, 출처 `req:<id>@<대상>`)로 풀린다 — 진행(`done` · `active` · `failed`)은
//! 그 지시들의 상태에서 센다(한 곳에만 진실이 있다).
//!
//! `GET /api/requests?state=open|all&limit` · `POST /api/requests`(`ext_ref` 멱등) · `POST /api/requests/batch`
//! · `PATCH /api/requests/{id}` · `POST /api/requests/{id}/cancel`

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{get as route_get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::ledger::Target;
use crate::state::AppState;
use crate::util::now_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReqKind {
    Inbound,
    Outbound,
    Move,
}

impl ReqKind {
    pub fn label(self) -> &'static str {
        match self {
            ReqKind::Inbound => "입고",
            ReqKind::Outbound => "출고",
            ReqKind::Move => "이동",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReqSource {
    Host,
    #[default]
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReqState {
    Open,
    Active,
    Done,
    Canceled,
    Failed,
}

impl ReqState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReqState::Open => "open",
            ReqState::Active => "active",
            ReqState::Done => "done",
            ReqState::Canceled => "canceled",
            ReqState::Failed => "failed",
        }
    }
    pub fn is_open(self) -> bool {
        matches!(self, ReqState::Open | ReqState::Active)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransferRequest {
    pub id: String,
    pub seq: i64,
    pub source: ReqSource,
    pub ext_ref: Option<String>,
    pub kind: ReqKind,
    pub item_code: Option<u32>,
    pub qty: u32,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub active: u32,
    #[serde(default)]
    pub failed: u32,
    pub from: Option<Target>,
    pub to: Option<Target>,
    #[serde(default)]
    pub priority: i32,
    pub state: ReqState,
    #[serde(default)]
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    /// 지금 못 도는 이유 — 엔진이 판정마다 적는다(저장 안 함).
    #[serde(default, skip_deserializing)]
    pub reason: Option<String>,
}

impl TransferRequest {
    pub fn remaining(&self) -> u32 {
        self.qty.saturating_sub(self.done + self.active)
    }
    /// 이 요청이 만든 이송 지시의 출처 접두.
    pub fn source_prefix(&self) -> String {
        format!("req:{}@", self.id)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct NewRequest {
    pub kind: Option<ReqKind>,
    pub source: Option<ReqSource>,
    pub ext_ref: Option<String>,
    pub item_code: Option<u32>,
    pub qty: u32,
    pub from: Option<Target>,
    pub to: Option<Target>,
    pub priority: i32,
    pub note: String,
}

fn is_station(t: &Target) -> bool {
    t.kind == "station"
}

/// 요청 모양 검사 — 입고는 스테이션 → 셀, 출고는 셀 → 스테이션, 이동은 셀 → 셀(둘 다 정해야).
pub fn validate(n: &NewRequest) -> Result<ReqKind, String> {
    let kind = n.kind.ok_or("kind 가 없음 (inbound · outbound · move)")?;
    if n.qty == 0 || n.qty > 1000 {
        return Err(format!("qty {} — 1..1000", n.qty));
    }
    for t in [&n.from, &n.to].into_iter().flatten() {
        if t.id == 0 || (t.kind != "cell" && t.kind != "station") {
            return Err(format!("대상 {} {} 이 잘못됨", t.kind, t.id));
        }
    }
    let side = |t: &Option<Target>, want_station: bool, what: &str| -> Result<(), String> {
        match t {
            Some(t) if is_station(t) != want_station => Err(format!("{what} 는 {} 이어야 함", if want_station { "스테이션" } else { "셀" })),
            _ => Ok(()),
        }
    };
    match kind {
        ReqKind::Inbound => {
            side(&n.from, true, "입고 출발")?;
            side(&n.to, false, "입고 도착")?;
        }
        ReqKind::Outbound => {
            side(&n.from, false, "출고 출발")?;
            side(&n.to, true, "출고 도착")?;
            if n.item_code.is_none() && n.from.is_none() {
                // 품목도 셀도 없으면 가장 오래된 재고 아무거나 — 허용하되 요청 문구에 남긴다.
            }
        }
        ReqKind::Move => {
            if n.from.is_none() || n.to.is_none() {
                return Err("이동은 출발 · 도착 셀을 모두 정해야 함".into());
            }
            side(&n.from, false, "이동 출발")?;
            side(&n.to, false, "이동 도착")?;
        }
    }
    if n.item_code == Some(0) {
        return Err("item_code 0 은 품목이 아님 — 비우거나 코드를 넣을 것".into());
    }
    Ok(kind)
}

fn format_id(date: time::Date, n: i64) -> String {
    format!("RQ-{:02}{:02}{:02}-{n:04}", date.year() % 100, date.month() as u8, date.day())
}

pub fn get(db: &Db, id: &str) -> Result<Option<TransferRequest>, ApiError> {
    let doc: Option<String> = db.with(|c| {
        c.query_row("SELECT doc_json FROM transfer_requests WHERE id = ?1", [id], |r| r.get(0)).map(Some).or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
    })?;
    Ok(doc.and_then(|d| serde_json::from_str(&d).ok()))
}

fn by_ext_ref(db: &Db, ext: &str) -> Result<Option<TransferRequest>, ApiError> {
    let doc: Option<String> = db.with(|c| {
        c.query_row("SELECT doc_json FROM transfer_requests WHERE ext_ref = ?1", [ext], |r| r.get(0)).map(Some).or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
    })?;
    Ok(doc.and_then(|d| serde_json::from_str(&d).ok()))
}

pub fn save(db: &Db, r: &TransferRequest) -> Result<(), ApiError> {
    let doc = serde_json::to_string(r)?;
    db.with(|c| {
        c.execute(
            "INSERT INTO transfer_requests (id, seq, state, ext_ref, doc_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, doc_json = excluded.doc_json, updated_at = excluded.updated_at",
            (&r.id, r.seq, r.state.as_str(), &r.ext_ref, &doc, &r.created_at, &r.updated_at),
        )
    })?;
    Ok(())
}

/// 새 요청 — 같은 `ext_ref` 가 있으면 새로 만들지 않고 그것을 돌려준다(`false` = 기존).
pub fn create(db: &Db, n: NewRequest) -> Result<(TransferRequest, bool), ApiError> {
    let kind = validate(&n).map_err(ApiError::BadRequest)?;
    let ext = n.ext_ref.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(e) = &ext
        && let Some(r) = by_ext_ref(db, e)?
    {
        return Ok((r, false));
    }
    let seq = db.next_counter("transfer_request", 1)?;
    let today = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()).date();
    let now = now_str();
    let r = TransferRequest {
        id: format_id(today, seq),
        seq,
        source: n.source.unwrap_or_default(),
        ext_ref: ext,
        kind,
        item_code: n.item_code,
        qty: n.qty,
        done: 0,
        active: 0,
        failed: 0,
        from: n.from,
        to: n.to,
        priority: n.priority,
        state: ReqState::Open,
        note: n.note.trim().to_string(),
        created_at: now.clone(),
        updated_at: now,
        ended_at: None,
        reason: None,
    };
    save(db, &r)?;
    crate::evtlog::console("CON_REQUEST", "console", seq, i64::from(r.qty), 0, format!("{} {} {} × {}", r.id, if r.source == ReqSource::Host { "상위" } else { "사용자" }, kind.label(), r.qty));
    Ok((r, true))
}

/// 목록 — `open` 이면 열린 것(open · active)만, 우선순위 높은 순 → 먼저 들어온 순.
pub fn list(db: &Db, open_only: bool, limit: u32) -> Result<Vec<TransferRequest>, ApiError> {
    let wh = if open_only { "WHERE state IN ('open','active')" } else { "" };
    let limit = limit.clamp(1, 2000);
    let docs: Vec<String> = db.with(|c| {
        let mut st = c.prepare(&format!("SELECT doc_json FROM transfer_requests {wh} ORDER BY seq DESC LIMIT {limit}"))?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    let mut v: Vec<TransferRequest> = docs.into_iter().filter_map(|d| serde_json::from_str(&d).ok()).collect();
    sort_for_service(&mut v);
    Ok(v)
}

/// 수행 순서 — 열린 것 먼저, 우선순위 높은 순, 같으면 먼저 들어온 순.
pub fn sort_for_service(v: &mut [TransferRequest]) {
    v.sort_by(|a, b| b.state.is_open().cmp(&a.state.is_open()).then(b.priority.cmp(&a.priority)).then(a.seq.cmp(&b.seq)));
}

/// 지시 상태별 수 → 진행과 상태. 바뀌었으면 `true`.
pub fn reconcile(r: &mut TransferRequest, by_state: &std::collections::BTreeMap<String, u32>) -> bool {
    let n = |k: &str| by_state.get(k).copied().unwrap_or(0);
    let done = n("done");
    let active = n("planned") + n("picking") + n("in_hand") + n("dropping");
    let failed = n("failed") + n("aborted");
    let before = (r.done, r.active, r.failed, r.state);
    r.done = done;
    r.active = active;
    r.failed = failed;
    if r.state.is_open() {
        r.state = if done >= r.qty {
            ReqState::Done
        } else if active > 0 {
            ReqState::Active
        } else {
            ReqState::Open
        };
    }
    if r.state == ReqState::Done && r.ended_at.is_none() {
        r.ended_at = Some(now_str());
    }
    let changed = before != (r.done, r.active, r.failed, r.state);
    if changed {
        r.updated_at = now_str();
    }
    changed
}

/// 열린 요청마다 이송 지시에서 진행을 다시 센다(판정마다). 끝난 것은 이벤트 로그에 남긴다.
pub fn refresh_open(db: &Db) -> Result<Vec<TransferRequest>, ApiError> {
    let mut out = Vec::new();
    for mut r in list(db, true, 2000)? {
        let counts = crate::stock::transfer::count_by_source(db, &r.source_prefix())?;
        if reconcile(&mut r, &counts) {
            save(db, &r)?;
            if r.state == ReqState::Done {
                crate::evtlog::console("CON_REQUEST", "console", r.seq, i64::from(r.done), 0, format!("{} 완료 {}/{}", r.id, r.done, r.qty));
                super::log::write(db, "request", &r.id, None, &format!("완료 {}/{}", r.done, r.qty), None);
            }
        }
        if r.state.is_open() {
            out.push(r);
        }
    }
    sort_for_service(&mut out);
    Ok(out)
}

// ── HTTP ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListQ {
    state: Option<String>,
    limit: Option<u32>,
}

async fn get_list(State(st): State<AppState>, Query(q): Query<ListQ>) -> ApiResult<Vec<TransferRequest>> {
    let open = q.state.as_deref() != Some("all");
    let mut v = list(&st.db, open, q.limit.unwrap_or(200))?;
    if let Some(e) = super::run::engine() {
        let why = e.request_reasons.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        for r in &mut v {
            r.reason = why.get(&r.id).cloned();
        }
    }
    Ok(axum::Json(v))
}

async fn post_one(State(st): State<AppState>, axum::Json(n): axum::Json<NewRequest>) -> ApiResult<Json> {
    let (r, created) = create(&st.db, n)?;
    Ok(axum::Json(json!({ "created": created, "request": r })))
}

async fn post_batch(State(st): State<AppState>, axum::Json(list): axum::Json<Vec<NewRequest>>) -> ApiResult<Json> {
    if list.len() > 500 {
        return Err(ApiError::BadRequest(format!("{} 건 — 한 번에 500 건까지", list.len())));
    }
    let out: Vec<Json> = list
        .into_iter()
        .enumerate()
        .map(|(i, n)| match create(&st.db, n) {
            Ok((r, created)) => json!({ "index": i, "ok": true, "created": created, "request": r }),
            Err(e) => json!({ "index": i, "ok": false, "error": e.to_string() }),
        })
        .collect();
    Ok(axum::Json(json!(out)))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Patch {
    priority: Option<i32>,
    qty: Option<u32>,
    note: Option<String>,
}

async fn patch(State(st): State<AppState>, Path(id): Path<String>, axum::Json(p): axum::Json<Patch>) -> ApiResult<TransferRequest> {
    let mut r = get(&st.db, &id)?.ok_or_else(|| ApiError::NotFound(format!("request {id}")))?;
    if !r.state.is_open() {
        return Err(ApiError::Conflict(format!("{} 요청은 고칠 수 없음", r.state.as_str())));
    }
    if let Some(v) = p.priority {
        r.priority = v;
    }
    if let Some(q) = p.qty {
        if q < r.done + r.active || q == 0 {
            return Err(ApiError::BadRequest(format!("qty {q} < 끝났거나 진행 중 {}", r.done + r.active)));
        }
        r.qty = q;
    }
    if let Some(n) = p.note {
        r.note = n.trim().to_string();
    }
    r.updated_at = now_str();
    save(&st.db, &r)?;
    Ok(axum::Json(r))
}

/// 취소 — 남은 수만. 이미 진행 중인 이송 지시는 그대로 끝난다(짝을 끊으면 화물이 Hand 에 남는다).
async fn cancel(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<TransferRequest> {
    let mut r = get(&st.db, &id)?.ok_or_else(|| ApiError::NotFound(format!("request {id}")))?;
    if !r.state.is_open() {
        return Err(ApiError::Conflict(format!("이미 {}", r.state.as_str())));
    }
    r.state = ReqState::Canceled;
    r.ended_at = Some(now_str());
    r.updated_at = now_str();
    save(&st.db, &r)?;
    crate::evtlog::console("CON_REQUEST", "console", r.seq, i64::from(r.done), 0, format!("{} 취소 ({}/{} 끝남, 진행 중 {})", r.id, r.done, r.qty, r.active));
    super::log::write(&st.db, "request", &r.id, None, &format!("취소 {}/{}", r.done, r.qty), None);
    Ok(axum::Json(r))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/requests", route_get(get_list).post(post_one))
        .route("/api/requests/batch", post(post_batch))
        .route("/api/requests/{id}", axum::routing::patch(patch))
        .route("/api/requests/{id}/cancel", post(cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: &str, id: u16) -> Option<Target> {
        Some(Target { kind: kind.into(), id })
    }

    #[test]
    fn shapes_are_checked() {
        let ok = |n: NewRequest| validate(&n);
        assert_eq!(ok(NewRequest { kind: Some(ReqKind::Inbound), qty: 3, ..Default::default() }), Ok(ReqKind::Inbound));
        assert!(ok(NewRequest { kind: Some(ReqKind::Inbound), qty: 0, ..Default::default() }).is_err());
        assert!(ok(NewRequest { kind: Some(ReqKind::Inbound), qty: 1, from: t("cell", 401), ..Default::default() }).is_err(), "inbound starts at a station");
        assert!(ok(NewRequest { kind: Some(ReqKind::Outbound), qty: 1, to: t("cell", 401), ..Default::default() }).is_err());
        assert!(ok(NewRequest { kind: Some(ReqKind::Move), qty: 1, from: t("cell", 401), ..Default::default() }).is_err(), "move needs both");
        assert!(ok(NewRequest { kind: Some(ReqKind::Move), qty: 1, from: t("cell", 401), to: t("cell", 402), ..Default::default() }).is_ok());
        assert!(ok(NewRequest { kind: Some(ReqKind::Outbound), qty: 1, item_code: Some(0), ..Default::default() }).is_err());
        assert!(ok(NewRequest { qty: 1, ..Default::default() }).is_err());
    }

    #[test]
    fn ext_ref_is_idempotent_and_progress_comes_from_orders() {
        let db = Db::open_memory().unwrap();
        let n = || NewRequest { kind: Some(ReqKind::Outbound), source: Some(ReqSource::Host), ext_ref: Some("WMS-7".into()), item_code: Some(2011), qty: 3, ..Default::default() };
        let (a, c1) = create(&db, n()).unwrap();
        let (b, c2) = create(&db, n()).unwrap();
        assert!(c1 && !c2);
        assert_eq!(a.id, b.id);
        assert!(a.id.starts_with("RQ-"));

        let mut r = a.clone();
        let mut m = std::collections::BTreeMap::new();
        m.insert("dropping".to_string(), 1);
        m.insert("done".to_string(), 1);
        m.insert("aborted".to_string(), 1);
        assert!(reconcile(&mut r, &m));
        assert_eq!((r.done, r.active, r.failed, r.state, r.remaining()), (1, 1, 1, ReqState::Active, 1));
        m.insert("done".to_string(), 3);
        m.remove("dropping");
        reconcile(&mut r, &m);
        assert_eq!(r.state, ReqState::Done);
        assert!(r.ended_at.is_some());
        assert!(!reconcile(&mut r, &m), "no change the second time");
    }

    #[test]
    fn service_order_is_priority_then_arrival() {
        let db = Db::open_memory().unwrap();
        let mk = |p: i32| create(&db, NewRequest { kind: Some(ReqKind::Inbound), qty: 1, priority: p, ..Default::default() }).unwrap().0;
        let a = mk(0);
        let b = mk(5);
        let c = mk(0);
        let ids: Vec<String> = list(&db, true, 10).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![b.id, a.id, c.id]);
    }

    #[test]
    fn orders_are_counted_by_source_prefix() {
        let db = Db::open_memory().unwrap();
        let (r, _) = create(&db, NewRequest { kind: Some(ReqKind::Inbound), qty: 2, ..Default::default() }).unwrap();
        let mk = |src: String| crate::stock::transfer::NewOrder { plc: "GR2_PLC".into(), item_code: 2011, count: 1, source: src, ..Default::default() };
        crate::stock::transfer::create(&db, mk(format!("{}2101", r.source_prefix())), crate::stock::transfer::OrderState::Done).unwrap();
        crate::stock::transfer::create(&db, mk(format!("{}2103", r.source_prefix())), crate::stock::transfer::OrderState::Picking).unwrap();
        crate::stock::transfer::create(&db, mk("gen:other".into()), crate::stock::transfer::OrderState::Done).unwrap();
        let open = refresh_open(&db).unwrap();
        assert_eq!((open[0].done, open[0].active, open[0].state), (1, 1, ReqState::Active));
    }
}
