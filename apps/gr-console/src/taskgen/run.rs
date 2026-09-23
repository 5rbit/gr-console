//! 생성 엔진 실행부 — 설정 · 예정 큐 · 수동 요청 수는 sqlite 에 남는다(재시작 뒤 이어서, 중복 생성 없음).
//! `params.gen_tick_ms` 마다: 세상 읽기(GRM STATION 인터록 · 예상 재고 · StackMax · 로봇 X) → 후보(자동 셀 선택,
//! 팔렛 다음 슬롯 확인) → 판정(`select`) → (자동 켜짐이면) 예정 큐 → 발행기가 실행기와 같은 규칙으로 보낸다:
//! 게이트 · 큐 깊이 · 영역 · 짝(PICK 이 PLC 에 받아진 뒤 DROP) · 이송 지시 · Hand/StackMax 검사.
//! 제출 실패는 `issue_retry_backoff_ms` 뒤 다시(같은 이송 지시에 시도 기록), `issue_max_retries` 를 넘으면 중단.
//! 시나리오 실행 중에는 만들지도 보내지도 않는다(두 발행자가 한 로봇에 겹치지 않게).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::{Action, Candidate, CellView, GenConfig, Knobs, RobotView, Selection, StationPi, Trigger, World, candidate_area, choose_dest, choose_source, fires, score, select};
use crate::area::Interval;
use crate::error::ApiError;
use crate::ledger::{Origin, ScenarioSource, Target, TaskRequest, TaskState};
use crate::state::{AppState, RobotCtx};
use crate::util::now_str;

/// 예정 큐의 생성 작업 하나(짝이면 두 스텝).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenItem {
    pub id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub robot: u8,
    pub steps: Vec<GenStep>,
    /// 다음에 보낼 스텝.
    pub next: usize,
    /// 보낸 스텝의 원장 id.
    pub task_ids: Vec<String>,
    /// 짝의 이송 지시 — 처음 PICK 을 보내려 할 때 열고, 재시도에도 같은 것을 쓴다.
    pub transfer_order_id: Option<String>,
    pub area: Interval,
    pub drop_x: Option<f32>,
    pub score: f32,
    pub created_at: String,
    /// 지금 못 보내는 사유(게이트 · 깊이 · 영역 · 재시도 대기).
    pub note: Option<String>,
    /// 제출 실패 횟수(지금 스텝).
    #[serde(default)]
    pub attempts: u32,
    /// 다음 시도 가능 시각(unix ms).
    #[serde(default)]
    pub retry_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenStep {
    #[serde(rename = "type")]
    pub task_type: String,
    pub target: Target,
    pub item_code: Option<u32>,
    pub count: u8,
    /// 팔렛 스테이션 DROP — 다음 슬롯 자동.
    #[serde(default)]
    pub pallet_auto: bool,
}

/// 지표(재시작 때 0).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    pub started_at: String,
    pub generated: u64,
    pub issued: u64,
    pub completed_items: u64,
    pub aborted: u64,
    pub submit_failures: u64,
    /// 대기 사유 종류 → 판정 횟수(생성 대기 + 발행 대기).
    pub waits: BTreeMap<String, u64>,
}

