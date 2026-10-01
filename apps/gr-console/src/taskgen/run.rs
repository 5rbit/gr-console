//! 생성 엔진 실행부 — 설정 · 예정 큐 · 수동 요청 수는 sqlite 에 남는다(재시작 뒤 이어서, 중복 생성 없음).
//! `params.gen_tick_ms` 마다: 세상 읽기(GRM 스테이션 신호 · **미래** 재고 · 예약 · 로봇 X) → 준비 상태 → 규칙(사용자 +
//! 요청 · 정책 파생) → 후보(자동 셀 선택, 팔렛 다음 슬롯 확인) → 판정(`select`) → (자동 켜짐이면) 예정 큐 → 발행기가
//! 실행기와 같은 규칙으로 보낸다: 게이트 · 큐 깊이 · 영역 · 짝(PICK 이 PLC 에 받아진 뒤 DROP) · 이송 지시 · Hand/StackMax.
//! 제출 실패는 `issue_retry_backoff_ms` 뒤 다시, `issue_max_retries` 를 넘으면 중단 → 그 키는 냉각(`gen_fail_cooldown_s`).
//! 시나리오 실행 중에는 만들지도 보내지도 않는다(두 발행자가 한 로봇에 겹치지 않게).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::profile::{DropMode, StationProfile};
use super::ready::{Hold, ItemFacts, ReadySnapshot, StationSig, TargetReady};
use super::requests::TransferRequest;
use super::{Action, Candidate, CellView, GenConfig, Knobs, RobotView, Rule, RuleOrigin, Selection, StationPi, Trigger, World, candidate_area, choose_dest, choose_source, score};
use crate::area::Interval;
use crate::error::ApiError;
use crate::ledger::{LedgerEntry, Origin, ScenarioSource, Target, TaskRequest, TaskState};
use crate::state::AppState;
use crate::util::now_str;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

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
    /// 규칙에서 끈 조건 — 제출 문도 같이 완화한다.
    #[serde(default)]
    pub allow_unknown_item: bool,
    #[serde(default)]
    pub ignore_stack_max: bool,
    /// 작업 대기열에 넣은 작업(2026-10-01) — 보내기는 대기열의 보내기 루프가 한다.
    #[serde(default)]
    pub job_id: Option<String>,
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
    /// 멀티 피킹 합치기 스텝 — PLC DoublePicking(부분 리프트, 요청 `multi_pick`).
    #[serde(default)]
    pub merge: bool,
}

/// 지표(재시작 때 0 — 오래 남길 것은 `taskgen_log`).
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

/// 실패 냉각 한 줄(화면 · API).
#[derive(Clone, Debug, Serialize)]
pub struct Cooldown {
    pub key: String,
    pub fails: u32,
    pub until: String,
    pub last_reason: String,
    #[serde(skip)]
    pub until_at: Option<Instant>,
}

/// 로봇 Hand 에 화물이 있는데 짝 DROP 이 없다.
#[derive(Clone, Debug, Serialize)]
pub struct HandAlert {
    pub robot: u8,
    pub robot_name: String,
    pub item_code: u32,
    pub count: u32,
    pub order: Option<String>,
    pub why: String,
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
    /// 사용자 규칙 상태(설정 순서) — 예전 화면용.
    pub rules: Mutex<Vec<super::RuleStatus>>,
    /// 사용자 + 파생 규칙 상태(요청 → 정책 → 사용자 순).
    pub derived: Mutex<Vec<super::RuleStatus>>,
    /// 엔진이 본 입력 한 벌(모니터).
    pub inputs: Mutex<super::EngineInputs>,
    /// 규칙 id → (만든 건수, 마지막 생성 시각) — 지표와 같은 수명.
    gen_stats: Mutex<HashMap<String, (u64, String)>>,
    pub note: Mutex<Option<String>>,
    pub metrics: Mutex<Metrics>,
    counter: std::sync::atomic::AtomicU64,
    /// 키(규칙 id) → 실패 냉각.
    pub cooldowns: Mutex<HashMap<String, Cooldown>>,
    /// (스테이션, pick|drop) → 신호가 준비된 시각.
    since: Mutex<HashMap<(u16, &'static str), String>>,
    /// 로봇 → 마지막으로 일이 있던 때(빈 시간 정리).
    busy_at: Mutex<HashMap<u8, Instant>>,
    /// 이송 지시 id → (키, 로봇) — 끝남 · 중단을 기록하려고 본다.
    watch_orders: Mutex<HashMap<String, (String, u8)>>,
    pub hand_alerts: Mutex<Vec<HandAlert>>,
    /// 열린 요청(판정마다 이송 지시에서 다시 센 것).
    pub open_requests: Mutex<Vec<TransferRequest>>,
    /// 요청 id → 지금 못 도는 이유.
    pub request_reasons: Mutex<HashMap<String, String>>,
    /// 대기 경고를 이미 낸 스테이션(준비가 풀리면 지운다).
    wait_alerted: Mutex<BTreeSet<u16>>,
    last_prune: Mutex<Option<Instant>>,
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
    } else if why.contains("예정·진행 중") || why.contains("같은 규칙") || why.contains("같은 스테이션") || why.contains("남은 수") || why.contains("이번 판정") {
        "busy"
    } else {
        "other"
    }
}

