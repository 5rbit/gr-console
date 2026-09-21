//! 생성 엔진 실행부 — 설정 저장(sqlite, 버전), 세상 읽기(GRM STATION 인터록 · 예상 재고 · StackMax · 로봇 X), 1 s 마다
//! 판정(`select`) → (자동 켜짐이면) "예정" 큐에 넣고 → 발행기가 실행기와 같은 규칙으로 보낸다:
//! 게이트 · 큐 깊이 1(`queue_depth_reason`) · 영역(`area::check`) · 짝(PICK 다음 DROP, 이송 지시) · Hand/StackMax 검사.
//! 시나리오 실행 중에는 만들지도 보내지도 않는다(두 발행자가 한 로봇에 겹치지 않게).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Action, Candidate, GenConfig, RobotView, Selection, World, candidate_area, fires, score, select};
use crate::area::Interval;
use crate::error::ApiError;
use crate::ledger::{Origin, ScenarioSource, Target, TaskRequest, TaskState};
use crate::state::{AppState, RobotCtx};
use crate::util::now_str;

pub const TICK_MS: u64 = 1000;

/// 예정 큐의 생성 작업 하나(짝이면 두 스텝).
#[derive(Clone, Debug, Serialize)]
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
    pub transfer_order_id: Option<String>,
    pub area: Interval,
    pub drop_x: Option<f32>,
    pub score: f32,
    pub created_at: String,
    /// 지금 못 보내는 사유(게이트 · 깊이 · 영역).
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenStep {
    #[serde(rename = "type")]
    pub task_type: String,
    pub target: Target,
    pub item_code: Option<u32>,
    pub count: u8,
}