pub struct Engine {
    db: crate::db::Db,
    cfg: Mutex<GenConfig>,
    /// 규칙 id → (조건이 처음 참이 된 때, 순번).
    seen: Mutex<HashMap<String, (Instant, u64)>>,
    manual: Mutex<HashMap<String, u32>>,
    pub queue: Mutex<Vec<GenItem>>,
    /// 지금 발행 중인 예정(지우기와 겹치지 않게).
    inflight: Mutex<BTreeSet<String>>,
    pub last: Mutex<Selection>,
    pub note: Mutex<Option<String>>,
    pub metrics: Mutex<Metrics>,
    counter: std::sync::atomic::AtomicU64,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

pub fn engine() -> Option<Arc<Engine>> {
    ENGINE.get().cloned()
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 대기 사유 → 지표 종류.
pub fn wait_kind(why: &str) -> &'static str {
    if why.starts_with("영역") {
        "area"
    } else if why.starts_with("제출 대기") {
        "gate"
    } else if why.starts_with("예정") {
        "queue_depth"
    } else if why.contains("PICK") {
        "pair"
    } else if why.starts_with("재시도") || why.starts_with("제출 실패") {
        "retry"
    } else if why.contains("예정·진행 중") || why.contains("같은 규칙") {
        "busy"
    } else {
        "other"
    }
}

impl Engine {
    pub fn load(db: crate::db::Db) -> Engine {
        let cfg = load_config(&db).unwrap_or_default();
        let queue: Vec<GenItem> = db
            .with(|c| {
                let mut st = c.prepare("SELECT doc_json FROM taskgen_queue ORDER BY updated_at, id")?;
                let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|d| serde_json::from_str(&d).ok())
            .collect();
        let manual: HashMap<String, u32> = db
            .with(|c| {
                let mut st = c.prepare("SELECT rule_id, count FROM taskgen_manual")?;
                let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap_or_default()
            .into_iter()
            .collect();
        let next = queue.iter().filter_map(|g| g.id.strip_prefix("gen-").and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0) + 1;
        Engine {
            db,
            cfg: Mutex::new(cfg),
            seen: Default::default(),
            manual: Mutex::new(manual),
            queue: Mutex::new(queue),
            inflight: Default::default(),
            last: Default::default(),
            note: Default::default(),
            metrics: Mutex::new(Metrics { started_at: now_str(), ..Default::default() }),
            counter: std::sync::atomic::AtomicU64::new(next),
        }
    }
    pub fn config(&self) -> GenConfig {
        self.cfg.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    /// 저장 — 버전 +1, 이력은 표에 남는다.
    pub fn save(&self, mut c: GenConfig) -> Result<GenConfig, ApiError> {
        let prev = self.config().version;
        c.version = prev + 1;
        for r in &mut c.rules {
            r.manual_requests = 0; // 요청 수는 `taskgen_manual`
        }
        let doc = serde_json::to_string(&c)?;
        self.db.with(|x| x.execute("INSERT INTO taskgen_config (version, doc_json, saved_at) VALUES (?1, ?2, ?3)", (c.version, &doc, now_str())))?;
        *self.cfg.lock().unwrap_or_else(PoisonError::into_inner) = c.clone();
        Ok(c)
    }
    fn persist_manual(&self, rule: &str, n: u32) {
        let _ = self.db.with(|c| c.execute("INSERT INTO taskgen_manual (rule_id, count) VALUES (?1, ?2) ON CONFLICT(rule_id) DO UPDATE SET count = excluded.count", (rule, n)));
    }
    pub fn request(&self, rule: &str) -> u32 {
        let mut m = self.manual.lock().unwrap_or_else(PoisonError::into_inner);
        let n = m.entry(rule.to_string()).or_default();
        *n += 1;
        let v = *n;
        self.persist_manual(rule, v);
        v
    }
    fn persist_item(&self, g: &GenItem) {
        if let Ok(doc) = serde_json::to_string(g) {
            let _ = self.db.with(|c| {
                c.execute(
                    "INSERT INTO taskgen_queue (id, doc_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET doc_json = excluded.doc_json, updated_at = excluded.updated_at",
                    (&g.id, &doc, &g.created_at),
                )
            });
        }
    }
    fn forget_item(&self, id: &str) {
        let _ = self.db.with(|c| c.execute("DELETE FROM taskgen_queue WHERE id = ?1", [id]));
    }
    /// 예정 큐에서 지우기 — 아직 한 스텝도 안 보냈고 지금 보내는 중이 아닌 것만(보낸 것은 Task 취소로).
    pub fn remove(&self, id: &str) -> Result<(), ApiError> {
        if self.inflight.lock().unwrap_or_else(PoisonError::into_inner).contains(id) {
            return Err(ApiError::Conflict("지금 보내는 중 — 잠시 뒤 다시".into()));
        }
        let mut q = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(i) = q.iter().position(|g| g.id == id) else { return Err(ApiError::NotFound(format!("gen item {id}"))) };
        if q[i].next > 0 {
            return Err(ApiError::Conflict("이미 첫 스텝을 보냄 — Task 취소로 지우세요".into()));
        }
        q.remove(i);
        drop(q);
        self.forget_item(id);
        Ok(())
    }
    fn count_wait(&self, why: &str) {
        let mut m = self.metrics.lock().unwrap_or_else(PoisonError::into_inner);
        *m.waits.entry(wait_kind(why).to_string()).or_default() += 1;
    }
}

pub fn load_config(db: &crate::db::Db) -> Option<GenConfig> {
    let doc: Option<String> = db
        .with(|c| {
            c.query_row("SELECT doc_json FROM taskgen_config ORDER BY version DESC LIMIT 1", [], |r| r.get(0))
                .map(Some)
                .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })
        .ok()
        .flatten();
    doc.and_then(|d| serde_json::from_str(&d).ok())
}

/// 디코드된 GRM `OPCUA` DB 에서 스테이션 `id` 의 인터록 입력 — `STATION[slot]`(0 부터 JSON 배열)의
/// `Interlock.PI` 는 계약상 Bool 구조체(`LGR_Interface_CV_PI`: CVOK · Req · MeasReq · ItemExist).
/// 슬롯의 `Para.Info.Id` 가 다르면(푸시 전) 모른다.
pub fn station_pi(opcua: &Json, id: u16) -> Option<StationPi> {
    let slot = crate::issue::station_offset::slot_of(id)?;
    let el = &opcua["STATION"][slot - 1];
    if el.is_null() || el["Para"]["Info"]["Id"].as_u64() != Some(id as u64) {
        return None;
    }
    let pi = &el["Interlock"]["PI"];
    let b = |k: &str| pi[k].as_bool().unwrap_or(false);
    Some(StationPi { cvok: b("CVOK"), req: b("Req"), item_exist: b("ItemExist") })
}

fn world(st: &AppState, cfg: &GenConfig) -> (World, Vec<CellView>) {
    let mut w = World::default();
    if let Some(h) = st.grm_plc()
        && let Some(d) = h.snap().db("OPCUA")
    {
        for r in &cfg.rules {
            if let Trigger::StationReq { station, .. } | Trigger::StationItem { station, .. } = r.trigger
                && let Some(pi) = station_pi(&d.json, station)
            {
                w.stations.insert(station, pi);
            }
        }
    }
    let stamps: HashMap<u16, String> = st.stock.list().unwrap_or_default().into_iter().map(|s| (s.cell_id, s.updated_at)).collect();
    let mut stock: HashMap<u16, (u32, u32)> = HashMap::new();
    if let Ok((cells, _)) = st.stock.projected_all(|| st.robots.iter().flat_map(|r| r.ledger.list()).collect()) {
        for c in cells {
            stock.insert(c.cell_id, (c.item_code, c.count));
        }
    }
    let room_of = |code: u32, n: u32| -> Option<u32> {
        if code == 0 {
            return None;
        }
        match st.registry.item(code) {
            Ok(Some(i)) if i.spec.stack_max > 0 => Some((i.spec.stack_max as u32).saturating_sub(n)),
            _ => None,
        }
    };
    let mut views = Vec::new();
    for c in st.registry.cells().unwrap_or_default() {
        let (item, count) = stock.get(&c.cell.id).copied().unwrap_or((0, 0));
        let room = room_of(item, count);
        w.stock.insert(c.cell.id, (item, count));
        if let Some(r) = room {
            w.room.insert(c.cell.id, r);
        }
        views.push(CellView {
            id: c.cell.id,
            x: c.cell.position[0],
            section: c.cell.section,
            row: c.cell.row,
            col: c.cell.col,
            use_: c.cell.use_,
            item,
            count,
            room,
            updated_at: stamps.get(&c.cell.id).cloned().unwrap_or_default(),
        });
    }
    (w, views)
}

fn gen_busy(r: &RobotCtx, rule: &str) -> bool {
    let tag = format!("gen:{rule}");
    r.ledger.list().iter().any(|e| !e.state.is_terminal() && e.state != TaskState::Draft && e.request.as_ref().and_then(|q| q.source.as_ref()).is_some_and(|s| s.scenario_id == tag))
}

/// 고른 대상으로 스텝을 만든다.
fn steps_for(a: &Action, first: &Target, second: Option<&Target>) -> Vec<GenStep> {
    match a {
        Action::Transfer { item, count, pallet_auto, .. } => vec![
            GenStep { task_type: "PICK".into(), target: first.clone(), item_code: *item, count: *count, pallet_auto: false },
            GenStep { task_type: "DROP".into(), target: second.cloned().unwrap_or_else(|| first.clone()), item_code: *item, count: *count, pallet_auto: *pallet_auto },
        ],
        Action::Move { .. } => vec![GenStep { task_type: "MOVE".into(), target: first.clone(), item_code: None, count: 1, pallet_auto: false }],
        Action::Measure { item, .. } => vec![GenStep { task_type: "MEASURE".into(), target: first.clone(), item_code: *item, count: 1, pallet_auto: false }],
    }
}

fn cell_target(id: u16) -> Target {
    Target { kind: "cell".into(), id }
}

/// 규칙의 대상(자동 셀 선택 포함) — 못 고르면 사유.
fn resolve(st: &AppState, a: &Action, cells: &[CellView], robot_x: Option<f32>, free: &dyn Fn(f32) -> bool) -> Result<(Target, Option<Target>), String> {
    match a {
        Action::Transfer { from, to, item, count, from_auto, to_auto, pallet_auto } => {
            let src = match from_auto {
                Some(p) => cell_target(choose_source(cells, p, *item, *count as u32, robot_x, free).ok_or("자동 출발 셀 없음 (재고·구역·영역)")?),
                None => from.clone(),
            };
            let src_item = item.or_else(|| cells.iter().find(|c| c.id == src.id).map(|c| c.item).filter(|i| *i != 0)).unwrap_or(0);
            let dst = match to_auto {
                Some(p) => cell_target(choose_dest(cells, p, src_item, *count as u32, robot_x, Some(src.id).filter(|_| src.kind == "cell"), free).ok_or("자동 도착 셀 없음 (칸·구역·영역)")?),
                None => to.clone(),
            };
            if *pallet_auto {
                // 팔렛 다음 슬롯: DROP 을 미리 작성해 본다(프로파일 없음 · 자리 없음이면 후보 아님)
                let req = TaskRequest {
                    task_type: "DROP".into(),
                    target: Some(dst.clone()),
                    item_code: Some(src_item).filter(|i| *i != 0),
                    count: (*count).max(1),
                    pallet: Some(crate::pallet::compose::PalletRef { auto: true, ..Default::default() }),
                    ..Default::default()
                };
                if let Err(e) = crate::issue::compose(st, &req) {
                    return Err(format!("팔렛 다음 슬롯 없음: {e}"));
                }
            }
            Ok((src, Some(dst)))
        }
        Action::Move { to } => Ok((to.clone(), None)),
        Action::Measure { target, .. } => Ok((target.clone(), None)),
    }
}

/// 한 번 판정(자동이면 생성까지). 결과는 `last` 에.
pub fn tick_generate(st: &AppState, e: &Engine) {
    let mut cfg = e.config();
    {
        let m = e.manual.lock().unwrap_or_else(PoisonError::into_inner);
        for r in &mut cfg.rules {
            r.manual_requests = m.get(&r.id).copied().unwrap_or(0);
        }
    }
    let p = crate::params::current(&st.db);
    let sep = if p.anticol_enabled && st.robots.len() > 1 { p.anticol_separation_mm } else { 0.0 };
    let knobs = Knobs { age_per_min: p.gen_age_per_min, distance_per_m: p.gen_distance_per_m };
    let (w, cells) = world(st, &cfg);
    let queue = e.queue.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let robots: Vec<RobotView> = st
        .robots
        .iter()
        .map(|r| {
            let mine: Vec<&GenItem> = queue.iter().filter(|g| g.robot == r.id).collect();
            RobotView {
                id: r.id,
                name: r.name.clone(),
                current_x: crate::area::robot_x(st, r),
                pending: crate::area::pending_targets(r),
                committed: crate::area::committed(r),
                scheduled: mine.iter().filter(|g| g.next == 0).map(|g| g.area).reduce(|a, b| a.with(b.lo).with(b.hi)),
                busy: !mine.is_empty(),
                margin: p.margin(r.id),
            }
        })
        .collect();
    let mut seen = e.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let mut cands = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for rule in &cfg.rules {
        let on = fires(rule, &w);
        if !on {
            seen.remove(&rule.id);
            continue;
        }
        if queue.iter().any(|g| g.rule_id == rule.id) || st.robots.iter().any(|r| gen_busy(r, &rule.id)) {
            continue; // 이 규칙의 작업이 아직 돈다
        }
        let (since, order) = *seen.entry(rule.id.clone()).or_insert_with(|| (Instant::now(), e.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let age = since.elapsed().as_secs_f32() / 60.0;
        for rv in robots.iter().filter(|rv| rule.robots.is_empty() || rule.robots.contains(&rv.id)) {
            // 자동 셀은 다른 로봇 영역 밖에서 고른다(동시 운전)
            let others: Vec<crate::area::Reservation> = robots.iter().filter(|o| o.id != rv.id).filter_map(|o| super::reserved(o, None)).collect();
            let free = |x: f32| crate::area::blocker(Interval::point(x), &others, sep + rv.margin).is_none();
            let (first, second) = match resolve(st, &rule.action, &cells, rv.current_x, &free) {
                Ok(t) => t,
                Err(why) => {
                    skipped.push((rule.name.clone(), format!("{}: {why}", rv.name)));
                    continue;
                }
            };
            let Some(x0) = crate::area::target_x(st, &first) else {
                skipped.push((rule.name.clone(), format!("대상 {} {} 미등록", first.kind, first.id)));
                continue;
            };
            let drop_x = second.as_ref().and_then(|t| crate::area::target_x(st, t));
            let dist = rv.current_x.map(|x| (x - x0).abs() / 1000.0).unwrap_or(0.0);
            if p.gen_max_distance_m > 0.0 && dist > p.gen_max_distance_m {
                skipped.push((rule.name.clone(), format!("{}: 거리 {dist:.1} m > {:.1} m", rv.name, p.gen_max_distance_m)));
                continue;
            }
            let targets: Vec<&Target> = std::iter::once(&first).chain(second.as_ref()).collect();
            let (s, b) = score(rule, &targets, rv.id, &cfg.weights, knobs, age, dist);
            cands.push(Candidate {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                robot: rv.id,
                first: first.clone(),
                second: second.clone(),
                target_x: x0,
                drop_x,
                area: candidate_area(rv, x0, drop_x),
                score: s,
                breakdown: b,
                order,
                age_min: age,
            });
        }
    }
    drop(seen);
    let mut sel = select(cands, &robots, sep);
    sel.skipped = skipped;
    for (c, why) in &mut sel.waiting {
        e.count_wait(why);
        if p.gen_blocked_penalty != 0.0 && why.starts_with("영역") {
            c.score -= p.gen_blocked_penalty;
            c.breakdown.push(("AntiCollision".into(), -p.gen_blocked_penalty));
        }
    }
    let running = st.scenario.current().state.is_active();
    *e.note.lock().unwrap_or_else(PoisonError::into_inner) = if running {
        Some("시나리오 실행 중 — 생성·발행 멈춤".into())
    } else if !cfg.auto {
        Some("자동 생성 꺼짐 — 후보만 보여 줌".into())
    } else {
        None
    };
    if cfg.auto && !running {
        let mut q = e.queue.lock().unwrap_or_else(PoisonError::into_inner);
        for c in &sel.generate {
            let Some(rule) = cfg.rules.iter().find(|r| r.id == c.rule_id) else { continue };
            if matches!(rule.trigger, Trigger::Manual) {
                let mut m = e.manual.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(n) = m.get_mut(&rule.id) {
                    *n = n.saturating_sub(1);
                    e.persist_manual(&rule.id, *n);
                }
            }
            let g = GenItem {
                id: format!("gen-{}", e.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
                rule_id: c.rule_id.clone(),
                rule_name: c.rule_name.clone(),
                robot: c.robot,
                steps: steps_for(&rule.action, &c.first, c.second.as_ref()),
                next: 0,
                task_ids: vec![],
                transfer_order_id: None,
                area: c.area,
                drop_x: c.drop_x,
                score: c.score,
                created_at: now_str(),
                note: None,
                attempts: 0,
                retry_at_ms: 0,
            };
            e.persist_item(&g);
            q.push(g);
            e.metrics.lock().unwrap_or_else(PoisonError::into_inner).generated += 1;
        }
    }
    *e.last.lock().unwrap_or_else(PoisonError::into_inner) = sel;
}

/// 예정 큐를 보낸다 — 로봇마다 맨 앞 항목의 다음 스텝 하나.
pub async fn tick_issue(st: &AppState, e: &Engine) {
    if st.scenario.current().state.is_active() || !e.config().auto {
        return;
    }
    let p = crate::params::current(&st.db);
    let items = e.queue.lock().unwrap_or_else(PoisonError::into_inner).clone();
    // 짝이 걸린 로봇(PICK 보냄, DROP 아직)의 DROP 목표 — 다른 로봇 영역 검사에.
    let pairs: BTreeMap<u8, f32> = items.iter().filter(|g| g.next == 1 && g.steps.len() == 2).filter_map(|g| g.drop_x.map(|x| (g.robot, x))).collect();
    let mut done_robots: Vec<u8> = Vec::new();
    for g in items {
        if done_robots.contains(&g.robot) {
            continue;
        }
        done_robots.push(g.robot);
        if g.retry_at_ms > unix_ms() {
            continue;
        }
        e.inflight.lock().unwrap_or_else(PoisonError::into_inner).insert(g.id.clone());
        let outcome = issue_one(st, &p, &pairs, &g).await;
        e.inflight.lock().unwrap_or_else(PoisonError::into_inner).remove(&g.id);
        let mut q = e.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(item) = q.iter_mut().find(|x| x.id == g.id) else { continue };
        match outcome {
            Issue::Sent { task_id, order } => {
                item.task_ids.push(task_id);
                if order.is_some() {
                    item.transfer_order_id = order;
                }
                item.next += 1;
                item.note = None;
                item.attempts = 0;
                item.retry_at_ms = 0;
                e.metrics.lock().unwrap_or_else(PoisonError::into_inner).issued += 1;
                if item.next >= item.steps.len() {
                    let id = item.id.clone();
                    q.retain(|x| x.id != id);
                    e.forget_item(&id);
                    e.metrics.lock().unwrap_or_else(PoisonError::into_inner).completed_items += 1;
                } else {
                    e.persist_item(item);
                }
            }
            Issue::Wait(why) => {
                e.count_wait(&why);
                item.note = Some(why);
            }
            Issue::Failed { why, order } => {
                // 같은 이송 지시로 다시 — 시도 기록은 지시 이력에.
                if order.is_some() {
                    item.transfer_order_id = order;
                }
                item.attempts += 1;
                e.metrics.lock().unwrap_or_else(PoisonError::into_inner).submit_failures += 1;
                if let Some(o) = &item.transfer_order_id {
                    let _ = st.stock.note_order(o, &format!("제출 실패 {}회: {why}", item.attempts));
                }
                if item.attempts > p.issue_max_retries {
                    tracing::warn!(item = %g.id, %why, "taskgen: gave up after retries");
                    if let Some(o) = &item.transfer_order_id
                        && item.next == 0
                    {
                        let _ = st.stock.abort_order(o, &format!("제출 {}회 실패 — 생성 작업 중단", item.attempts), false);
                    }
                    let id = item.id.clone();
                    q.retain(|x| x.id != id);
                    e.forget_item(&id);
                    e.metrics.lock().unwrap_or_else(PoisonError::into_inner).aborted += 1;
                } else {
                    item.retry_at_ms = unix_ms() + p.issue_retry_backoff_ms;
                    item.note = Some(format!("재시도 {}/{} ({} ms 뒤): {why}", item.attempts, p.issue_max_retries, p.issue_retry_backoff_ms));
                    e.count_wait("재시도");
                    e.persist_item(item);
                }
            }
            Issue::Abort(why) => {
                tracing::warn!(item = %g.id, %why, "taskgen: item aborted");
                if let Some(o) = &item.transfer_order_id {
                    let _ = st.stock.abort_order(o, &format!("생성 작업 중단: {why}"), false);
                }
                let id = item.id.clone();
                q.retain(|x| x.id != id);
                e.forget_item(&id);
                e.metrics.lock().unwrap_or_else(PoisonError::into_inner).aborted += 1;
            }
        }
    }
}

enum Issue {
    Sent { task_id: String, order: Option<String> },
    Wait(String),
    Failed { why: String, order: Option<String> },
    Abort(String),
}

async fn issue_one(st: &AppState, p: &crate::params::Params, pairs: &BTreeMap<u8, f32>, g: &GenItem) -> Issue {
    let Ok(r) = st.robot(Some(g.robot)) else { return Issue::Abort(format!("로봇 {} 없음", g.robot)) };
    let step = &g.steps[g.next];
    // 짝 DROP 은 PICK 이 PLC 에 받아진 뒤(깊이 1: PICK 실행 중 → DROP 대기). PICK 이 나쁘게 끝났으면 DROP 은 보내지 않는다.
    if g.next == 1 {
        match g.task_ids.first().and_then(|id| st.find_task(id)).map(|(_, e)| e) {
            Some(pick) if matches!(pick.state, TaskState::Rejected | TaskState::Failed | TaskState::Canceled | TaskState::Lost) => {
                return Issue::Abort(format!("짝 PICK 이 {}", pick.state.as_str()));
            }
            Some(pick) if matches!(pick.state, TaskState::Draft | TaskState::Submitted) => return Issue::Wait("짝 PICK 의 PLC 수령 대기".into()),
            None => return Issue::Abort("짝 PICK 을 원장에서 찾지 못함".into()),
            _ => {}
        }
    }
    let gate = crate::ledger::ops::gate(st, r);
    if !gate.can_submit {
        return Issue::Wait(format!("제출 대기: {}", gate.reasons.join("; ")));
    }
    if let Some(why) = crate::scenario::runner::queue_depth_reason(r.ledger.list().iter().map(|e| e.state), p.issue_queue_depth) {
        return Issue::Wait(why);
    }
    let Some(x) = crate::area::target_x(st, &step.target) else { return Issue::Abort(format!("대상 {} {} 미등록", step.target.kind, step.target.id)) };
    let pair_drop = (g.next == 0 && g.steps.len() == 2).then_some(g.drop_x).flatten();
    let mut others = pairs.clone();
    others.remove(&g.robot);
    let area_cfg = crate::area::AreaConfig { separation_mm: p.anticol_separation_mm, enabled: p.anticol_enabled };
    if let Some((b, mine)) = crate::area::check(st, &area_cfg, r, x, pair_drop, &others) {
        return Issue::Wait(crate::area::wait_reason(&b, mine, area_cfg.separation_mm));
    }
    // 이송 지시 — 짝의 PICK 에서 한 번만 연다(재시도도 같은 지시).
    let mut order = g.transfer_order_id.clone();
    if order.is_none() && g.steps.len() == 2 {
        let n = crate::stock::transfer::NewOrder {
            robot: Some(r.id),
            plc: r.plc.clone(),
            item_code: step.item_code.unwrap_or(0),
            count: step.count as u32,
            from: Some(g.steps[0].target.clone()),
            to: Some(g.steps[1].target.clone()),
            source: format!("gen:{}", g.rule_id),
            note: g.rule_name.clone(),
        };
        match st.stock.open_order(n) {
            Ok(o) => order = Some(o.id),
            Err(e) => return Issue::Wait(format!("이송 지시: {e}")),
        }
    }
    let req = TaskRequest {
        task_type: step.task_type.clone(),
        target: Some(step.target.clone()),
        item_code: step.item_code,
        count: step.count.max(1),
        params: serde_json::json!({}),
        note: format!("생성: {}", g.rule_name),
        source: Some(ScenarioSource { scenario_id: format!("gen:{}", g.rule_id), run_id: g.id.clone(), iteration: 0, step_index: g.next as u32 }),
        robot: Some(r.id),
        transfer_order_id: order.clone(),
        pallet: step.pallet_auto.then(|| crate::pallet::compose::PalletRef { auto: true, ..Default::default() }),
        ..Default::default()
    };
    let composed = match crate::issue::compose(st, &req) {
        Ok(c) => c,
        Err(e) => return Issue::Failed { why: format!("compose: {e}"), order },
    };
    match crate::ledger::ops::create_and_submit(st, r, Origin::Scenario, Some(req), Some(composed.params), composed.task, composed.pallet, true).await {
        Ok(entry) => Issue::Sent { task_id: entry.id, order },
        Err(e) => Issue::Failed { why: format!("제출 실패: {e}"), order },
    }
}

pub fn spawn(st: AppState) {
    let e = Arc::new(Engine::load(st.db.clone()));
    let _ = ENGINE.set(e.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(crate::params::current(&st.db).gen_tick_ms.max(200))).await;
            tick_generate(&st, &e);
            tick_issue(&st, &e).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 계약(`plc/contract/GRM_PLC`)대로 인코딩한 `OPCUA.STATION[1].Interlock.PI` 를 읽는다 — PI 는 Bool 구조체다.
    #[test]
    fn station_pi_follows_the_grm_contract() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract/GRM_PLC");
        let grm = plc_layout::Contract::load_dir(&root).expect("GRM contract");
        let mut buf = vec![0u8; grm.size_of_db("OPCUA").expect("OPCUA size") as usize];
        grm.encode_path("OPCUA", "STATION[1].Para.Info.Id", &serde_json::json!(2101), &mut buf).unwrap();
        grm.encode_path("OPCUA", "STATION[1].Interlock.PI.Req", &serde_json::json!(true), &mut buf).unwrap();
        grm.encode_path("OPCUA", "STATION[1].Interlock.PI.CVOK", &serde_json::json!(true), &mut buf).unwrap();
        let json = grm.decode_db("OPCUA", &buf).unwrap();
        assert!(json["STATION"][0]["Interlock"]["PI"].is_object(), "PI is a struct of Bools");
        assert_eq!(station_pi(&json, 2101), Some(StationPi { cvok: true, req: true, item_exist: false }));
        assert_eq!(station_pi(&json, 2102), None, "slot 2 holds no station yet");
        assert_eq!(station_pi(&json, 2201), None, "slot 1 is 2101, not 2201");
    }

    #[test]
    fn wait_reasons_are_counted_by_kind() {
        assert_eq!(wait_kind("영역 겹침: GR2 X 1..2"), "area");
        assert_eq!(wait_kind("영역 대기: GR2"), "area");
        assert_eq!(wait_kind("제출 대기: GR1: AUTO 모드가 아님"), "gate");
        assert_eq!(wait_kind("예정 — 실행 대기 중인 Task 1 건"), "queue_depth");
        assert_eq!(wait_kind("짝 PICK 의 PLC 수령 대기"), "pair");
        assert_eq!(wait_kind("재시도 1/3"), "retry");
        assert_eq!(wait_kind("이 로봇에 예정·진행 중 생성 작업 있음"), "busy");
    }

    /// 예정 큐 · 수동 요청은 DB 에 남아 새 엔진이 이어받는다(중복 생성 없음), 지운 것은 사라진다.
    #[test]
    fn queue_and_manual_requests_survive_a_restart() {
        let db = crate::db::Db::open_memory().unwrap();
        let e = Engine::load(db.clone());
        assert_eq!(e.request("r1"), 1);
        assert_eq!(e.request("r1"), 2);
        let g = GenItem {
            id: "gen-7".into(),
            rule_id: "r1".into(),
            rule_name: "r1".into(),
            robot: 1,
            steps: vec![],
            next: 1,
            task_ids: vec!["t1".into()],
            transfer_order_id: Some("TO-1".into()),
            area: Interval::point(1.0),
            drop_x: None,
            score: 0.0,
            created_at: "a".into(),
            note: None,
            attempts: 1,
            retry_at_ms: 0,
        };
        e.persist_item(&g);
        let g2 = GenItem { id: "gen-8".into(), next: 0, ..g.clone() };
        e.persist_item(&g2);
        e.queue.lock().unwrap().extend([g.clone(), g2.clone()]);
        e.remove("gen-8").unwrap();
        assert!(e.remove("gen-7").is_err(), "already sent a step");
        let back = Engine::load(db);
        let q = back.queue.lock().unwrap().clone();
        assert_eq!(q.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["gen-7"]);
        assert_eq!((q[0].next, q[0].transfer_order_id.as_deref(), q[0].attempts), (1, Some("TO-1"), 1));
        assert_eq!(back.manual.lock().unwrap().get("r1"), Some(&2));
        assert!(back.counter.load(std::sync::atomic::Ordering::Relaxed) > 7, "ids keep counting after the restart");
    }
}