/// 요청 파생 규칙 id(`req:<id>@<대상>`) → 요청 id.
pub fn request_of(rule_id: &str) -> Option<&str> {
    rule_id.strip_prefix("req:").and_then(|x| x.split('@').next())
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
        // 재시작 전에 연 지시도 끝남 · 중단을 계속 본다.
        let watch = queue.iter().filter_map(|g| g.transfer_order_id.clone().map(|o| (o, (g.rule_id.clone(), g.robot)))).collect();
        Engine {
            db,
            cfg: Mutex::new(cfg),
            seen: Default::default(),
            manual: Mutex::new(manual),
            queue: Mutex::new(queue),
            inflight: Default::default(),
            last: Default::default(),
            rules: Default::default(),
            derived: Default::default(),
            inputs: Default::default(),
            gen_stats: Default::default(),
            note: Default::default(),
            metrics: Mutex::new(Metrics { started_at: now_str(), ..Default::default() }),
            counter: std::sync::atomic::AtomicU64::new(next),
            cooldowns: Default::default(),
            since: Default::default(),
            busy_at: Default::default(),
            watch_orders: Mutex::new(watch),
            hand_alerts: Default::default(),
            open_requests: Default::default(),
            request_reasons: Default::default(),
            wait_alerted: Default::default(),
            last_prune: Default::default(),
        }
    }
    pub fn config(&self) -> GenConfig {
        lock(&self.cfg).clone()
    }
    /// 저장 — 버전 +1, 이력은 표에 남는다. 파생 규칙은 저장하지 않는다(판정마다 다시 만든다).
    pub fn save(&self, mut c: GenConfig) -> Result<GenConfig, ApiError> {
        let old = self.config();
        let prev = old.version;
        c.version = prev + 1;
        c.rules.retain(|r| r.origin == RuleOrigin::User);
        for r in &mut c.rules {
            r.manual_requests = 0; // 요청 수는 `taskgen_manual`
            r.request_id = None;
            r.station = None;
        }
        let doc = serde_json::to_string(&c)?;
        self.db.with(|x| x.execute("INSERT INTO taskgen_config (version, doc_json, saved_at) VALUES (?1, ?2, ?3)", (c.version, &doc, now_str())))?;
        *lock(&self.cfg) = c.clone();
        crate::evtlog::console("CON_SETTINGS", "console", i64::from(c.version), 0, 0, format!("Task 생성 규칙 v{} ({} 규칙)", c.version, c.rules.len()));
        if old.auto != c.auto {
            crate::evtlog::console("CON_RUNNER", "console", i64::from(c.auto), 0, 0, format!("자동 생성 {}", if c.auto { "켜짐" } else { "꺼짐" }));
        }
        Ok(c)
    }
    fn persist_manual(&self, rule: &str, n: u32) {
        let _ = self.db.with(|c| c.execute("INSERT INTO taskgen_manual (rule_id, count) VALUES (?1, ?2) ON CONFLICT(rule_id) DO UPDATE SET count = excluded.count", (rule, n)));
    }
    pub fn request(&self, rule: &str) -> u32 {
        let mut m = lock(&self.manual);
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
        if lock(&self.inflight).contains(id) {
            return Err(ApiError::Conflict("지금 보내는 중 — 잠시 뒤 다시".into()));
        }
        let mut q = lock(&self.queue);
        let Some(i) = q.iter().position(|g| g.id == id) else { return Err(ApiError::NotFound(format!("gen item {id}"))) };
        if q[i].next > 0 {
            return Err(ApiError::Conflict("이미 첫 스텝을 보냄 — Task 취소로 지우세요".into()));
        }
        q.remove(i);
        drop(q);
        self.forget_item(id);
        Ok(())
    }
    /// 로봇의 보낸 예정을 지운다(Hand 정리 — Task 는 호출자가 취소한다).
    fn drop_robot_items(&self, robot: u8) -> Vec<GenItem> {
        let mut q = lock(&self.queue);
        let (gone, keep): (Vec<GenItem>, Vec<GenItem>) = q.drain(..).partition(|g| g.robot == robot && g.next > 0);
        *q = keep;
        drop(q);
        for g in &gone {
            self.forget_item(&g.id);
        }
        gone
    }
    fn count_wait(&self, why: &str) {
        let mut m = lock(&self.metrics);
        *m.waits.entry(wait_kind(why).to_string()).or_default() += 1;
    }
    /// 실패 냉각 — `gen_fail_cooldown_s` × 2^(n−1), 최대 `gen_fail_cooldown_max_s`. `gen_fail_alert` 번째부터 경고.
    fn cool(&self, p: &crate::params::Params, key: &str, robot: Option<u8>, why: &str) {
        let mut m = lock(&self.cooldowns);
        let c = m.entry(key.to_string()).or_insert(Cooldown { key: key.into(), fails: 0, until: String::new(), last_reason: String::new(), until_at: None });
        c.fails += 1;
        let base = u64::from(p.gen_fail_cooldown_s);
        let secs = (base << (c.fails - 1).min(10)).min(u64::from(p.gen_fail_cooldown_max_s).max(base));
        c.until_at = Some(Instant::now() + Duration::from_secs(secs));
        let until = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()) + time::Duration::seconds(secs as i64);
        c.until = until.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
        c.last_reason = why.to_string();
        let fails = c.fails;
        drop(m);
        super::log::write(&self.db, "cooldown", key, robot, &format!("{fails}회 실패 — {secs} s 냉각: {why}"), Some(secs as f64));
        if p.gen_fail_alert > 0 && fails >= p.gen_fail_alert {
            crate::evtlog::console("CON_SCHED", "console", i64::from(fails), secs as i64, 0, format!("{key} {fails}회 연속 실패 — {why}"));
        }
    }
    pub fn clear_cooldown(&self, key: &str) -> bool {
        lock(&self.cooldowns).remove(key).is_some()
    }
    fn cooling(&self, key: &str) -> Option<String> {
        let m = lock(&self.cooldowns);
        let c = m.get(key)?;
        c.until_at.filter(|t| *t > Instant::now()).map(|_| format!("냉각 중 ({}회 실패, {} 까지) — {}", c.fails, c.until, c.last_reason))
    }
    pub fn cooldown_rows(&self) -> Vec<Cooldown> {
        let now = Instant::now();
        let mut v: Vec<Cooldown> = lock(&self.cooldowns).values().filter(|c| c.until_at.is_some_and(|t| t > now)).cloned().collect();
        v.sort_by(|a, b| a.key.cmp(&b.key));
        v
    }
    /// 판정 한 번을 적용 없이(시뮬레이션) — `seen` 은 복사본을 쓴다.
    pub fn simulate(&self, st: &AppState, cfg: &GenConfig) -> Plan {
        let mut seen = lock(&self.seen).clone();
        plan(st, self, cfg, &mut seen)
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

/// 규칙 · 준비 상태가 보는 세상 한 벌.
struct Gathered {
    w: World,
    cells: Vec<CellView>,
    ready: Vec<TargetReady>,
    /// 실제 Hand(PLC 이름 → (품목, 개수, 지시)).
    hands: BTreeMap<String, (u32, u32, Option<String>)>,
    entries: Vec<(u8, LedgerEntry)>,
}

fn op_of(task_type: u8) -> &'static str {
    match gr_proto::TaskType::from_code(task_type) {
        Some(gr_proto::TaskType::Pick) => "PICK",
        Some(gr_proto::TaskType::Drop) => "DROP",
        Some(gr_proto::TaskType::Measure) => "MEASURE",
        _ => "MOVE",
    }
}

/// 원장 Task 가 대상을 잡고 있나 — 끝나지 않은 것(사람이 만든 초안은 빼고, 짝 초안은 넣는다).
fn holds_target(e: &LedgerEntry) -> bool {
    !e.state.is_terminal() && (e.state != TaskState::Draft || e.transfer_order_id.is_some())
}

fn step_delta(s: &GenStep) -> Option<(u16, u32, i32)> {
    let n = s.count.max(1) as i32;
    match s.task_type.as_str() {
        "PICK" => Some((s.target.id, s.item_code.unwrap_or(0), -n)),
        "DROP" => Some((s.target.id, s.item_code.unwrap_or(0), n)),
        _ => None,
    }
}