pub struct Engine {
    db: crate::db::Db,
    cfg: Mutex<GenConfig>,
    /// 규칙 id → (조건이 처음 참이 된 때, 순번).
    seen: Mutex<HashMap<String, (Instant, u64)>>,
    manual: Mutex<HashMap<String, u32>>,
    pub queue: Mutex<Vec<GenItem>>,
    pub last: Mutex<Selection>,
    pub note: Mutex<Option<String>>,
    counter: std::sync::atomic::AtomicU64,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

pub fn engine() -> Option<Arc<Engine>> {
    ENGINE.get().cloned()
}

impl Engine {
    pub fn load(db: crate::db::Db) -> Engine {
        let cfg = load_config(&db).unwrap_or_default();
        Engine {
            db,
            cfg: Mutex::new(cfg),
            seen: Default::default(),
            manual: Default::default(),
            queue: Default::default(),
            last: Default::default(),
            note: Default::default(),
            counter: std::sync::atomic::AtomicU64::new(1),
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
            r.manual_requests = 0; // 요청 수는 저장하지 않는다(메모리)
        }
        let doc = serde_json::to_string(&c)?;
        self.db.with(|x| x.execute("INSERT INTO taskgen_config (version, doc_json, saved_at) VALUES (?1, ?2, ?3)", (c.version, &doc, now_str())))?;
        *self.cfg.lock().unwrap_or_else(PoisonError::into_inner) = c.clone();
        Ok(c)
    }
    pub fn request(&self, rule: &str) -> u32 {
        let mut m = self.manual.lock().unwrap_or_else(PoisonError::into_inner);
        let n = m.entry(rule.to_string()).or_default();
        *n += 1;
        *n
    }
    /// 예정 큐에서 지우기 — 아직 한 스텝도 안 보낸 것만(보낸 것은 Task 취소로).
    pub fn remove(&self, id: &str) -> Result<(), ApiError> {
        let mut q = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(i) = q.iter().position(|g| g.id == id) else { return Err(ApiError::NotFound(format!("gen item {id}"))) };
        if q[i].next > 0 {
            return Err(ApiError::Conflict("이미 첫 스텝을 보냄 — Task 취소로 지우세요".into()));
        }
        q.remove(i);
        Ok(())
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

/// GRM `OPCUA.STATION[slot].Interlock.PI` 의 (Req, ItemExist) — 비트 묶음(숫자)이나 구조체 둘 다 읽는다.
fn station_bits(st: &AppState, id: u16) -> Option<(bool, bool)> {
    let slot = crate::issue::station_offset::slot_of(id)?;
    let h = st.grm_plc()?;
    let snap = h.snap();
    let d = snap.db("OPCUA")?;
    let el = &d.json["STATION"][slot - 1];
    if el["Para"]["Info"]["Id"].as_u64().is_some_and(|x| x != id as u64) {
        return None;
    }
    let pi = &el["Interlock"]["PI"];
    if let Some(n) = pi.as_u64() {
        return Some((n & 0b10 != 0, n & 0b1000 != 0));
    }
    let b = |k: &str| pi[k].as_bool().unwrap_or(false);
    Some((b("Req"), b("ItemExist")))
}

fn world(st: &AppState, cfg: &GenConfig) -> World {
    let mut w = World::default();
    for r in &cfg.rules {
        if let super::Trigger::StationReq { station } | super::Trigger::StationItem { station } = r.trigger
            && let Some(b) = station_bits(st, station)
        {
            w.stations.insert(station, b);
        }
    }
    if let Ok((cells, _)) = st.stock.projected_all(|| st.robots.iter().flat_map(|r| r.ledger.list()).collect()) {
        for c in cells {
            w.stock.insert(c.cell_id, (c.item_code, c.count));
            if c.item_code != 0
                && let Ok(Some(item)) = st.registry.item(c.item_code)
                && item.spec.stack_max > 0
            {
                w.room.insert(c.cell_id, (item.spec.stack_max as u32).saturating_sub(c.count));
            }
        }
    }
    w
}

fn gen_busy(r: &RobotCtx, rule: &str) -> bool {
    let tag = format!("gen:{rule}");
    r.ledger.list().iter().any(|e| !e.state.is_terminal() && e.state != TaskState::Draft && e.request.as_ref().and_then(|q| q.source.as_ref()).is_some_and(|s| s.scenario_id == tag))
}

fn steps_of(a: &Action) -> Vec<GenStep> {
    match a {
        Action::Transfer { from, to, item, count } => {
            vec![GenStep { task_type: "PICK".into(), target: from.clone(), item_code: *item, count: *count }, GenStep { task_type: "DROP".into(), target: to.clone(), item_code: *item, count: *count }]
        }
        Action::Move { to } => vec![GenStep { task_type: "MOVE".into(), target: to.clone(), item_code: None, count: 1 }],
        Action::Measure { target, item } => vec![GenStep { task_type: "MEASURE".into(), target: target.clone(), item_code: *item, count: 1 }],
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
    let area_cfg = crate::area::load(&st.db);
    let sep = if area_cfg.enabled && st.robots.len() > 1 { area_cfg.separation_mm } else { 0.0 };
    let w = world(st, &cfg);
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
            }
        })
        .collect();
    // 규칙이 참이 된 순서(같은 점수의 결정적 순서) · 대기 시간
    let mut seen = e.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let mut cands = Vec::new();
    for rule in &cfg.rules {
        let busy = queue.iter().any(|g| g.rule_id == rule.id) || st.robots.iter().any(|r| gen_busy(r, &rule.id));
        if !fires(rule, &w) || busy {
            if !fires(rule, &w) {
                seen.remove(&rule.id);
            }
            continue;
        }
        let (since, order) = *seen.entry(rule.id.clone()).or_insert_with(|| (Instant::now(), e.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let age = since.elapsed().as_secs_f32() / 60.0;
        let steps = steps_of(&rule.action);
        let Some(x0) = crate::area::target_x(st, &steps[0].target) else { continue };
        let drop_x = steps.get(1).and_then(|s| crate::area::target_x(st, &s.target));
        for rv in robots.iter().filter(|rv| rule.robots.is_empty() || rule.robots.contains(&rv.id)) {
            let dist = rv.current_x.map(|x| (x - x0).abs() / 1000.0).unwrap_or(0.0);
            let (s, b) = score(rule, rv.id, &cfg.weights, age, dist);
            cands.push(Candidate {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                robot: rv.id,
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
    let pen = cfg.weights.blocked_penalty;
    for (c, why) in &mut sel.waiting {
        if pen != 0.0 && why.starts_with("영역") {
            c.score -= pen;
            c.breakdown.push(("AntiCollision".into(), -pen));
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
            if matches!(rule.trigger, super::Trigger::Manual) {
                let mut m = e.manual.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(n) = m.get_mut(&rule.id) {
                    *n = n.saturating_sub(1);
                }
            }
            q.push(GenItem {
                id: format!("gen-{}", e.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
                rule_id: c.rule_id.clone(),
                rule_name: c.rule_name.clone(),
                robot: c.robot,
                steps: steps_of(&rule.action),
                next: 0,
                task_ids: vec![],
                transfer_order_id: None,
                area: c.area,
                drop_x: c.drop_x,
                score: c.score,
                created_at: now_str(),
                note: None,
            });
        }
    }
    *e.last.lock().unwrap_or_else(PoisonError::into_inner) = sel;
}

/// 예정 큐를 보낸다 — 로봇마다 맨 앞 항목의 다음 스텝 하나.
pub async fn tick_issue(st: &AppState, e: &Engine) {
    if st.scenario.current().state.is_active() || !e.config().auto {
        return;
    }
    let area_cfg = crate::area::load(&st.db);
    let items = e.queue.lock().unwrap_or_else(PoisonError::into_inner).clone();
    // 짝이 걸린 로봇(PICK 보냄, DROP 아직)의 DROP 목표 — 다른 로봇 영역 검사에.
    let pairs: std::collections::BTreeMap<u8, f32> = items.iter().filter(|g| g.next == 1 && g.steps.len() == 2).filter_map(|g| g.drop_x.map(|x| (g.robot, x))).collect();
    let mut done_robots: Vec<u8> = Vec::new();
    for g in items {
        if done_robots.contains(&g.robot) {
            continue;
        }
        done_robots.push(g.robot);
        let outcome = issue_one(st, &area_cfg, &pairs, &g).await;
        let mut q = e.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(item) = q.iter_mut().find(|x| x.id == g.id) else { continue };
        match outcome {
            Issue::Sent { task_id, order } => {
                item.task_ids.push(task_id);
                item.transfer_order_id = order.or(item.transfer_order_id.take());
                item.next += 1;
                item.note = None;
                if item.next >= item.steps.len() {
                    let id = item.id.clone();
                    q.retain(|x| x.id != id);
                }
            }
            Issue::Wait(why) => item.note = Some(why),
            Issue::Abort(why) => {
                tracing::warn!(item = %g.id, %why, "taskgen: item aborted");
                if let Some(o) = &item.transfer_order_id {
                    let _ = st.stock.abort_order(o, &format!("생성 작업 중단: {why}"), false);
                }
                let id = item.id.clone();
                q.retain(|x| x.id != id);
            }
        }
    }
}

enum Issue {
    Sent { task_id: String, order: Option<String> },
    Wait(String),
    Abort(String),
}

async fn issue_one(st: &AppState, area_cfg: &crate::area::AreaConfig, pairs: &std::collections::BTreeMap<u8, f32>, g: &GenItem) -> Issue {
    let Ok(r) = st.robot(Some(g.robot)) else { return Issue::Abort(format!("로봇 {} 없음", g.robot)) };
    let step = &g.steps[g.next];
    // 짝 DROP 은 PICK 이 PLC 에 받아진 뒤(깊이 1: PICK 실행 중 → DROP 대기). PICK 이 나쁘게 끝났으면 DROP 은 보내지 않는다.
    if g.next == 1
        && let Some(pick) = g.task_ids.first().and_then(|id| st.find_task(id)).map(|(_, e)| e)
    {
        if matches!(pick.state, TaskState::Rejected | TaskState::Failed | TaskState::Canceled | TaskState::Lost) {
            return Issue::Abort(format!("짝 PICK 이 {}", pick.state.as_str()));
        }
        if matches!(pick.state, TaskState::Draft | TaskState::Submitted) {
            return Issue::Wait("짝 PICK 의 PLC 수령 대기".into());
        }
    }
    let gate = crate::ledger::ops::gate(st, r);
    if !gate.can_submit {
        return Issue::Wait(format!("제출 대기: {}", gate.reasons.join("; ")));
    }
    if let Some(why) = crate::scenario::runner::queue_depth_reason(r.ledger.list().iter().map(|e| e.state)) {
        return Issue::Wait(why);
    }
    let Some(x) = crate::area::target_x(st, &step.target) else { return Issue::Abort(format!("대상 {} {} 미등록", step.target.kind, step.target.id)) };
    let pair_drop = (g.next == 0 && g.steps.len() == 2).then_some(g.drop_x).flatten();
    let mut others = pairs.clone();
    others.remove(&g.robot);
    if let Some((b, mine)) = crate::area::check(st, area_cfg, r, x, pair_drop, &others) {
        return Issue::Wait(crate::area::wait_reason(&b, mine, area_cfg.separation_mm));
    }
    let mut order = g.transfer_order_id.clone();
    if g.next == 0 && g.steps.len() == 2 {
        let n = crate::stock::transfer::NewOrder {
            robot: Some(r.id),
            plc: r.plc.clone(),
            item_code: step.item_code.unwrap_or(0),
            count: step.count as u32,
            from: Some(step.target.clone()),
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
        ..Default::default()
    };
    let composed = match crate::issue::compose(st, &req) {
        Ok(c) => c,
        Err(e) => return Issue::Abort(format!("compose: {e}")),
    };
    match crate::ledger::ops::create_and_submit(st, r, Origin::Scenario, Some(req), Some(composed.params), composed.task, composed.pallet, true).await {
        Ok(entry) => Issue::Sent { task_id: entry.id, order },
        Err(e) => {
            if g.next == 0
                && let Some(o) = &order
            {
                let _ = st.stock.abort_order(o, "PICK 을 보내지 못함", true);
            }
            Issue::Wait(format!("제출 실패: {e}"))
        }
    }
}

pub fn spawn(st: AppState) {
    let e = Arc::new(Engine::load(st.db.clone()));
    let _ = ENGINE.set(e.clone());
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            tick_generate(&st, &e);
            tick_issue(&st, &e).await;
        }
    });
}