fn gather(st: &AppState, e: &Engine, cfg: &GenConfig, queue: &[GenItem]) -> Gathered {
    let mut w = World::default();
    {
        let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        w.now_min = u32::from(now.hour()) * 60 + u32::from(now.minute());
    }
    let registered = st.registry.stations().unwrap_or_default();
    // GRM 스테이션 신호 — 맵(`station_live`)과 같은 한 줄에서.
    let live = crate::issue::station_live::station_live_rows(st).unwrap_or_default();
    let mut sigs: BTreeMap<u16, StationSig> = live.iter().map(|l| (l.id, StationSig::from(l))).collect();
    // 규칙이 겨냥했지만 등록 안 된 스테이션도 GRM 에서 읽어 본다(옛 규칙).
    if let Some(h) = st.grm_plc()
        && let Some(d) = h.snap().db("OPCUA")
    {
        for r in &cfg.rules {
            if let Trigger::StationReq { station, .. } | Trigger::StationItem { station, .. } = r.trigger
                && !sigs.contains_key(&station)
                && let Some(pi) = station_pi(&d.json, station)
            {
                sigs.insert(station, StationSig { live: true, cvok: pi.cvok, req: pi.req, item_exist: pi.item_exist, ..Default::default() });
            }
        }
    }
    for (id, s) in &sigs {
        if s.live {
            w.stations.insert(*id, StationPi { cvok: s.cvok, req: s.req, item_exist: s.item_exist });
        }
    }
    // 품목 규격.
    for i in st.registry.items().unwrap_or_default() {
        if i.spec.profiles.iter().all(|p| p.rows.is_empty()) {
            w.unmeasured.insert(i.code);
        }
        w.items.insert(
            i.code,
            ItemFacts { stack_max: u32::from(i.spec.stack_max), height: i.item.height, compression: i.spec.compression.unwrap_or(0.0), od: i.item.outer_diameter, inner_dia: i.item.inner_diameter },
        );
    }
    // 미래 재고 = 표 + 진행 중 Task(projected_all) + 짝 초안 + 예정 큐의 아직 원장에 없는 스텝.
    let entries: Vec<(u8, LedgerEntry)> = st.robots.iter().flat_map(|r| r.ledger.list().into_iter().map(move |x| (r.id, x))).collect();
    let mut stock: BTreeMap<u16, (u32, u32)> = BTreeMap::new();
    if let Ok((cells, _)) = st.stock.projected_all(|| entries.iter().map(|(_, x)| x.clone()).collect()) {
        for c in cells {
            stock.insert(c.cell_id, (c.item_code, c.count));
        }
    }
    let hands: BTreeMap<String, (u32, u32, Option<String>)> = st.stock.hands().unwrap_or_default().into_iter().map(|h| (h.plc.clone(), (h.item_code, h.count, h.transfer_order_id))).collect();
    let mut drafts: Vec<&LedgerEntry> = entries.iter().map(|(_, x)| x).filter(|x| x.state == TaskState::Draft && x.transfer_order_id.is_some()).collect();
    drafts.sort_by_key(|x| x.seq);
    for d in drafts {
        if let Some((id, item, n)) = crate::stock::task_step(&d.plc_task) {
            let cur = stock.get(&id).copied().unwrap_or((0, 0));
            stock.insert(id, crate::stock::fold_step(cur, item, n));
        }
    }
    let mut holds: BTreeMap<u16, Vec<Hold>> = BTreeMap::new();
    let robot_name = |id: u8| st.robots.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| format!("로봇 {id}"));
    for (rid, x) in entries.iter().filter(|(_, x)| holds_target(x)) {
        let id = x.plc_task.cell.id;
        if id != 0 {
            holds.entry(id).or_default().push(Hold {
                robot: *rid,
                robot_name: robot_name(*rid),
                op: op_of(x.plc_task.task_type).into(),
                state: x.state.as_str().into(),
                order: x.transfer_order_id.clone(),
                task: Some(x.id.clone()),
            });
        }
    }
    for g in queue {
        for (i, s) in g.steps.iter().enumerate() {
            if i < g.task_ids.len() {
                continue; // 원장에 있다(보냈거나 짝 초안)
            }
            if let Some((id, item, n)) = step_delta(s) {
                let cur = stock.get(&id).copied().unwrap_or((0, 0));
                stock.insert(id, crate::stock::fold_step(cur, item, n));
            }
            holds.entry(s.target.id).or_default().push(Hold {
                robot: g.robot,
                robot_name: robot_name(g.robot),
                op: s.task_type.clone(),
                state: "planned".into(),
                order: g.transfer_order_id.clone(),
                task: None,
            });
        }
    }
    let stamps: HashMap<u16, String> = st.stock.list().unwrap_or_default().into_iter().map(|s| (s.cell_id, s.updated_at)).collect();
    let mut views = Vec::new();
    for c in st.registry.cells().unwrap_or_default() {
        let (item, count) = stock.get(&c.cell.id).copied().unwrap_or((0, 0));
        let room = if item == 0 { None } else { w.items.get(&item).filter(|f| f.stack_max > 0).map(|f| f.stack_max.saturating_sub(count)) };
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
    for v in &views {
        w.stock.insert(v.id, (v.item, v.count));
        w.usable.insert(v.id, v.use_);
        if let Some(r) = v.room {
            w.room.insert(v.id, r);
        }
    }
    // 스테이션 재고(컨베이어 트래킹이 옮긴 콘솔 재고) — 칸은 프로파일(DROP 방식)이 본다.
    let mut station_stock = BTreeMap::new();
    for s in &registered {
        let v = stock.get(&s.id).copied().unwrap_or((0, 0));
        station_stock.insert(s.id, v);
        w.stock.insert(s.id, v);
        w.usable.insert(s.id, s.para.info.use_);
    }
    w.profiles = super::profile::list(&st.db).unwrap_or_default().into_iter().map(|p| (p.station_id, p)).collect();
    w.multi_pick_max = u32::from(cfg.policy.multi_pick_max);
    let stations: Vec<(u16, bool)> = registered.iter().map(|s| (s.id, s.para.info.use_)).collect();
    let since = {
        let mut m = lock(&e.since);
        super::ready::update_since(&mut m, &sigs, &w.profiles, &now_str());
        m.clone()
    };
    let ready = super::ready::compute(&super::ready::Inputs {
        stations: &stations,
        sig: &sigs,
        profiles: &w.profiles,
        cells: &views,
        station_stock: &station_stock,
        items: &w.items,
        holds: &holds,
        since: &since,
    });
    w.ready = ready.iter().map(|t| (t.id, t.clone())).collect();
    Gathered { w, cells: views, ready, hands, entries }
}

/// 규칙 줄에 쓰는 한 줄 설명(`St 2101 → Auto`).
pub fn describe_action(a: &Action) -> String {
    let at = |t: &Target| format!("{} {}", if t.kind == "station" { "St" } else { "Cell" }, t.id);
    match a {
        Action::Transfer { from, to, from_auto, to_auto, .. } => {
            format!("{} → {}", if from_auto.is_some() { "Auto".to_string() } else { at(from) }, if to_auto.is_some() { "Auto".to_string() } else { at(to) })
        }
        Action::Move { to } => format!("MOVE {}", at(to)),
        Action::Measure { target, .. } => format!("MEASURE {}", at(target)),
    }
}

fn gen_busy(entries: &[(u8, LedgerEntry)], rule: &str) -> bool {
    let tag = format!("gen:{rule}");
    entries.iter().any(|(_, e)| !e.state.is_terminal() && e.state != TaskState::Draft && e.request.as_ref().and_then(|q| q.source.as_ref()).is_some_and(|s| s.scenario_id == tag))
}

/// 고른 대상으로 스텝을 만든다. `item` = 확정된 품목(짝의 두 스텝이 같은 값을 든다).
fn steps_for(a: &Action, first: &Target, second: Option<&Target>, item: Option<u32>) -> Vec<GenStep> {
    match a {
        Action::Transfer { count, pallet_auto, merge, .. } => vec![
            GenStep { task_type: "PICK".into(), target: first.clone(), item_code: item, count: *count, pallet_auto: false, merge: *merge },
            GenStep { task_type: "DROP".into(), target: second.cloned().unwrap_or_else(|| first.clone()), item_code: item, count: *count, pallet_auto: *pallet_auto, merge: *merge },
        ],
        Action::Move { .. } => vec![GenStep { task_type: "MOVE".into(), target: first.clone(), item_code: None, count: 1, pallet_auto: false, merge: false }],
        Action::Measure { .. } => vec![GenStep { task_type: "MEASURE".into(), target: first.clone(), item_code: item, count: 1, pallet_auto: false, merge: false }],
    }
}

/// 기준 대상의 X — 스테이션·셀 어느 쪽이든 등록된 것에서 찾는다(도착 셀 "가까운 순" 의 기준).
fn ref_x(st: &AppState, id: u16) -> Option<f32> {
    ["station", "cell"].iter().find_map(|k| crate::area::target_x(st, &Target { kind: (*k).to_string(), id }))
}

fn cell_target(id: u16) -> Target {
    Target { kind: "cell".into(), id }
}

/// 규칙의 대상과 품목 — 자동 셀 선택, 양쪽 위치 등록, 품목 확정, 스택 스테이션 높이까지. 하나라도 안 되면 사유.
fn resolve(st: &AppState, w: &World, rule: &Rule, cells: &[CellView], robot_x: Option<f32>, free: &dyn Fn(f32) -> bool) -> Result<(Target, Option<Target>, Option<u32>), String> {
    let c = &rule.cond;
    match &rule.action {
        Action::Transfer { from, to, item, count, from_auto, to_auto, pallet_auto, merge } => {
            let src = match from_auto {
                Some(p) => cell_target(choose_source(cells, p, *item, *count as u32, robot_x, free).ok_or("자동 출발 셀 없음 (재고·구역·영역)")?),
                None => from.clone(),
            };
            let stocked = w.stock.get(&src.id).map(|(x, _)| *x).filter(|x| *x != 0);
            // 규칙(요청)이 정한 품목과 출발 재고 품목이 다르면 만들지 않는다 — 잘못 집는다.
            if let (Some(want), Some(have)) = (item, stocked)
                && *want != have
            {
                return Err(format!("출발 {} {} 품목 {have} ≠ 요청 품목 {want}", src.kind, src.id));
            }
            // 품목은 GCS 가 알아야 한다 — 규칙이 정했거나 출발의 콘솔 재고가 안다(스테이션도 같다).
            let known = item.or(stocked);
            if c.item_known && known.is_none() {
                return Err(format!("출발 {} {} 품목을 콘솔이 모름", src.kind, src.id));
            }
            if let Some(code) = known
                && st.registry.item(code).ok().flatten().is_none()
            {
                return Err(format!("품목 {code} 이 콘솔 목록에 없음 (규격·StackMax 를 모름)"));
            }
            let src_item = known.unwrap_or(0);
            if let Some(why) = super::source_ready(w, &src, *count, known, c) {
                return Err(format!("출발 {} {}: {why}", src.kind, src.id));
            }
            let dst = match to_auto {
                Some(p) => cell_target(
                    choose_dest(cells, p, src_item, *count as u32, robot_x, p.near.and_then(|id| ref_x(st, id)), Some(src.id).filter(|_| src.kind == "cell"), free)
                        .ok_or("자동 도착 셀 없음 (칸·구역·영역)")?,
                ),
                None => to.clone(),
            };
            if let Some(why) = super::dest_ready(w, &dst, *count, c) {
                return Err(format!("도착 {} {}: {why}", dst.kind, dst.id));
            }
            // 멀티 피킹 합치기 — 같은 품목 1개 위(2단)에만.
            if *merge {
                super::merge_check(w, &src, &dst)?;
            }
            // 스택 스테이션 — 이 품목을 얹은 높이가 스테이션 최대 높이 안이어야, 품목 StackMax 도.
            if dst.kind == "station"
                && !*merge
                && let Some(p) = w.profiles.get(&dst.id).filter(|p| p.drop_mode == DropMode::Stack)
            {
                let (ci, cn) = w.stock.get(&dst.id).copied().unwrap_or((0, 0));
                let (ci, cn) = if w.stations.get(&dst.id).is_some_and(|s| !s.item_exist) { (0, 0) } else { (ci, cn) };
                super::ready::stack_room(&w.items, ci, cn, src_item, p.max_height_mm).map_err(|why| format!("도착 스테이션 {}: {why}", dst.id))?;
            }
            // 두 위치 모두 등록돼 있어야(=X 를 알아야) 보낼 수 있다.
            for t in [&src, &dst] {
                if crate::area::target_x(st, t).is_none() {
                    return Err(format!("{} {} 위치 미등록", t.kind, t.id));
                }
            }
            if *pallet_auto {
                // 팔렛 다음 슬롯: DROP 을 미리 작성해 본다(프로파일 없음 · 자리 없음이면 후보 아님)
                let req = TaskRequest {
                    task_type: "DROP".into(),
                    target: Some(dst.clone()),
                    item_code: Some(src_item),
                    count: (*count).max(1),
                    pallet: Some(crate::pallet::compose::PalletRef { auto: true, ..Default::default() }),
                    ..Default::default()
                };
                if let Err(e) = crate::issue::compose(st, &req) {
                    return Err(format!("팔렛 다음 슬롯 없음: {e}"));
                }
            }
            Ok((src, Some(dst), known))
        }
        Action::Move { to } => Ok((to.clone(), None, None)),
        Action::Measure { target, item } => Ok((target.clone(), None, item.or_else(|| w.stock.get(&target.id).map(|(x, _)| *x).filter(|x| *x != 0)))),
    }
}

fn stations_of(first: &Target, second: Option<&Target>) -> Vec<u16> {
    std::iter::once(first).chain(second).filter(|t| t.kind == "station").map(|t| t.id).collect()
}

/// 판정 한 번의 결과 — `tick_generate` 가 적용하고, 시뮬레이션은 보기만 한다.
pub struct Plan {
    pub sel: Selection,
    /// 판정한 규칙 전부(사용자 + 파생) — 적용 때 찾는다.
    pub rules: Vec<Rule>,
    pub statuses: Vec<super::RuleStatus>,
    pub inputs: super::EngineInputs,
    pub ready: ReadySnapshot,
    pub hand_alerts: Vec<HandAlert>,
    pub request_reasons: HashMap<String, String>,
    /// 스테이션 → 준비된 뒤 지난 초(대기 경고).
    pub station_wait: Vec<(u16, f64)>,
}

/// 판정 — 세상을 읽어 후보를 만들고 고른다. 큐에 넣지는 않는다. `seen` 은 조건이 처음 참이 된 때(대기 가점 · 순서).
pub fn plan(st: &AppState, e: &Engine, cfg: &GenConfig, seen: &mut HashMap<String, (Instant, u64)>) -> Plan {
    let p = crate::params::current(&st.db);
    let sep = if p.anticol_enabled && st.robots.len() > 1 { p.anticol_separation_mm } else { 0.0 };
    let knobs = Knobs { age_per_min: p.gen_age_per_min, distance_per_m: p.gen_distance_per_m };
    let queue = lock(&e.queue).clone();
    let g = gather(st, e, cfg, &queue);
    let w = &g.w;
    // Hand 경고 — 화물을 들었는데 짝 DROP 이 없으면 그 로봇에는 만들지 않는다(사람이 정리).
    let mut hand_alerts = Vec::new();
    for r in st.robots.iter() {
        let Some((item, n, order)) = g.hands.get(&r.plc).cloned() else { continue };
        if n == 0 {
            continue;
        }
        let live_drop = g.entries.iter().any(|(rid, x)| *rid == r.id && !x.state.is_terminal() && op_of(x.plc_task.task_type) == "DROP");
        let queued = queue.iter().any(|q| q.robot == r.id);
        if !live_drop && !queued {
            hand_alerts.push(HandAlert {
                robot: r.id,
                robot_name: r.name.clone(),
                item_code: item,
                count: n,
                order,
                why: "화물을 든 채 짝 DROP 이 없음 — Hand 정리(사람이 제거) 또는 DROP 단일 생성".into(),
            });
        }
    }
    let blocked_robots: BTreeSet<u8> = hand_alerts.iter().map(|h| h.robot).collect();
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
    // 빈 시간 — 예정도 진행 중 Task 도 없는 로봇이 `consolidate_idle_s` 넘게 쉬었다.
    let idle_robots: Vec<u8> = {
        let mut b = lock(&e.busy_at);
        let mut v = Vec::new();
        for r in &robots {
            let working = r.busy || !r.pending.is_empty();
            let at = b.entry(r.id).or_insert_with(Instant::now);
            if working {
                *at = Instant::now();
            } else if at.elapsed().as_secs() >= u64::from(cfg.policy.consolidate_idle_s) {
                v.push(r.id);
            }
        }
        v
    };
    // 규칙 = 요청 → 정책 → 사용자.
    let requests = lock(&e.open_requests).clone();
    let mut queued_for_request: BTreeMap<String, u32> = BTreeMap::new();
    for q in queue.iter().filter(|q| q.transfer_order_id.is_none()) {
        if let Some(r) = request_of(&q.rule_id) {
            *queued_for_request.entry(r.to_string()).or_default() += 1;
        }
    }
    let first_drop_station = w.profiles.values().find(|x| x.role.drops()).map(|x| x.station_id);
    let station_stock: BTreeMap<u16, (u32, u32)> = w.stock.iter().filter(|(id, _)| (2001..=2999).contains(*id)).map(|(k, v)| (*k, *v)).collect();
    let ctx = super::policy::Ctx { profiles: &w.profiles, first_drop_station, idle_robots, queued_for_request: &queued_for_request, station_stock: &station_stock };
    let mut rules = super::policy::derive(&cfg.policy, &requests, &ctx);
    {
        let m = lock(&e.manual);
        for r in &cfg.rules {
            let mut r = r.clone();
            r.manual_requests = m.get(&r.id).copied().unwrap_or(0);
            r.origin = RuleOrigin::User;
            rules.push(r);
        }
    }
    let caps: BTreeMap<String, u32> = requests.iter().map(|r| (r.id.clone(), super::policy::request_cap(r, &queued_for_request))).collect();
    let mut cands = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    // 규칙 id → (상태, 사유, 조건 참이 된 뒤 지난 분)
    let mut status: HashMap<String, (&'static str, Option<String>, f32)> = HashMap::new();
    let mut terms_of: HashMap<String, Vec<super::Term>> = HashMap::new();
    for rule in &rules {
        let (on, terms) = super::evaluate(rule, w);
        let first_bad = terms.iter().find(|t| !t.ok).map(|t| format!("{}: {}", t.label, t.value));
        terms_of.insert(rule.id.clone(), terms);
        if !on {
            seen.remove(&rule.id);
            status.insert(rule.id.clone(), (if rule.enabled { "idle" } else { "off" }, first_bad, 0.0));
            continue;
        }
        if queue.iter().any(|g| g.rule_id == rule.id) {
            status.insert(rule.id.clone(), ("queued", Some("이 규칙의 작업이 예정 큐에 있다".into()), 0.0));
            continue;
        }
        if gen_busy(&g.entries, &rule.id) {
            status.insert(rule.id.clone(), ("busy", Some("이 규칙으로 만든 Task 가 아직 진행 중".into()), 0.0));
            continue;
        }
        if let Some(why) = e.cooling(&rule.id) {
            status.insert(rule.id.clone(), ("cooldown", Some(why), 0.0));
            continue;
        }
        if rule.limits.per_hour > 0 {
            let n = super::log::count_since(&st.db, "generated", &rule.id, 3600);
            if n >= rule.limits.per_hour {
                status.insert(rule.id.clone(), ("limited", Some(format!("시간당 {}건 한도 — 최근 1 시간 {n}건", rule.limits.per_hour)), 0.0));
                continue;
            }
        }
        let (since, order) = *seen.entry(rule.id.clone()).or_insert_with(|| (Instant::now(), e.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let age = since.elapsed().as_secs_f32() / 60.0;
        let mut robot_count = 0;
        let skip_from = skipped.len();
        for rv in robots.iter().filter(|rv| rule.robots.is_empty() || rule.robots.contains(&rv.id)) {
            robot_count += 1;
            if blocked_robots.contains(&rv.id) {
                skipped.push((rule.name.clone(), format!("{}: Hand 에 짝 없는 화물 — 정리 필요", rv.name)));
                continue;
            }
            // 자동 셀은 다른 로봇 영역 밖에서 고른다(동시 운전)
            let others: Vec<crate::area::Reservation> = robots.iter().filter(|o| o.id != rv.id).filter_map(|o| super::reserved(o, None)).collect();
            let free = |x: f32| crate::area::blocker(Interval::point(x), &others, sep + rv.margin).is_none();
            // 거리는 로봇이 앞 작업을 끝낸 자리에서 — 이어지는 작업(복합 사이클)이 가까우면 먼저.
            let end_x = rv.pending.last().copied().or(rv.current_x);
            let (first, second, item) = match resolve(st, w, rule, &g.cells, end_x, &free) {
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
            let dist = end_x.map(|x| (x - x0).abs() / 1000.0).unwrap_or(0.0);
            if p.gen_max_distance_m > 0.0 && dist > p.gen_max_distance_m {
                skipped.push((rule.name.clone(), format!("{}: 거리 {dist:.1} m > {:.1} m", rv.name, p.gen_max_distance_m)));
                continue;
            }
            let targets: Vec<&Target> = std::iter::once(&first).chain(second.as_ref()).collect();
            let xs: Vec<f32> = std::iter::once(x0).chain(drop_x).collect();
            let ids: Vec<u16> = targets.iter().map(|t| t.id).collect();
            let inner = item.and_then(|c| w.items.get(&c)).map(|f| f.inner_dia).filter(|d| *d > 0.0);
            let robot_bonus = match super::robot_rule_check(&cfg.robot_rules, rv.id, &xs, inner, &ids) {
                Ok(b) => b,
                Err(why) => {
                    skipped.push((rule.name.clone(), format!("{}: {why}", rv.name)));
                    continue;
                }
            };
            let (mut s, mut b) = score(rule, &targets, rv.id, &cfg.weights, knobs, age, dist);
            if robot_bonus != 0.0 {
                s += robot_bonus;
                b.push(("RobotRule".into(), robot_bonus));
            }
            cands.push(Candidate {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                robot: rv.id,
                stations: stations_of(&first, second.as_ref()),
                request_id: rule.request_id.clone(),
                first: first.clone(),
                second: second.clone(),
                item,
                target_x: x0,
                drop_x,
                area: candidate_area(rv, x0, drop_x),
                score: s,
                breakdown: b,
                order,
                age_min: age,
            });
        }
        let made = cands.iter().any(|c| c.rule_id == rule.id);
        let why = if robot_count == 0 { Some("보낼 로봇이 없다 — 규칙의 Robots 를 확인".to_string()) } else { skipped[skip_from..].last().map(|(_, w)| w.clone()) };
        status.insert(rule.id.clone(), if made { ("ready", None, age) } else { ("skipped", why, age) });
    }
    let mut sel = super::select_with(cands, &robots, sep, p.gen_avoid_bonus, &caps);
    sel.skipped = skipped;
    for (c, why) in &mut sel.waiting {
        e.count_wait(why);
        // 같은 규칙의 다른 로봇 후보가 생성됐으면 규칙 줄은 생성으로 둔다.
        if !sel.generate.iter().any(|g| g.rule_id == c.rule_id)
            && let Some(s) = status.get_mut(&c.rule_id)
        {
            *s = ("waiting", Some(why.clone()), c.age_min);
        }
        if p.gen_blocked_penalty != 0.0 && why.starts_with("영역") {
            c.score -= p.gen_blocked_penalty;
            c.breakdown.push(("AntiCollision".into(), -p.gen_blocked_penalty));
        }
    }
    let gs = lock(&e.gen_stats).clone();
    let statuses: Vec<super::RuleStatus> = rules
        .iter()
        .map(|r| {
            let (state, reason, age) = status.get(&r.id).cloned().unwrap_or(("idle", None, 0.0));
            let (generated, last) = gs.get(&r.id).cloned().map(|(n, at)| (n, Some(at))).unwrap_or((0, None));
            let terms = terms_of.remove(&r.id).unwrap_or_default();
            super::RuleStatus {
                rule_id: r.id.clone(),
                fires: !matches!(state, "off" | "idle"),
                inputs: terms.iter().map(|x| format!("{} {}{}", x.label, x.value, if x.ok { "" } else { " ✗" })).collect::<Vec<_>>().join(" · "),
                terms,
                state: state.to_string(),
                reason,
                age_min: age,
                generated,
                last_generated_at: last,
                origin: r.origin,
                name: r.name.clone(),
                request_id: r.request_id.clone(),
                station: r.station,
            }
        })
        .collect();
    // 요청마다 지금 못 도는 이유 — 파생 규칙 중 가장 앞선 사유.
    let mut request_reasons = HashMap::new();
    for rq in &requests {
        let mine: Vec<&super::RuleStatus> = statuses.iter().filter(|s| s.request_id.as_deref() == Some(rq.id.as_str())).collect();
        if mine.iter().any(|s| matches!(s.state.as_str(), "ready" | "queued" | "busy")) {
            continue;
        }
        let why = if mine.is_empty() {
            if super::policy::request_cap(rq, &queued_for_request) == 0 {
                "남은 수는 예정 · 진행 중".to_string()
            } else {
                match rq.kind {
                    super::requests::ReqKind::Inbound => "PICK 스테이션 없음 — 스테이션 프로파일 역할을 정하세요".into(),
                    super::requests::ReqKind::Outbound => "DROP 스테이션 없음 — 스테이션 프로파일 역할을 정하세요".into(),
                    super::requests::ReqKind::Move => "출발 · 도착 셀을 정해야 함".into(),
                }
            }
        } else {
            mine.iter().find_map(|s| s.reason.clone()).unwrap_or_else(|| "조건 대기".into())
        };
        request_reasons.insert(rq.id.clone(), why);
    }
    let station_wait: Vec<(u16, f64)> = g.ready.iter().filter(|t| t.kind == "station").filter_map(|t| Some((t.id, secs_since(t.since.as_deref()?)?))).collect();
    let inputs = engine_inputs(cfg, w, &robots, &p, sep);
    Plan { sel, rules, statuses, inputs, ready: ReadySnapshot { at: now_str(), targets: g.ready }, hand_alerts, request_reasons, station_wait }
}

fn secs_since(at: &str) -> Option<f64> {
    let t = time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).ok()?;
    Some((time::OffsetDateTime::now_utc() - t).as_seconds_f64().max(0.0))
}

/// 한 번 판정(자동이면 생성까지). 결과는 엔진에.
pub fn tick_generate(st: &AppState, e: &Engine) {
    let cfg = e.config();
    let p = crate::params::current(&st.db);
    match super::requests::refresh_open(&st.db) {
        Ok(v) => *lock(&e.open_requests) = v,
        Err(err) => tracing::warn!(%err, "taskgen: 요청 목록을 읽지 못함"),
    }
    watch_orders(st, e, &p);
    let plan = {
        let mut seen = lock(&e.seen);
        plan(st, e, &cfg, &mut seen)
    };
    *lock(&e.note) = if !cfg.auto { Some("자동 생성 꺼짐 — 후보만 보여 줌".into()) } else { None };
    if cfg.auto {
        let mut q = lock(&e.queue);
        for c in &plan.sel.generate {
            let Some(rule) = plan.rules.iter().find(|r| r.id == c.rule_id) else { continue };
            if matches!(rule.trigger, Trigger::Manual) {
                let mut m = lock(&e.manual);
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
                steps: steps_for(&rule.action, &c.first, c.second.as_ref(), c.item),
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
                allow_unknown_item: !rule.cond.item_known,
                ignore_stack_max: !rule.cond.dest_room,
                job_id: None,
            };
            let at = g.created_at.clone();
            e.persist_item(&g);
            let route = g.steps.iter().map(|s| format!("{} {} {}", s.task_type, s.target.kind, s.target.id)).collect::<Vec<_>>().join(" → ");
            super::log::write(&st.db, "generated", &c.rule_id, Some(c.robot), &format!("{} · {route} · 점수 {:.1}", c.rule_name, c.score), Some(f64::from(c.age_min) * 60.0));
            q.push(g);
            lock(&e.metrics).generated += 1;
            let mut gs = lock(&e.gen_stats);
            let s = gs.entry(c.rule_id.clone()).or_insert((0, at.clone()));
            s.0 += 1;
            s.1 = at;
        }
    }
    // 준비된 뒤 오래 못 만든 스테이션 — 경고 한 번(준비가 풀리면 다시 켠다).
    {
        let profiles: BTreeMap<u16, StationProfile> = super::profile::list(&st.db).unwrap_or_default().into_iter().map(|x| (x.station_id, x)).collect();
        let mut alerted = lock(&e.wait_alerted);
        let waiting: BTreeSet<u16> = plan.station_wait.iter().map(|(s, _)| *s).collect();
        alerted.retain(|s| waiting.contains(s));
        for (s, secs) in &plan.station_wait {
            let Some(limit) = profiles.get(s).map(|x| x.max_wait_s).filter(|x| *x > 0) else { continue };
            if *secs > f64::from(limit) && alerted.insert(*s) {
                crate::evtlog::console("CON_SCHED", "console", i64::from(*s), *secs as i64, 0, format!("스테이션 {s} 준비 뒤 {secs:.0} s 동안 작업 없음 (한도 {limit} s)"));
                super::log::write(&st.db, "wait_alert", &s.to_string(), None, &format!("준비 뒤 {secs:.0} s (한도 {limit} s)"), Some(*secs));
            }
        }
    }
    *lock(&e.rules) = plan.statuses.iter().filter(|s| s.origin == RuleOrigin::User).cloned().collect();
    *lock(&e.derived) = plan.statuses;
    *lock(&e.inputs) = plan.inputs;
    *lock(&e.hand_alerts) = plan.hand_alerts;
    *lock(&e.request_reasons) = plan.request_reasons;
    super::ready::publish(plan.ready);
    *lock(&e.last) = plan.sel;
    let mut lp = lock(&e.last_prune);
    if lp.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) {
        *lp = Some(Instant::now());
        super::log::prune(&st.db);
    }
}

/// 엔진이 연 이송 지시의 끝을 본다 — 끝남(기록 · 냉각 풀기) · 실패/중단(기록 · 냉각).
fn watch_orders(st: &AppState, e: &Engine, p: &crate::params::Params) {
    let list: Vec<(String, (String, u8))> = lock(&e.watch_orders).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (id, (key, robot)) in list {
        let Ok(Some(o)) = st.stock.order(&id) else {
            lock(&e.watch_orders).remove(&id);
            continue;
        };
        use crate::stock::transfer::OrderState as S;
        match o.state {
            S::Done => {
                super::log::write(&st.db, "completed", &key, Some(robot), &format!("{id} 끝남"), None);
                lock(&e.metrics).completed_items += 1;
                e.clear_cooldown(&key);
                lock(&e.watch_orders).remove(&id);
            }
            S::Failed | S::Aborted => {
                let why = o.history.last().map(|h| h.note.clone()).unwrap_or_default();
                super::log::write(&st.db, "aborted", &key, Some(robot), &format!("{id} {} — {why}", o.state.as_str()), None);
                e.cool(p, &key, Some(robot), &format!("{id} {}", o.state.as_str()));
                lock(&e.watch_orders).remove(&id);
            }
            _ => {}
        }
    }
}

/// 모니터용 입력 한 벌 — 스테이션은 전부, 셀은 규칙이 겨냥한 것과 재고가 있는 것만(많아서 자른다).
fn engine_inputs(cfg: &GenConfig, w: &World, robots: &[RobotView], p: &crate::params::Params, sep: f32) -> super::EngineInputs {
    let watch_station: BTreeSet<u16> = cfg
        .rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| match r.trigger {
            Trigger::StationReq { station, .. } | Trigger::StationItem { station, .. } => Some(station),
            _ => None,
        })
        .chain(w.profiles.values().filter(|x| x.role != super::profile::StationRole::Off).map(|x| x.station_id))
        .collect();
    let mut watch_cell: BTreeSet<u16> = BTreeSet::new();
    for r in cfg.rules.iter().filter(|r| r.enabled) {
        if let Trigger::CellStock { cell, .. } = r.trigger {
            watch_cell.insert(cell);
        }
        match &r.action {
            Action::Transfer { from, to, from_auto, to_auto, .. } => {
                if from_auto.is_none() && from.kind == "cell" {
                    watch_cell.insert(from.id);
                }
                if to_auto.is_none() && to.kind == "cell" {
                    watch_cell.insert(to.id);
                }
            }
            Action::Move { to } => {
                if to.kind == "cell" {
                    watch_cell.insert(to.id);
                }
            }
            Action::Measure { target, .. } => {
                if target.kind == "cell" {
                    watch_cell.insert(target.id);
                }
            }
        }
    }
    const CELL_LIMIT: usize = 60;
    let cells: Vec<super::CellRow> = w
        .stock
        .iter()
        .filter(|(id, _)| !(2001..=2999).contains(*id))
        .filter(|(id, (_, n))| *n > 0 || watch_cell.contains(id))
        .take(CELL_LIMIT)
        .map(|(id, (code, n))| super::CellRow { id: *id, item_code: *code, count: *n, room: w.room.get(id).copied(), watched: watch_cell.contains(id) })
        .collect();
    super::EngineInputs {
        stations: w.stations.iter().map(|(id, pi)| super::StationRow { id: *id, cvok: pi.cvok, req: pi.req, item_exist: pi.item_exist, watched: watch_station.contains(id) }).collect(),
        cells,
        robots: robots.iter().map(|r| super::RobotRow { id: r.id, name: r.name.clone(), x: r.current_x, busy: r.busy }).collect(),
        separation_mm: sep,
        queue_depth: p.issue_queue_depth,
        tick_ms: p.gen_tick_ms,
        updated_at: now_str(),
    }
}

/// 예정 큐를 보낸다 — 로봇마다 맨 앞 항목의 다음 스텝 하나.
/// 자동 생성이 꺼져도 이미 대기열에 넣은 작업은 끝까지 따라간다(취소 · 실패 · 보냄을 놓치면 로봇이 계속 바쁨으로 남는다).
pub async fn tick_issue(st: &AppState, e: &Engine) {
    let auto = e.config().auto;
    let p = crate::params::current(&st.db);
    let items = lock(&e.queue).clone();
    // 짝이 걸린 로봇(PICK 보냄, DROP 아직)의 DROP 목표 — 다른 로봇 영역 검사에.
    let pairs: BTreeMap<u8, f32> = items.iter().filter(|g| g.next == 1 && g.steps.len() == 2).filter_map(|g| g.drop_x.map(|x| (g.robot, x))).collect();
    let mut done_robots: Vec<u8> = Vec::new();
    for g in items {
        // 꺼져 있으면 새로 넣지 않고, 이미 넣은 작업만 모두 따라간다.
        if !auto && g.job_id.is_none() {
            continue;
        }
        if auto {
            if done_robots.contains(&g.robot) {
                continue;
            }
            done_robots.push(g.robot);
        }
        if g.retry_at_ms > unix_ms() {
            continue;
        }
        lock(&e.inflight).insert(g.id.clone());
        let outcome = issue_one(st, &p, &pairs, &g).await;
        lock(&e.inflight).remove(&g.id);
        let mut drafts_to_discard: Vec<String> = Vec::new();
        {
            let mut q = lock(&e.queue);
            let Some(item) = q.iter_mut().find(|x| x.id == g.id) else { continue };
            match outcome {
                Issue::Queued { job_id, order } => {
                    item.job_id = Some(job_id);
                    if order.is_some() {
                        item.transfer_order_id = order;
                    }
                    if let Some(o) = &item.transfer_order_id {
                        lock(&e.watch_orders).insert(o.clone(), (item.rule_id.clone(), item.robot));
                    }
                    super::log::write(&st.db, "queued", &item.rule_id, Some(item.robot), &format!("작업 대기열 · {}", item.transfer_order_id.as_deref().unwrap_or("-")), None);
                    item.note = Some("작업 대기열에서 차례 기다림".into());
                    item.attempts = 0;
                    item.retry_at_ms = 0;
                    e.persist_item(item);
                }
                Issue::Issued { task_ids } => {
                    for (i, step) in item.steps.iter().enumerate() {
                        super::log::write(
                            &st.db,
                            "issued",
                            &item.rule_id,
                            Some(item.robot),
                            &format!("{} {} {} · {}", step.task_type, step.target.kind, step.target.id, item.transfer_order_id.as_deref().unwrap_or("-")),
                            None,
                        );
                        let _ = i;
                    }
                    item.task_ids = task_ids;
                    item.next = item.steps.len();
                    lock(&e.metrics).issued += item.steps.len() as u64;
                    let id = item.id.clone();
                    let single = item.transfer_order_id.is_none();
                    let (key, robot) = (item.rule_id.clone(), item.robot);
                    q.retain(|x| x.id != id);
                    e.forget_item(&id);
                    // 지시 없는 한 스텝(측정 · 이동)은 보낸 것으로 끝 — 냉각을 푼다.
                    if single {
                        e.clear_cooldown(&key);
                        super::log::write(&st.db, "completed", &key, Some(robot), "발행 끝(단일 스텝)", None);
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
                    lock(&e.metrics).submit_failures += 1;
                    if let Some(o) = &item.transfer_order_id {
                        let _ = st.stock.note_order(o, &format!("제출 실패 {}회: {why}", item.attempts));
                    }
                    if item.attempts > p.issue_max_retries {
                        tracing::warn!(item = %g.id, %why, "taskgen: gave up after retries");
                        drafts_to_discard = item.task_ids.clone();
                        if let Some(o) = &item.transfer_order_id
                            && item.next == 0
                        {
                            let _ = st.stock.abort_order(o, &format!("제출 {}회 실패 — 생성 작업 중단", item.attempts), false);
                        }
                        let (id, key, robot) = (item.id.clone(), item.rule_id.clone(), item.robot);
                        q.retain(|x| x.id != id);
                        e.forget_item(&id);
                        lock(&e.metrics).aborted += 1;
                        super::log::write(&st.db, "aborted", &key, Some(robot), &format!("제출 실패 — {why}"), None);
                        e.cool(&p, &key, Some(robot), &why);
                    } else {
                        item.retry_at_ms = unix_ms() + p.issue_retry_backoff_ms;
                        item.note = Some(format!("재시도 {}/{} ({} ms 뒤): {why}", item.attempts, p.issue_max_retries, p.issue_retry_backoff_ms));
                        e.count_wait("재시도");
                        e.persist_item(item);
                    }
                }
                Issue::Abort(why) => {
                    tracing::warn!(item = %g.id, %why, "taskgen: item aborted");
                    drafts_to_discard = item.task_ids.clone();
                    if let Some(o) = &item.transfer_order_id {
                        let _ = st.stock.abort_order(o, &format!("생성 작업 중단: {why}"), false);
                    }
                    let (id, key, robot) = (item.id.clone(), item.rule_id.clone(), item.robot);
                    q.retain(|x| x.id != id);
                    e.forget_item(&id);
                    lock(&e.metrics).aborted += 1;
                    super::log::write(&st.db, "aborted", &key, Some(robot), &why, None);
                    e.cool(&p, &key, Some(robot), &why);
                }
            }
        }
        discard_drafts(st, &drafts_to_discard).await;
    }
}

/// 보내지 않은 짝 초안을 지운다 — 중단된 짝의 DROP 이 원장에 남지 않게.
async fn discard_drafts(st: &AppState, ids: &[String]) {
    for id in ids {
        if let Some((_, e)) = st.find_task(id)
            && e.state == TaskState::Draft
            && let Err(err) = crate::ledger::ops::cancel(st, id).await
        {
            tracing::warn!(task = %id, %err, "taskgen: 짝 초안 지우기 실패");
        }
    }
}

/// Hand 정리 — 사람이 그리퍼의 화물을 들어낸다. 재고(셀)에는 반영하지 않고 Hand 를 비우며, 그 로봇의 짝 Task
/// (살아 있는 DROP · 짝 초안)를 취소하고 이송 지시를 중단한다. PLC 취소 규칙(AUTO 거부 등)은 그대로 걸린다.
pub async fn hand_remove(st: &AppState, robot: u8) -> Result<(Vec<String>, Option<String>), ApiError> {
    let r = st.robot(Some(robot))?;
    let hand = st.stock.hand(&r.plc)?;
    // 살아 있는 DROP(보낸 것 먼저, 초안은 뒤) — 취소가 짝을 같이 정리한다.
    let mut live: Vec<LedgerEntry> = r.ledger.list().into_iter().filter(|x| !x.state.is_terminal() && op_of(x.plc_task.task_type) == "DROP").collect();
    live.sort_by_key(|x| (x.state == TaskState::Draft, x.seq));
    let mut canceled = Vec::new();
    for x in live {
        let fresh = r.ledger.get(&x.id).unwrap_or(x);
        if fresh.state.is_terminal() {
            continue;
        }
        crate::ledger::ops::cancel(st, &fresh.id).await?;
        canceled.push(fresh.id);
    }
    if let Some(e) = engine() {
        e.drop_robot_items(robot);
    }
    let order = hand.transfer_order_id.clone();
    st.stock.set_hand(&r.plc, 0, 0, "manual-remove: 사람이 화물 제거 — 재고 반영 없음", None, order.as_deref())?;
    if let Some(o) = &order {
        st.stock.abort_order(o, "Hand 정리 — 사람이 화물 제거(재고 반영 없음)", false)?;
    }
    let what = format!("{} Hand 정리: 품목 {} × {} (취소 {} 건)", r.name, hand.item_code, hand.count, canceled.len());
    crate::evtlog::console("CON_SCHED", &r.plc, i64::from(hand.item_code), i64::from(hand.count), 0, what.clone());
    super::log::write(&st.db, "hand_remove", &r.name, Some(robot), &what, None);
    Ok((canceled, order))
}

enum Issue {
    /// 작업 대기열에 넣었다(WorkId · 이송 지시는 대기열이 붙였다).
    Queued {
        job_id: String,
        order: Option<String>,
    },
    /// 대기열의 보내기 루프가 이 작업의 스텝을 모두 PLC 로 보냈다.
    Issued {
        task_ids: Vec<String>,
    },
    Wait(String),
    Failed {
        why: String,
        order: Option<String>,
    },
    Abort(String),
}

/// 이송 지시 출처 — 요청 규칙이면 `req:<id>@<대상>`(요청 진행을 이것으로 센다), 그 밖은 `gen:<규칙>`.
fn order_source(rule_id: &str) -> String {
    if rule_id.starts_with("req:") { rule_id.to_string() } else { format!("gen:{rule_id}") }
}

/// 생성 작업의 스텝 `i` 를 작성 요청으로(출처 태그 `gen:<규칙>` 은 `gen_busy` 가 본다).
fn step_request(g: &GenItem, i: usize) -> TaskRequest {
    let step = &g.steps[i];
    TaskRequest {
        task_type: step.task_type.clone(),
        target: Some(step.target.clone()),
        item_code: step.item_code,
        count: step.count.max(1),
        params: serde_json::json!({}),
        note: if i == 0 { format!("생성: {}", g.rule_name) } else { format!("생성: {} (짝 DROP)", g.rule_name) },
        source: Some(ScenarioSource { scenario_id: format!("gen:{}", g.rule_id), run_id: g.id.clone(), iteration: 0, step_index: i as u32 }),
        robot: Some(g.robot),
        allow_unknown_item: g.allow_unknown_item,
        ignore_stack_max: g.ignore_stack_max,
        pallet: step.pallet_auto.then(|| crate::pallet::compose::PalletRef { auto: true, ..Default::default() }),
        multi_pick: step.merge.then_some(true),
        ..Default::default()
    }
}

/// 자동 작업의 우선(사람 작업 50 보다 뒤, 결정 2026-10-01).
const AUTO_PRIORITY: i32 = 30;

/// 생성 작업 하나를 작업 대기열로. 보내기(게이트 · 깊이 · 영역 · 짝 이어 보내기)는 대기열의 보내기 루프가 한다 —
/// 송신자가 하나여야 같은 로봇에 두 곳이 밀어 넣지 않는다.
async fn issue_one(st: &AppState, _p: &crate::params::Params, _pairs: &BTreeMap<u8, f32>, g: &GenItem) -> Issue {
    let Some(store) = crate::jobs::store() else { return Issue::Wait("작업 대기열 준비 중".into()) };
    let Some(job_id) = &g.job_id else {
        let steps: Vec<TaskRequest> = (0..g.steps.len()).map(|i| step_request(g, i)).collect();
        let n = crate::jobs::NewJob { robot: Some(g.robot), steps, priority: Some(AUTO_PRIORITY), note: g.rule_name.clone(), force_cargo: false };
        return match crate::jobs::enqueue(st, n, Origin::Auto, Some(order_source(&g.rule_id))) {
            Ok(job) => Issue::Queued { job_id: job.id, order: job.transfer_order_id },
            Err(e) => Issue::Failed { why: format!("대기열: {e}"), order: None },
        };
    };
    let Some(job) = store.get(job_id) else { return Issue::Abort("작업 대기열에서 작업을 찾지 못함".into()) };
    let sent: Vec<String> = job.steps.iter().filter_map(|s| s.task.clone()).collect();
    if sent.len() == job.steps.len() {
        return Issue::Issued { task_ids: sent };
    }
    match job.stage {
        crate::jobs::Stage::Canceled | crate::jobs::Stage::Failed => Issue::Abort(format!("작업 {}", job.stage.label())),
        _ => Issue::Wait(job.wait_reason.unwrap_or_else(|| "작업 대기열에서 차례 기다림".into())),
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
        assert_eq!(wait_kind("같은 스테이션 2101 을 이번 판정에서 다른 작업이 맡음"), "busy");
    }

    #[test]
    fn request_ids_and_order_sources() {
        assert_eq!(request_of("req:RQ-260928-0001@2101"), Some("RQ-260928-0001"));
        assert_eq!(request_of("in-2101"), None);
        assert_eq!(order_source("req:RQ-1@2101"), "req:RQ-1@2101");
        assert_eq!(order_source("in-2101"), "gen:in-2101");
    }

    /// 예정 큐의 아직 원장에 없는 스텝은 미래 재고에 접힌다 — PICK −1, DROP +1.
    #[test]
    fn queued_steps_fold_into_future_stock() {
        let s = |t: &str, id: u16| GenStep { task_type: t.into(), target: Target { kind: "cell".into(), id }, item_code: Some(2011), count: 1, pallet_auto: false, merge: false };
        assert_eq!(step_delta(&s("PICK", 401)), Some((401, 2011, -1)));
        assert_eq!(step_delta(&s("DROP", 402)), Some((402, 2011, 1)));
        assert_eq!(step_delta(&s("MEASURE", 402)), None);
        assert_eq!(crate::stock::fold_step((2011, 2), 2011, -1), (2011, 1));
        assert_eq!(crate::stock::fold_step((2011, 1), 2011, -1), (0, 0));
        assert_eq!(crate::stock::fold_step((0, 0), 2013, 1), (2013, 1));
    }

    #[test]
    fn cooldown_backs_off_and_clears() {
        let db = crate::db::Db::open_memory().unwrap();
        let e = Engine::load(db);
        let p = crate::params::Params { gen_fail_cooldown_s: 30, gen_fail_cooldown_max_s: 100, ..Default::default() };
        e.cool(&p, "in-2101", Some(2), "PLC 거부");
        assert!(e.cooling("in-2101").unwrap().contains("1회"));
        e.cool(&p, "in-2101", Some(2), "PLC 거부");
        e.cool(&p, "in-2101", Some(2), "PLC 거부");
        let rows = e.cooldown_rows();
        assert_eq!((rows.len(), rows[0].fails), (1, 3));
        assert!(e.clear_cooldown("in-2101"));
        assert!(e.cooling("in-2101").is_none());
        assert_eq!(super::super::log::list(&e.db, 10, Some("cooldown")).unwrap().len(), 3);
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
            allow_unknown_item: false,
            ignore_stack_max: false,
            job_id: None,
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
        assert!(back.watch_orders.lock().unwrap().contains_key("TO-1"), "open orders are watched after a restart");
    }
}
