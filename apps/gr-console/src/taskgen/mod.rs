//! Task 생성 엔진 — 콘솔이 GCS(SRC 5000)로서 유일한 생성원이다. GRM 은 명령 중계(`OPCUA.GR[n].CMD`)와 신호원
//! (`OPCUA.STATION[n].Interlock.PI`)일 뿐이다.
//!
//! - 규칙 = 조건(`Trigger`) + 만들 것(`Action`) + 로봇 + 기본 우선순위. 코드가 아니라 저장된 JSON(버전)이다.
//! - 출발·도착 셀은 고정하거나 자동으로 고른다(`CellPick` — 구역 필터, 가장 오래된/가까운 재고, 같은 품목 먼저,
//!   StackMax 칸, 다른 로봇 영역 밖). 팔렛 스테이션 DROP 은 다음 슬롯을 compose 가 고른다(`pallet_auto`).
//! - 점수 = 규칙 기본값 + 대상 · 품목 · 로봇 가중(규칙 설정) + 대기 가점 − 거리 감점(파라미터, `params.rs`).
//!   같은 점수면 조건이 먼저 참이 된 후보가 먼저 — 결정적.
//! - 영역(`select`): 후보 영역(짝이면 PICK∪DROP)이 다른 로봇이 잡은 영역(발행 · 예정 · 이번에 고른 것)과
//!   간격(+ 로봇별 여유) 안이면 지금은 만들지 않는다. 겹치지 않는 후보를 먼저 골라 두 로봇이 동시에 돈다.
//!   막힌 로봇은 상대가 비켜 가는 작업을 만들면 그 명령이 PLC 에 **받아진 뒤**(`area::reservation` 의 committed)
//!   다음 판정에서 풀리고, 비켜 줄 것이 없으면 사유와 함께 기다린다. 자동 회피 MOVE 는 만들지 않는다.
//! - 스케줄러(docs/scheduler.md): 사용자 규칙 말고도 **요청 목록**과 **정책**이 판정마다 규칙을 파생한다(`policy::derive`).
//!   스테이션 작업은 어느 규칙이든 준비 상태(`ready`: `CVOK & Req` · 화물 유무 · 인계 · 예약)가 있어야 만든다.

pub mod log;
pub mod policy;
pub mod profile;
pub mod ready;
pub mod requests;
pub mod routes;
pub mod run;

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::area::{Interval, Reservation, blocker, reservation, task_area};
use crate::ledger::Target;

/// 조건.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// 운전자가 "요청" 을 누른 수만큼.
    Manual,
    /// GRM `OPCUA.STATION[n].Interlock.PI.Req` — `require_cvok` 면 `CVOK`(컨베이어 준비)도.
    StationReq {
        station: u16,
        #[serde(default = "yes")]
        require_cvok: bool,
    },
    /// GRM `PI.ItemExist` (스테이션에 화물 있음) — `require_cvok` 면 `CVOK` 도.
    StationItem {
        station: u16,
        #[serde(default = "yes")]
        require_cvok: bool,
    },
    /// 대상(셀·스테이션)에 재고가 있고 그 품목이 **아직 측정되지 않았다**(비드 프로파일 없음) — 측정 먼저.
    Unmeasured { target: Target },
    /// 셀 재고가 `min` 개 이상(품목 지정 시 그 품목만).
    CellStock {
        cell: u16,
        #[serde(default)]
        item: Option<u32>,
        #[serde(default = "one")]
        min: u32,
    },
    /// 파생 규칙용 — 조건은 대상의 준비 상태(스테이션 게이트)가 본다. `require_measured` 면 출발 품목에 비드
    /// 프로파일이 있어야(측정 먼저 정책이 먼저 돈다).
    Ready {
        #[serde(default)]
        require_measured: bool,
    },
}

fn one() -> u32 {
    1
}

/// 셀 자동 선택 — 필터와 순서.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellPick {
    pub section: Option<u16>,
    pub row_min: Option<u16>,
    pub row_max: Option<u16>,
    pub col_min: Option<u16>,
    pub col_max: Option<u16>,
    /// 출발: `oldest`(재고가 가장 오래된 셀) | `nearest`(로봇에 가장 가까운). 도착: `nearest` 만 본다.
    pub order: String,
    /// 도착: 같은 품목이 쌓인 셀 먼저(아니면 빈 셀 먼저).
    pub same_item_first: bool,
    /// 도착: 이 대상(보통 출고 스테이션)에 **가까운 셀 먼저**. 없으면 로봇에 가까운 순.
    pub near: Option<u16>,
    /// 도착: 같은 품목이 이미 있는 셀만(빈 셀 제외) — 빈 시간 정리(같은 품목 모으기).
    pub same_item_only: bool,
    /// 도착: 빈 셀만 — 멀티 피킹 2개는 빈 셀에 내리고 빈 시간 정리(Split)로 max 스택을 만든다.
    pub empty_only: bool,
}

/// 만들 것.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// PICK → DROP 짝(이송 지시 하나). `from_auto`/`to_auto` 가 있으면 그 셀은 자동으로 고른다(고정 대상은 무시).
    Transfer {
        from: Target,
        to: Target,
        #[serde(default)]
        item: Option<u32>,
        #[serde(default = "one_u8")]
        count: u8,
        #[serde(default)]
        from_auto: Option<CellPick>,
        #[serde(default)]
        to_auto: Option<CellPick>,
        /// 도착이 팔렛 스테이션 — 다음 슬롯을 compose 가 고른다(자리 없으면 후보 아님).
        #[serde(default)]
        pallet_auto: bool,
        /// 멀티 피킹 합치기 — 출발 스테이션의 타이어를 도착 스테이션 타이어 **위(2단)** 에 내린다(PLC DoublePicking:
        /// 두 스텝 모두 LiftUpPartial). 도착은 PICK 스테이션이라 DROP 준비 대신 PICK 준비(화물 있음)를 본다.
        #[serde(default)]
        merge: bool,
    },
    Move {
        to: Target,
    },
    Measure {
        target: Target,
        #[serde(default)]
        item: Option<u32>,
    },
}

fn one_u8() -> u8 {
    1
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
    /// 보낼 수 있는 로봇(비면 전부).
    #[serde(default)]
    pub robots: Vec<u8>,
    /// 기본 우선순위(점수).
    #[serde(default)]
    pub priority: f32,
    /// `Manual` 규칙의 남은 요청 수(저장은 `taskgen_manual`).
    #[serde(default)]
    pub manual_requests: u32,
    /// 켜고 끌 수 있는 생성 조건.
    #[serde(default)]
    pub cond: Conditions,
    /// 출처 — 저장된 규칙은 늘 `user`, 정책 · 요청 규칙은 판정마다 파생된다(저장 안 함).
    #[serde(default)]
    pub origin: RuleOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// 이 수요가 겨냥한 스테이션(표시 · KPI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOrigin {
    #[default]
    User,
    Policy,
    Request,
}

/// 규칙마다 켜고 끄는 생성 조건 — 기본은 모두 켜짐. 끈 항목은 판단에서 빠지고 판단 기준 목록에 "무시" 로 남는다.
/// 위치 등록(출발·도착의 X)은 끌 수 없다 — 위치 없이는 명령을 만들 수 없다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Conditions {
    /// 품목을 콘솔이 알아야(규칙이 정하거나 출발 재고가 안다).
    pub item_known: bool,
    /// 출발 재고 ≥ 수량.
    pub source_stock: bool,
    /// 도착 남은 칸(품목 StackMax) ≥ 수량.
    pub dest_room: bool,
    /// 출발·도착의 Use(사용)가 켜져 있어야.
    pub target_use: bool,
}

impl Default for Conditions {
    fn default() -> Self {
        Conditions { item_known: true, source_stock: true, dest_room: true, target_use: true }
    }
}

/// 상황별 가중(규칙 설정과 함께 저장). 대기 가점 · 거리 감점 같은 전역 값은 파라미터(`params.rs`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    /// 스테이션(또는 셀) id → 가중(출발·도착 어느 쪽이든 그 대상이 들어가면 더한다).
    pub target: BTreeMap<u16, f32>,
    /// 품목 코드 → 가중(특정 규격 우선).
    pub item: BTreeMap<u32, f32>,
    /// 로봇 id → 가중(선호 호기).
    pub robot: BTreeMap<u8, f32>,
}

/// 전역 점수 손잡이(파라미터).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Knobs {
    pub age_per_min: f32,
    pub distance_per_m: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GenConfig {
    /// 저장할 때마다 +1.
    pub version: u32,
    /// 전체 자동 생성 — 기본 꺼짐(안전).
    pub auto: bool,
    pub weights: Weights,
    pub rules: Vec<Rule>,
    /// 모든 셀 · 스테이션에 공통으로 도는 정책(요청 목록 포함).
    pub policy: policy::Policy,
}

/// 스테이션 인터록 입력(GRM `LGR_Interface_CV_PI`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StationPi {
    pub cvok: bool,
    pub req: bool,
    pub item_exist: bool,
}

/// 규칙 조건을 판단할 세상(스냅샷).
#[derive(Clone, Debug, Default)]
pub struct World {
    pub stations: BTreeMap<u16, StationPi>,
    /// 셀·스테이션 id → (품목, 개수) — **미래** 재고(진행 중 · 짝 초안 · 예정 큐까지 접은 값).
    pub stock: BTreeMap<u16, (u32, u32)>,
    /// 셀 id → 남은 칸(StackMax − 미래 재고). 없으면 제한 없음.
    pub room: BTreeMap<u16, u32>,
    /// 셀·스테이션 id → Use(사용) 플래그.
    pub usable: BTreeMap<u16, bool>,
    /// 아직 측정되지 않은 품목 코드(비드 프로파일 없음).
    pub unmeasured: std::collections::BTreeSet<u32>,
    /// 대상 id → 준비 상태(스테이션 게이트).
    pub ready: BTreeMap<u16, ready::TargetReady>,
    pub profiles: BTreeMap<u16, profile::StationProfile>,
    pub items: HashMap<u32, ready::ItemFacts>,
    /// 멀티 피킹 최대 개수(정책, 0 = 2).
    pub multi_pick_max: u32,
}

#[cfg(test)]
pub fn fires(rule: &Rule, w: &World) -> bool {
    evaluate(rule, w).0
}

/// 스테이션 대상의 준비 한 줄 — 스테이션 작업은 어느 규칙이든 `CVOK & Req`(+ 화물 · 인계 · 예약)가 있어야 만든다.
fn station_gate(w: &World, t: &Target, op: &str) -> Option<Term> {
    if t.kind != "station" {
        return None;
    }
    let label = format!("STATION {} {}", t.id, op.to_uppercase());
    Some(match w.ready.get(&t.id) {
        None => Term { label, value: "준비 상태 모름 (등록 안 됨)".into(), ok: false },
        Some(r) => {
            let x = match op {
                "pick" => &r.pick,
                "drop" => &r.drop,
                _ => &r.measure,
            };
            let value = match (&x.why, x.ok) {
                (Some(w), true) => format!("가능 — {w}"),
                (None, true) => "가능".into(),
                (Some(w), false) => w.clone(),
                (None, false) => "불가".into(),
            };
            Term { label, value, ok: x.ok }
        }
    })
}

/// 멀티 피킹 합치기가 되나 — 두 스테이션의 품목을 콘솔이 알고 같으며, 출발 1개를 도착 스택(1개 이상) 위에 얹어도
/// 최대 개수(`multi_pick_max`)와 품목 StackMax(0 = 제한 없음) 안이다. 되면 품목 코드.
pub fn merge_check(w: &World, from: &Target, to: &Target) -> Result<u32, String> {
    let (si, sn) = w.stock.get(&from.id).copied().unwrap_or((0, 0));
    let (ti, tn) = w.stock.get(&to.id).copied().unwrap_or((0, 0));
    if si == 0 || sn == 0 {
        return Err(format!("{} 품목을 콘솔이 모름", from.id));
    }
    if ti == 0 || tn == 0 {
        return Err(format!("{} 품목을 콘솔이 모름", to.id));
    }
    if si != ti {
        return Err(format!("품목 다름 ({si} ≠ {ti})"));
    }
    if sn != 1 {
        return Err(format!("출발 재고 {sn} — 1개일 때만 옮겨 얹는다"));
    }
    let max = if w.multi_pick_max == 0 { 2 } else { w.multi_pick_max };
    if tn + 1 > max {
        return Err(format!("도착 {tn}개 — 얹으면 {} > 최대 {max}개", tn + 1));
    }
    if let Some(sm) = w.items.get(&si).map(|f| f.stack_max).filter(|m| *m > 0 && tn + 1 > *m) {
        return Err(format!("품목 {si} StackMax {sm} — {}단 불가", tn + 1));
    }
    Ok(si)
}

/// 규칙 판정 한 번 — 조건 항목 목록과 전체 충족(= 모든 항목 충족). `fires` 와 판단 기준 화면이 같은 결과를 쓴다.
pub fn evaluate(rule: &Rule, w: &World) -> (bool, Vec<Term>) {
    let mut terms = rule_terms(rule, w);
    match &rule.action {
        Action::Transfer { from, to, from_auto, to_auto, merge, .. } => {
            if from_auto.is_none() {
                terms.extend(station_gate(w, from, "pick"));
            }
            // 멀티 피킹 합치기는 PICK 스테이션의 타이어 위에 내린다 — 도착도 PICK 준비(화물 있음)를 본다.
            if to_auto.is_none() {
                terms.extend(station_gate(w, to, if *merge { "pick" } else { "drop" }));
            }
        }
        Action::Measure { target, .. } => terms.extend(station_gate(w, target, "measure")),
        Action::Move { .. } => {}
    }
    (rule.enabled && terms.iter().all(|t| t.ok), terms)
}

/// 출발이 준비됐나 — Use · 재고 · 품목(규칙에서 켠 것만). 안 됐으면 사유.
pub fn source_ready(w: &World, t: &Target, count: u8, want: Option<u32>, c: &Conditions) -> Option<String> {
    if t.kind != "cell" && t.kind != "station" {
        return None;
    }
    if let Some(why) = use_off(w, t, c) {
        return Some(why);
    }
    let (code, mut n) = w.stock.get(&t.id).copied().unwrap_or((0, 0));
    // 스테이션 화물은 GRM `ItemExist` 가 사실이다 — 콘솔 재고(트래킹이 옮긴 코드)가 비어 있어도 한 개는 있다.
    if t.kind == "station" && w.stations.get(&t.id).is_some_and(|p| p.item_exist) {
        n = n.max(1);
    }
    let need = count.max(1) as u32;
    if c.source_stock && n < need {
        return Some(format!("재고 {n} < 필요 {need}"));
    }
    // 품목은 규칙이 정하거나 콘솔 재고가 알아야 한다 — 0 으로 보내면 PLC 가 INVALID_ITEM_CODE 로 거부한다.
    if c.item_known && want.or(Some(code).filter(|x| *x != 0)).is_none() {
        return Some("품목을 콘솔이 모름".into());
    }
    None
}

/// 도착이 준비됐나 — Use · 남은 칸(규칙에서 켠 것만). 안 됐으면 사유.
pub fn dest_ready(w: &World, t: &Target, count: u8, c: &Conditions) -> Option<String> {
    if t.kind != "cell" && t.kind != "station" {
        return None;
    }
    if let Some(why) = use_off(w, t, c) {
        return Some(why);
    }
    let need = count.max(1) as u32;
    match w.room.get(&t.id) {
        Some(r) if c.dest_room && *r < need => Some(format!("남은 칸 {r} < 필요 {need}")),
        _ => None,
    }
}

/// Use(사용)가 꺼진 대상이면 사유 — 조건을 끄면 무시한다.
fn use_off(w: &World, t: &Target, c: &Conditions) -> Option<String> {
    (c.target_use && w.usable.get(&t.id) == Some(&false)).then(|| "Use 꺼짐".to_string())
}

/// 기본 규칙 한 벌 — 현장에서 바로 고쳐 쓰라고 심는 시작점(운전자 규칙 다섯 가지).
///
/// 1. 미측정 규격은 **측정 먼저** — 입고 스테이션에 올라온 화물의 품목에 비드 프로파일이 없으면 MEASURE.
/// 2. **출고 우선** — 출고 스테이션이 요청하면 셀에서 꺼내 내보낸다(가중치로도 올린다).
/// 3. 영역이 겹치지 않는 명령 우선 — 엔진이 늘 그렇게 고른다(`select`). 규칙이 아니라 동작이다.
/// 4. **회피가 최우선** — 막힌 로봇의 자리를 비켜 주는 후보에 `gen_avoid_bonus` 를 얹는다(파라미터).
/// 5. 적재는 **출고에 가까운 셀 먼저** — 입고에서 셀로 넣을 때 도착 셀을 출고 기준으로 고른다.
pub fn default_rules(out_station: u16, in_stations: &[u16]) -> GenConfig {
    let station = |id: u16| Target { kind: "station".into(), id };
    let auto_cell = || Target { kind: "cell".into(), id: 0 };
    let mut rules = Vec::new();
    for (i, &inb) in in_stations.iter().enumerate() {
        let n = if in_stations.len() > 1 { format!(" {}", i + 1) } else { String::new() };
        rules.push(Rule {
            id: format!("measure-first-{inb}"),
            name: format!("측정 먼저 — 입고{n} {inb}"),
            enabled: true,
            trigger: Trigger::Unmeasured { target: station(inb) },
            action: Action::Measure { target: station(inb), item: None },
            robots: vec![],
            priority: 80.0,
            manual_requests: 0,
            cond: Conditions::default(),
            origin: RuleOrigin::User,
            request_id: None,
            station: None,
        });
        rules.push(Rule {
            id: format!("store-near-out-{inb}"),
            name: format!("적재 — 입고{n} {inb} → 출고에 가까운 셀"),
            enabled: true,
            trigger: Trigger::StationReq { station: inb, require_cvok: true },
            action: Action::Transfer {
                from: station(inb),
                to: auto_cell(),
                item: None,
                count: 1,
                from_auto: None,
                to_auto: Some(CellPick { order: "nearest".into(), near: Some(out_station), ..Default::default() }),
                pallet_auto: false,
                merge: false,
            },
            robots: vec![],
            priority: 40.0,
            manual_requests: 0,
            cond: Conditions::default(),
            origin: RuleOrigin::User,
            request_id: None,
            station: None,
        });
    }
    rules.push(Rule {
        id: format!("ship-{out_station}"),
        name: format!("출고 우선 — 셀 → 출고 {out_station}"),
        enabled: true,
        trigger: Trigger::StationReq { station: out_station, require_cvok: true },
        action: Action::Transfer {
            from: auto_cell(),
            to: station(out_station),
            item: None,
            count: 1,
            from_auto: Some(CellPick { order: "oldest".into(), ..Default::default() }),
            to_auto: None,
            pallet_auto: false,
            merge: false,
        },
        robots: vec![],
        priority: 60.0,
        manual_requests: 0,
        cond: Conditions::default(),
        origin: RuleOrigin::User,
        request_id: None,
        station: None,
    });
    let mut weights = Weights::default();
    weights.target.insert(out_station, 20.0);
    GenConfig { version: 0, auto: false, weights, rules, policy: policy::Policy::default() }
}

/// 판단 기준 한 줄 — 무엇을 보는지(`label`), 지금 값(`value`), 그 값이 조건을 만족하는가(`ok`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Term {
    pub label: String,
    pub value: String,
    pub ok: bool,
}

/// 규칙 하나가 보는 조건 전부 — `fires` 가 보는 것과 같은 항목이다(조건 + 고정 셀의 재고·칸).
pub fn rule_terms(rule: &Rule, w: &World) -> Vec<Term> {
    let t = |label: String, value: String, ok: bool| Term { label, value, ok };
    let onoff = |v: bool| if v { "1".to_string() } else { "0".to_string() };
    let pi = |s: &u16| w.stations.get(s).copied().unwrap_or_default();
    let known = |s: &u16| w.stations.contains_key(s);
    let mut v = Vec::new();
    match &rule.trigger {
        Trigger::Manual => v.push(t("요청 대기".into(), format!("{} 건", rule.manual_requests), rule.manual_requests > 0)),
        Trigger::StationReq { station, require_cvok } | Trigger::StationItem { station, require_cvok } => {
            let p = pi(station);
            let (name, bit) = match &rule.trigger {
                Trigger::StationItem { .. } => ("ItemExist", p.item_exist),
                _ => ("Req", p.req),
            };
            if known(station) {
                v.push(t(format!("STATION {station} {name}"), onoff(bit), bit));
                v.push(if *require_cvok { t(format!("STATION {station} CVOK"), onoff(p.cvok), p.cvok) } else { t(format!("STATION {station} CVOK"), "무시".into(), true) });
            } else {
                v.push(t(format!("STATION {station}"), "GRM 에서 못 읽음".into(), false));
            }
        }
        Trigger::Unmeasured { target } => {
            let (code, n) = w.stock.get(&target.id).copied().unwrap_or((0, 0));
            let label = format!("{} {} 미측정 품목", target.kind, target.id);
            v.push(match (n, code) {
                (0, _) => t(label, "재고 없음".into(), false),
                (_, 0) => t(label, "품목을 모름".into(), false),
                (_, c) if w.unmeasured.contains(&c) => t(label, format!("품목 {c} — 비드 프로파일 없음"), true),
                (_, c) => t(label, format!("품목 {c} — 측정 있음"), false),
            });
        }
        Trigger::CellStock { cell, item, min } => {
            let (code, n) = w.stock.get(cell).copied().unwrap_or((0, 0));
            v.push(t(format!("셀 {cell} 재고"), format!("{n} 개 / 조건 ≥ {min}"), n >= *min));
            if let Some(want) = item {
                v.push(t(format!("셀 {cell} 품목"), format!("{code} / 조건 {want}"), code == *want));
            }
        }
        Trigger::Ready { require_measured } => {
            if *require_measured && let Action::Transfer { from, from_auto: None, item, .. } = &rule.action {
                let code = item.or_else(|| w.stock.get(&from.id).map(|(c, _)| *c).filter(|c| *c != 0));
                v.push(match code {
                    None => t("품목 측정".into(), "품목을 모름".into(), false),
                    Some(c) if w.unmeasured.contains(&c) => t("품목 측정".into(), format!("{c} — 비드 프로파일 없음 (측정 먼저)"), false),
                    Some(c) => t("품목 측정".into(), format!("{c} — 측정 있음"), true),
                });
            }
        }
    }
    match &rule.action {
        Action::Transfer { from, to, count, from_auto, to_auto, pallet_auto, item, merge } => {
            // 품목 — 규칙이 정하거나 출발의 콘솔 재고가 알아야 한다(스테이션도 같다).
            let src_code = from_auto.is_none().then(|| w.stock.get(&from.id).map(|(c, _)| *c).unwrap_or(0)).filter(|c| *c != 0);
            match (item, src_code, from_auto.is_some()) {
                (Some(i), _, _) => v.push(t("품목".into(), format!("{i} (규칙)"), true)),
                (None, Some(c), _) => v.push(t("품목".into(), format!("{c} (출발 재고)"), true)),
                (None, None, true) => v.push(t("품목".into(), "출발 셀을 고를 때 확정".into(), true)),
                (None, None, false) if !rule.cond.item_known => v.push(t("품목".into(), "무시 (조건 끔)".into(), true)),
                (None, None, false) => v.push(t("품목".into(), "콘솔이 모름".into(), false)),
            }
            match from_auto {
                Some(p) => v.push(t("출발 셀".into(), format!("자동 선택({})", if p.order.is_empty() { "oldest" } else { &p.order }), true)),
                None => {
                    let why = source_ready(w, from, *count, *item, &rule.cond);
                    let have = w.stock.get(&from.id).map(|(_, n)| *n).unwrap_or(0);
                    let ok = if rule.cond.source_stock { format!("재고 {have} / 필요 {count}") } else { format!("재고 {have} (수량 조건 끔)") };
                    v.push(t(format!("출발 {} {} 준비", from.kind, from.id), why.clone().unwrap_or(ok), why.is_none()));
                }
            }
            match to_auto {
                Some(_) => v.push(t("도착 셀".into(), "자동 선택(가까운 순)".into(), true)),
                None => {
                    let why = dest_ready(w, to, *count, &rule.cond);
                    let room = match w.room.get(&to.id) {
                        Some(r) if rule.cond.dest_room => format!("남은 칸 {r} / 필요 {count}"),
                        Some(r) => format!("남은 칸 {r} (칸 조건 끔)"),
                        None => "칸 제한 없음".into(),
                    };
                    v.push(t(format!("도착 {} {} 준비", to.kind, to.id), why.clone().unwrap_or(room), why.is_none()));
                }
            }
            if *pallet_auto {
                v.push(t("팔렛 다음 슬롯".into(), "작성 때 확인".into(), true));
            }
            if *merge {
                let label = format!("멀티 피킹 {} → {} 위", from.id, to.id);
                v.push(match merge_check(w, from, to) {
                    Ok(code) => t(label, format!("품목 {code} 같음 · 도착 {}개 위", w.stock.get(&to.id).map(|(_, n)| *n).unwrap_or(0)), true),
                    Err(why) => t(label, why, false),
                });
            }
        }
        Action::Move { to } => v.push(t("목표".into(), format!("{} {}", to.kind, to.id), true)),
        Action::Measure { target, .. } => v.push(t("대상".into(), format!("{} {}", target.kind, target.id), true)),
    }
    v
}

/// 판단 기준을 한 줄로(툴팁·좁은 화면).
#[cfg(test)]
pub fn trigger_inputs(rule: &Rule, w: &World) -> String {
    rule_terms(rule, w).iter().map(|x| format!("{} {}{}", x.label, x.value, if x.ok { "" } else { " ✗" })).collect::<Vec<_>>().join(" · ")
}

/// 스테이션 인터록의 지금 값(모니터) — 규칙이 겨냥하지 않는 스테이션도 보인다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StationRow {
    pub id: u16,
    pub cvok: bool,
    pub req: bool,
    pub item_exist: bool,
    /// 켠 규칙이 이 스테이션을 본다.
    pub watched: bool,
}

/// 셀 재고의 지금 값(모니터) — 규칙이 겨냥한 셀과 재고가 있는 셀.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellRow {
    pub id: u16,
    pub item_code: u32,
    pub count: u32,
    pub room: Option<u32>,
    pub watched: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RobotRow {
    pub id: u8,
    pub name: String,
    pub x: Option<f32>,
    /// 이 로봇에 생성 작업이 이미 있다(예정·진행 중).
    pub busy: bool,
}

/// 엔진이 판단에 쓰는 입력 한 벌 — 규칙과 무관하게 지금 값을 보여 준다.
#[derive(Clone, Debug, Default, Serialize)]
pub struct EngineInputs {
    pub stations: Vec<StationRow>,
    pub cells: Vec<CellRow>,
    pub robots: Vec<RobotRow>,
    /// 영역 간격(0 = 검사 안 함) · 로봇당 큐 깊이 · 판정 주기.
    pub separation_mm: f32,
    pub queue_depth: u32,
    pub tick_ms: u64,
    pub updated_at: String,
}

/// 규칙 하나의 지금 상태(모니터).
#[derive(Clone, Debug, Serialize)]
pub struct RuleStatus {
    pub rule_id: String,
    pub fires: bool,
    pub inputs: String,
    /// 조건 항목별 지금 값과 충족 여부.
    pub terms: Vec<Term>,
    /// `off` 규칙 꺼짐 · `idle` 조건 거짓 · `busy` 이 규칙 작업이 아직 돈다 · `queued` 예정에 있다 ·
    /// `skipped` 후보 못 됨 · `waiting` 후보지만 대기 · `ready` 지금 만들 수 있다(자동이 꺼져 있으면 만들지 않는다).
    pub state: String,
    pub reason: Option<String>,
    /// 조건이 참이 된 뒤 지난 시간(분).
    pub age_min: f32,
    /// 엔진이 도는 동안의 누적.
    pub generated: u64,
    pub last_generated_at: Option<String>,
    /// 사용자 규칙 · 정책 수요 · 요청.
    pub origin: RuleOrigin,
    pub name: String,
    pub request_id: Option<String>,
    pub station: Option<u16>,
}

/// 자동 선택에 쓰는 셀 하나.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellView {
    pub id: u16,
    pub x: f32,
    pub section: u16,
    pub row: u16,
    pub col: u16,
    pub use_: bool,
    /// 예상 재고.
    pub item: u32,
    pub count: u32,
    /// StackMax − 예상 재고(품목을 모르면 `None` = 제한 없음).
    pub room: Option<u32>,
    /// 재고 갱신 시각(오래된 재고 먼저).
    pub updated_at: String,
}

fn in_filter(c: &CellView, f: &CellPick) -> bool {
    c.use_
        && f.section.is_none_or(|s| s == c.section)
        && f.row_min.is_none_or(|r| c.row >= r)
        && f.row_max.is_none_or(|r| c.row <= r)
        && f.col_min.is_none_or(|v| c.col >= v)
        && f.col_max.is_none_or(|v| c.col <= v)
}

/// 출발 셀 — 품목(있으면 같은 품목) · 개수 충분 · 영역 밖(`free`) 중 `oldest`(재고 갱신이 가장 오래된) · `nearest`
/// · `fewest`(가장 적게 쌓인 — 빈 시간 정리).
pub fn choose_source(cells: &[CellView], f: &CellPick, item: Option<u32>, count: u32, robot_x: Option<f32>, free: &dyn Fn(f32) -> bool) -> Option<u16> {
    let mut v: Vec<&CellView> = cells.iter().filter(|c| in_filter(c, f) && c.count >= count.max(1) && c.item != 0 && item.is_none_or(|i| i == c.item) && free(c.x)).collect();
    let dist = |c: &CellView| robot_x.map(|x| (c.x - x).abs()).unwrap_or(0.0);
    if f.order == "nearest" {
        v.sort_by(|a, b| dist(a).total_cmp(&dist(b)).then(a.id.cmp(&b.id)));
    } else if f.order == "fewest" {
        v.sort_by(|a, b| a.count.cmp(&b.count).then(dist(a).total_cmp(&dist(b))).then(a.id.cmp(&b.id)));
    } else {
        v.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
    }
    v.first().map(|c| c.id)
}

/// 도착 셀 — 칸 충분(StackMax) · 비었거나 같은 품목 · 영역 밖 중 (`same_item_first` 면 같은 품목 먼저) 가까운 순.
/// `near_x` 가 있으면 그 자리(보통 출고 스테이션)에 가까운 순, 없으면 로봇에 가까운 순.
#[allow(clippy::too_many_arguments)]
pub fn choose_dest(cells: &[CellView], f: &CellPick, item: u32, count: u32, robot_x: Option<f32>, near_x: Option<f32>, exclude: Option<u16>, free: &dyn Fn(f32) -> bool) -> Option<u16> {
    let ok = |c: &&CellView| {
        in_filter(c, f)
            && Some(c.id) != exclude
            && (c.count == 0 || (item != 0 && c.item == item))
            && !(f.same_item_only && c.count == 0)
            && !(f.empty_only && c.count != 0)
            && c.room.is_none_or(|r| r >= count.max(1))
            && free(c.x)
    };
    let mut v: Vec<&CellView> = cells.iter().filter(ok).collect();
    let from = near_x.or(robot_x);
    let dist = |c: &CellView| from.map(|x| (c.x - x).abs()).unwrap_or(0.0);
    v.sort_by(|a, b| {
        let same = |c: &CellView| if f.same_item_first { (c.count == 0) as u8 } else { (c.count != 0) as u8 };
        // 같은 품목만(빈 시간 정리)이면 가장 많이 쌓인 셀부터 채운다 — Split > Merge 로 풀스택을 만든다.
        let fuller = if f.same_item_only { b.count.cmp(&a.count) } else { std::cmp::Ordering::Equal };
        same(a).cmp(&same(b)).then(fuller).then(dist(a).total_cmp(&dist(b))).then(a.id.cmp(&b.id))
    });
    v.first().map(|c| c.id)
}

/// 후보 하나(규칙 × 로봇).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Candidate {
    pub rule_id: String,
    pub rule_name: String,
    pub robot: u8,
    /// 고른(또는 고정) 첫 대상과 짝 DROP 대상.
    pub first: Target,
    pub second: Option<Target>,
    /// 확정된 품목(PICK/DROP 짝은 늘 있다).
    pub item: Option<u32>,
    /// 첫 목표 X · (짝이면) DROP 목표 X.
    pub target_x: f32,
    pub drop_x: Option<f32>,
    pub area: Interval,
    pub score: f32,
    /// 점수 근거 — 화면의 "왜 이 순서" 툴팁.
    pub breakdown: Vec<(String, f32)>,
    /// 조건이 처음 참이 된 순서(같은 점수의 결정적 순서).
    pub order: u64,
    pub age_min: f32,
    /// 이 후보가 쓰는 스테이션 — 한 판정에서 한 스테이션은 한 후보만(인터록 인계가 Task 하나 단위).
    #[serde(skip)]
    pub stations: Vec<u16>,
    /// 파생 규칙의 요청 id — 한 판정에서 요청의 남은 수보다 많이 만들지 않는다.
    #[serde(skip)]
    pub request_id: Option<String>,
}

/// 로봇 하나의 지금 모습(영역 판단용).
#[derive(Clone, Debug, Default)]
pub struct RobotView {
    pub id: u8,
    pub name: String,
    pub current_x: Option<f32>,
    /// 발행된 진행 중 Task 목표 X(원장 순서).
    pub pending: Vec<f32>,
    /// PLC 가 받은 명령이 있다.
    pub committed: bool,
    /// 생성됐지만 아직 발행 안 된(예정) 영역.
    pub scheduled: Option<Interval>,
    /// 이 로봇에 예정·진행 중 생성 작업이 있어 새로 만들지 않는다.
    pub busy: bool,
    /// 추가 여유(mm, 파라미터).
    pub margin: f32,
}

fn targets_of(a: &Action) -> Vec<&Target> {
    match a {
        Action::Transfer { from, to, .. } => vec![from, to],
        Action::Move { to } => vec![to],
        Action::Measure { target, .. } => vec![target],
    }
}

pub fn item_of(a: &Action) -> Option<u32> {
    match a {
        Action::Transfer { item, .. } | Action::Measure { item, .. } => *item,
        Action::Move { .. } => None,
    }
}

/// 점수와 근거. `targets` = 실제로 고른 대상(자동 선택 뒤).
pub fn score(rule: &Rule, targets: &[&Target], robot: u8, w: &Weights, k: Knobs, age_min: f32, distance_m: f32) -> (f32, Vec<(String, f32)>) {
    let mut b: Vec<(String, f32)> = vec![("Rule.Priority".into(), rule.priority)];
    let fixed = targets_of(&rule.action);
    let all: Vec<&Target> = if targets.is_empty() { fixed } else { targets.to_vec() };
    for t in all {
        if let Some(v) = w.target.get(&t.id) {
            b.push((format!("Target {}", t.id), *v));
        }
    }
    if let Some(i) = item_of(&rule.action)
        && let Some(v) = w.item.get(&i)
    {
        b.push((format!("Item {i}"), *v));
    }
    if let Some(v) = w.robot.get(&robot) {
        b.push((format!("Robot {robot}"), *v));
    }
    if k.age_per_min != 0.0 && age_min > 0.0 {
        b.push(("Age".into(), k.age_per_min * age_min));
    }
    if k.distance_per_m != 0.0 && distance_m > 0.0 {
        b.push(("Distance".into(), -k.distance_per_m * distance_m));
    }
    (b.iter().map(|(_, v)| v).sum(), b)
}

/// 점수 내림차순, 같으면 먼저 생긴 순.
pub fn rank(c: &mut [Candidate]) {
    c.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.order.cmp(&b.order)).then(a.robot.cmp(&b.robot)));
}

/// 후보 영역.
pub fn candidate_area(r: &RobotView, target_x: f32, drop_x: Option<f32>) -> Interval {
    task_area(r.pending.last().copied(), r.current_x, target_x, drop_x)
}

/// 다른 로봇 `o` 가 잡은 영역(발행 · 예정 · 이번에 고른 것).
pub fn reserved(o: &RobotView, picked: Option<Interval>) -> Option<Reservation> {
    let mut r = reservation(o.id, &o.name, o.current_x, &o.pending, o.committed, None);
    for extra in [o.scheduled, picked].into_iter().flatten() {
        r = Some(match r {
            Some(mut x) => {
                x.area = x.area.with(extra.lo).with(extra.hi);
                x.idle = false;
                x
            }
            None => Reservation { robot: o.id, name: o.name.clone(), area: extra, idle: false, holds_pair: false, margin: 0.0 },
        });
    }
    r.map(|mut x| {
        x.margin = o.margin;
        x
    })
}

/// 판정 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Selection {
    /// 이번에 "예정" 으로 만들 후보(로봇마다 최대 하나, 규칙마다 하나).
    pub generate: Vec<Candidate>,
    /// 못 만든 후보와 사유.
    pub waiting: Vec<(Candidate, String)>,
    /// 후보조차 못 된 규칙과 사유(자동 셀 없음 · 팔렛 자리 없음 · 거리 초과).
    pub skipped: Vec<(String, String)>,
    /// 영역에 막힌 후보와 막은 로봇 — 회피 우선 판정이 쓴다(화면에는 `waiting` 사유로 나간다).
    #[serde(skip)]
    pub blocked: Vec<(Candidate, u8)>,
}

/// 영역을 지키며 고른다 — 순위대로, 로봇·규칙마다 하나, 다른 로봇 영역(+ 이번에 고른 것)과 간격 안이면 넘긴다.
/// 막힌 로봇의 사유: 상대가 비켜 가는 후보를 이번에 만들면 "상대 명령 수령 대기", 아니면 "비켜 줄 작업 없음 — 대기".
/// **회피가 먼저다** — 막힌 로봇이 있으면 막은 로봇의 후보 중 그 자리를 비켜 주는 것(도착점이 막힌 후보
/// 영역에서 간격 밖)에 `avoid_bonus`(파라미터 `gen_avoid_bonus`)를 얹어 다시 고른다. 얹지 않으면 점수가
/// 높은 다른 일이 먼저 나가 교착이 길어진다.
/// `caps` = 요청 id → 이번에 더 만들 수 있는 수(남은 수).
pub fn select_with(mut cands: Vec<Candidate>, robots: &[RobotView], sep: f32, avoid_bonus: f32, caps: &BTreeMap<String, u32>) -> Selection {
    if avoid_bonus > 0.0 {
        let mut boosted = false;
        for (blocked, by) in select_once(cands.clone(), robots, sep, caps).blocked {
            for c in cands.iter_mut().filter(|c| c.robot == by) {
                let frees = Interval::point(c.drop_x.unwrap_or(c.target_x)).gap(blocked.area) >= sep;
                if frees && !c.breakdown.iter().any(|(k, _)| k == "Avoidance") {
                    c.score += avoid_bonus;
                    c.breakdown.push(("Avoidance".into(), avoid_bonus));
                    boosted = true;
                }
            }
        }
        if boosted {
            rank(&mut cands);
        }
    }
    select_once(cands, robots, sep, caps)
}

fn select_once(mut cands: Vec<Candidate>, robots: &[RobotView], sep: f32, caps: &BTreeMap<String, u32>) -> Selection {
    rank(&mut cands);
    let mut out = Selection::default();
    let mut picked: BTreeMap<u8, Interval> = BTreeMap::new();
    let mut used_rules: Vec<String> = Vec::new();
    let mut used_stations: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    let mut req_used: BTreeMap<String, u32> = BTreeMap::new();
    let mut blocked: Vec<(Candidate, u8)> = Vec::new();
    for c in cands {
        let Some(me) = robots.iter().find(|r| r.id == c.robot) else { continue };
        if me.busy {
            out.waiting.push((c, "이 로봇에 예정·진행 중 생성 작업 있음".into()));
            continue;
        }
        if picked.contains_key(&c.robot) {
            out.waiting.push((c, "이번 판정에서 이 로봇에 더 앞선 작업을 맡김".into()));
            continue;
        }
        if used_rules.contains(&c.rule_id) {
            out.waiting.push((c, "같은 규칙을 다른 로봇이 맡음".into()));
            continue;
        }
        if let Some(s) = c.stations.iter().find(|s| used_stations.contains(s)) {
            let why = format!("같은 스테이션 {s} 을 이번 판정에서 다른 작업이 맡음");
            out.waiting.push((c, why));
            continue;
        }
        if let Some(r) = &c.request_id
            && req_used.get(r).copied().unwrap_or(0) >= caps.get(r).copied().unwrap_or(u32::MAX)
        {
            let why = format!("요청 {r} 의 남은 수만큼 이미 맡김");
            out.waiting.push((c, why));
            continue;
        }
        let others: Vec<Reservation> = robots.iter().filter(|o| o.id != c.robot).filter_map(|o| reserved(o, picked.get(&o.id).copied())).collect();
        match blocker(c.area, &others, sep + me.margin) {
            Some(b) => {
                let why = format!("영역 겹침: {} X {} 사용 중 — 이 후보 X {} (간격 {:.0} mm)", b.name, b.area.label(), c.area.label(), sep + me.margin + b.margin);
                blocked.push((c.clone(), b.robot));
                out.waiting.push((c, why));
            }
            None => {
                picked.insert(c.robot, c.area);
                used_rules.push(c.rule_id.clone());
                used_stations.extend(c.stations.iter().copied());
                if let Some(r) = &c.request_id {
                    *req_used.entry(r.clone()).or_default() += 1;
                }
                out.generate.push(c);
            }
        }
    }
    // 막힌 후보의 사유를 다듬는다 — 상대가 이번에 비켜 가는(도착점이 이 후보 영역에서 간격 밖) 작업을 만들었나.
    for (c, by) in blocked.iter().cloned() {
        let yield_ = out.generate.iter().any(|g| g.robot == by && Interval::point(g.drop_x.unwrap_or(g.target_x)).gap(c.area) >= sep);
        let who = robots.iter().find(|r| r.id == by).map(|r| r.name.clone()).unwrap_or_default();
        let note = if yield_ {
            format!("{who} 비켜 가는 명령 생성 — PLC 수령(Accepted) 뒤 이 호기 생성")
        } else {
            format!("{who} 가 비켜 줄 작업 없음 — 대기 (자동 회피 MOVE 없음)")
        };
        if let Some(w) = out.waiting.iter_mut().find(|(w, _)| w.rule_id == c.rule_id && w.robot == c.robot) {
            w.1 = format!("{} · {note}", w.1);
        }
    }
    out.blocked = blocked;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEP: f32 = 2403.0;

    fn cell(id: u16) -> Target {
        Target { kind: "cell".into(), id }
    }
    fn transfer(from: u16, to: u16) -> Action {
        Action::Transfer { from: cell(from), to: cell(to), item: Some(2011), count: 1, from_auto: None, to_auto: None, pallet_auto: false, merge: false }
    }
    fn rule(id: &str, prio: f32, from: u16, to: u16) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            trigger: Trigger::Manual,
            action: transfer(from, to),
            robots: vec![],
            priority: prio,
            manual_requests: 1,
            cond: Conditions::default(),
            origin: RuleOrigin::User,
            request_id: None,
            station: None,
        }
    }
    fn cand(rule_id: &str, robot: u8, lo: f32, hi: f32, score: f32, order: u64) -> Candidate {
        Candidate {
            rule_id: rule_id.into(),
            rule_name: rule_id.into(),
            robot,
            first: cell(1),
            second: Some(cell(2)),
            item: Some(2011),
            target_x: lo,
            drop_x: Some(hi),
            area: Interval::span(lo, hi),
            score,
            breakdown: vec![],
            order,
            age_min: 0.0,
            stations: vec![],
            request_id: None,
        }
    }
    fn robot(id: u8, x: f32) -> RobotView {
        RobotView { id, name: format!("GR{id}"), current_x: Some(x), ..Default::default() }
    }

    #[test]
    fn triggers_room_and_cvok() {
        let mut w = World::default();
        w.stations.insert(2101, StationPi { cvok: false, req: true, item_exist: false });
        w.stock.insert(401, (2011, 3));
        w.room.insert(402, 0);
        let mut r = rule("a", 0.0, 401, 403);
        assert!(fires(&r, &w), "manual request + stock at the source");
        r.manual_requests = 0;
        assert!(!fires(&r, &w));
        // Req 만 있고 CVOK 없음 — 기본(require_cvok)이면 안 선다
        r.trigger = Trigger::StationReq { station: 2101, require_cvok: true };
        assert!(!fires(&r, &w));
        r.trigger = Trigger::StationReq { station: 2101, require_cvok: false };
        assert!(fires(&r, &w));
        w.stations.insert(2101, StationPi { cvok: true, req: true, item_exist: false });
        r.trigger = Trigger::StationReq { station: 2101, require_cvok: true };
        assert!(fires(&r, &w));
        r.trigger = Trigger::StationItem { station: 2101, require_cvok: true };
        assert!(!fires(&r, &w));
        r.trigger = Trigger::CellStock { cell: 401, item: Some(2011), min: 3 };
        assert!(fires(&r, &w));
        r.trigger = Trigger::CellStock { cell: 401, item: Some(9999), min: 1 };
        assert!(!fires(&r, &w));
        // 도착 칸 없음 → 안 만든다, 자동 도착이면 고를 때 본다
        r.trigger = Trigger::Manual;
        r.manual_requests = 1;
        r.action = transfer(401, 402);
        assert!(!fires(&r, &w));
        if let Action::Transfer { to_auto, .. } = &mut r.action {
            *to_auto = Some(CellPick::default());
        }
        assert!(fires(&r, &w));
        r.enabled = false;
        assert!(!fires(&r, &w));
    }

    fn cv(id: u16, x: f32, item: u32, count: u32, room: Option<u32>, at: &str) -> CellView {
        CellView { id, x, section: 1, row: 1, col: id % 10, use_: true, item, count, room, updated_at: at.into() }
    }

    #[test]
    fn automatic_source_and_destination() {
        let cells = vec![
            cv(401, 1000.0, 2011, 3, Some(1), "2026-09-21T10:00:00"),
            cv(402, 5000.0, 2011, 2, Some(2), "2026-09-21T09:00:00"),
            cv(403, 3000.0, 0, 0, None, ""),
            cv(404, 2000.0, 7777, 1, Some(4), "2026-09-21T08:00:00"),
            cv(405, 9000.0, 0, 0, None, ""),
        ];
        let free = |_: f32| true;
        // 출발: 오래된 재고(같은 품목) — 402, 가까운 — 401
        let oldest = CellPick { order: "oldest".into(), ..Default::default() };
        assert_eq!(choose_source(&cells, &oldest, Some(2011), 1, Some(0.0), &free), Some(402));
        let nearest = CellPick { order: "nearest".into(), ..Default::default() };
        assert_eq!(choose_source(&cells, &nearest, Some(2011), 1, Some(0.0), &free), Some(401));
        assert_eq!(choose_source(&cells, &nearest, Some(2011), 3, Some(0.0), &free), Some(401), "only 401 has 3");
        // 다른 로봇 영역(X > 4000 막힘)
        let left = |x: f32| x < 4000.0;
        assert_eq!(choose_source(&cells, &oldest, Some(2011), 1, None, &left), Some(401));
        // 도착: 같은 품목 먼저 → 칸 있는 402(401 은 칸 1, 2개면 안 됨), 아니면 가까운 빈 셀
        let same = CellPick { same_item_first: true, ..Default::default() };
        assert_eq!(choose_dest(&cells, &same, 2011, 2, Some(0.0), None, None, &free), Some(402));
        let empty_first = CellPick::default();
        assert_eq!(choose_dest(&cells, &empty_first, 2011, 1, Some(0.0), None, None, &free), Some(403));
        // 출발 셀은 도착에서 뺀다 · 다른 품목 셀은 안 된다 · 영역
        assert_eq!(choose_dest(&cells, &same, 2011, 1, Some(0.0), None, Some(401), &left), Some(403));
        // 필터: 구역 2 셀 없음
        let sec2 = CellPick { section: Some(2), ..Default::default() };
        assert_eq!(choose_dest(&cells, &sec2, 2011, 1, None, None, None, &free), None);
    }

    #[test]
    fn scoring_uses_station_item_robot_age_distance() {
        let mut w = Weights::default();
        w.target.insert(2101, 10.0);
        w.item.insert(2011, 5.0);
        w.robot.insert(2, 1.0);
        let k = Knobs { age_per_min: 0.5, distance_per_m: 2.0 };
        let mut r = rule("a", 3.0, 401, 2101);
        r.action = Action::Transfer { from: cell(401), to: Target { kind: "station".into(), id: 2101 }, item: Some(2011), count: 1, from_auto: None, to_auto: None, pallet_auto: false, merge: false };
        let (s, b) = score(&r, &[], 2, &w, k, 4.0, 1.5);
        assert_eq!(s, 3.0 + 10.0 + 5.0 + 1.0 + 2.0 - 3.0);
        assert!(b.iter().any(|(k, _)| k == "Target 2101") && b.iter().any(|(k, v)| k == "Distance" && *v == -3.0));
        // 자동으로 고른 대상으로 가중을 본다
        let picked = Target { kind: "cell".into(), id: 2101 };
        let (s2, _) = score(&rule("b", 0.0, 1, 2), &[&cell(1), &picked], 1, &w, Knobs::default(), 0.0, 0.0);
        assert_eq!(s2, 10.0 + 5.0);
        // 같은 점수면 먼저 생긴 것
        let mut c = vec![cand("x", 1, 0.0, 1.0, 5.0, 2), cand("y", 1, 0.0, 1.0, 9.0, 3), cand("z", 1, 0.0, 1.0, 5.0, 1)];
        rank(&mut c);
        assert_eq!(c.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["y", "z", "x"]);
    }

    /// 두 로봇 — 떨어진 후보는 같이, 겹치는 후보는 하나만(점수 높은 것).
    #[test]
    fn zone_aware_selection_maximises_concurrency() {
        let robots = vec![robot(1, 2000.0), robot(2, 14000.0)];
        let s = select_with(vec![cand("a", 1, 2000.0, 5000.0, 5.0, 1), cand("b", 2, 10000.0, 14000.0, 4.0, 2)], &robots, SEP, 0.0, &BTreeMap::new());
        assert_eq!(s.generate.len(), 2);
        let s = select_with(vec![cand("b", 2, 9000.0, 14000.0, 9.0, 1), cand("a1", 1, 2000.0, 8000.0, 8.0, 2), cand("a2", 1, 2000.0, 5000.0, 1.0, 3)], &robots, SEP, 0.0, &BTreeMap::new());
        assert_eq!(s.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["b", "a2"]);
        assert!(s.waiting.iter().any(|(c, why)| c.rule_id == "a1" && why.contains("영역 겹침")));
        let s = select_with(vec![cand("r", 1, 2000.0, 3000.0, 5.0, 1), cand("r", 2, 12000.0, 14000.0, 5.0, 1)], &robots, SEP, 0.0, &BTreeMap::new());
        assert_eq!(s.generate.len(), 1);
        // 로봇 여유가 간격에 더해진다
        let mut wide = robots.clone();
        wide[1].margin = 3000.0;
        let s = select_with(vec![cand("b", 2, 10000.0, 14000.0, 9.0, 1), cand("a", 1, 2000.0, 5000.0, 1.0, 2)], &wide, SEP, 0.0, &BTreeMap::new());
        assert_eq!(s.generate.len(), 1, "5000..10000 gap < 2403 + 3000");
    }

    /// 회피 순서: GR1 이 GR2 자리가 필요 — GR2 가 비켜 가는 후보를 이번에 만들고 GR1 은 "수령 뒤" 로 기다린다.
    #[test]
    fn avoidance_is_sequenced_after_the_other_robot_accepts() {
        let mut robots = vec![robot(1, 2000.0), robot(2, 8000.0)];
        let need = cand("a", 1, 2000.0, 7000.0, 9.0, 1);
        let away = cand("b", 2, 8000.0, 13000.0, 1.0, 2);
        let s = select_with(vec![need.clone(), away.clone()], &robots, SEP, 0.0, &BTreeMap::new());
        assert_eq!(s.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        let why = &s.waiting.iter().find(|(c, _)| c.rule_id == "a").unwrap().1;
        assert!(why.contains("PLC 수령(Accepted) 뒤"), "{why}");
        robots[1].scheduled = Some(away.area);
        robots[1].busy = true;
        assert!(select_with(vec![need.clone()], &robots, SEP, 0.0, &BTreeMap::new()).generate.is_empty());
        robots[1].scheduled = None;
        robots[1].pending = vec![13000.0];
        assert!(select_with(vec![need.clone()], &robots, SEP, 0.0, &BTreeMap::new()).generate.is_empty());
        robots[1].committed = true;
        assert_eq!(select_with(vec![need.clone()], &robots, SEP, 0.0, &BTreeMap::new()).generate.len(), 1);
    }

    #[test]
    fn no_deadlock_no_overlap() {
        let robots = vec![robot(1, 4000.0), robot(2, 7000.0)];
        let s = select_with(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1)], &robots, SEP, 0.0, &BTreeMap::new());
        assert!(s.generate.is_empty());
        assert!(s.waiting[0].1.contains("비켜 줄 작업 없음"));
        let s = select_with(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1), cand("b", 2, 3500.0, 7000.0, 4.0, 2)], &robots, SEP, 0.0, &BTreeMap::new());
        assert!(s.generate.is_empty() && s.waiting.len() == 2);
    }

    #[test]
    fn config_round_trips_as_versioned_json() {
        let mut c = GenConfig { version: 3, auto: false, ..Default::default() };
        c.weights.target.insert(2101, 10.0);
        c.rules.push(Rule { trigger: Trigger::StationReq { station: 2101, require_cvok: true }, ..rule("st", 2.0, 401, 402) });
        c.rules.push(Rule { trigger: Trigger::CellStock { cell: 401, item: None, min: 2 }, action: Action::Measure { target: cell(401), item: None }, ..rule("m", 0.0, 0, 0) });
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("\"kind\":\"station_req\"") && j.contains("\"kind\":\"transfer\""), "{j}");
        let back: GenConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        // 모자란 필드는 기본값(enabled/require_cvok = true, count = 1, min = 1), 옛 가중치 필드는 무시
        let r: Rule =
            serde_json::from_str(r#"{"id":"x","name":"x","trigger":{"kind":"station_req","station":5},"action":{"kind":"transfer","from":{"kind":"cell","id":1},"to":{"kind":"cell","id":2}}}"#)
                .unwrap();
        assert!(r.enabled);
        assert_eq!(r.trigger, Trigger::StationReq { station: 5, require_cvok: true });
        let old: GenConfig = serde_json::from_str(r#"{"version":1,"weights":{"age_per_min":1.0,"target":{"7":1.0}}}"#).unwrap();
        assert_eq!(old.weights.target.get(&7), Some(&1.0));
    }

    #[test]
    fn trigger_inputs_say_the_live_values() {
        let mut w = World::default();
        w.stations.insert(2101, StationPi { cvok: true, req: false, item_exist: true });
        w.stock.insert(401, (2011, 3));
        w.room.insert(402, 1);
        fn shown(r: &Rule, w: &World) -> Vec<(String, String, bool)> {
            rule_terms(r, w).iter().map(|t| (t.label.clone(), t.value.clone(), t.ok)).collect()
        }

        let r = Rule { trigger: Trigger::StationReq { station: 2101, require_cvok: true }, ..rule("st", 0.0, 401, 402) };
        assert_eq!(
            shown(&r, &w),
            vec![
                ("STATION 2101 Req".into(), "0".into(), false),
                ("STATION 2101 CVOK".into(), "1".into(), true),
                ("품목".into(), "2011 (규칙)".into(), true),
                ("출발 cell 401 준비".into(), "재고 3 / 필요 1".into(), true),
                ("도착 cell 402 준비".into(), "남은 칸 1 / 필요 1".into(), true),
            ]
        );
        let s = trigger_inputs(&r, &w);
        assert!(s.contains("STATION 2101 Req 0 ✗") && !s.contains("CVOK 1 ✗"), "{s}");

        let m = Rule { trigger: Trigger::Manual, manual_requests: 2, ..rule("man", 0.0, 401, 402) };
        assert_eq!(shown(&m, &w)[0], ("요청 대기".into(), "2 건".into(), true));

        // 품목을 규칙이 정하지 않으면 출발의 콘솔 재고가 알아야 한다 — 스테이션도 같다.
        let no_item = Rule {
            action: Action::Transfer { from: Target { kind: "station".into(), id: 2101 }, to: cell(402), item: None, count: 1, from_auto: None, to_auto: None, pallet_auto: false, merge: false },
            ..rule("noitem", 0.0, 0, 0)
        };
        let t = shown(&no_item, &w);
        assert_eq!(t[t.len() - 3], ("품목".into(), "콘솔이 모름".into(), false));
        // 스테이션 화물은 GRM ItemExist 가 사실 — 콘솔 재고가 0 이어도 수량은 있고, 품목만 모른다.
        assert_eq!(t[t.len() - 2], ("출발 station 2101 준비".into(), "품목을 콘솔이 모름".into(), false));
        assert!(!fires(&Rule { trigger: Trigger::Manual, manual_requests: 1, ..no_item.clone() }, &w), "품목·재고를 모르면 조건이 참이 아니다");

        // 콘솔 재고가 품목을 알면 통과한다(스테이션 재고는 컨베이어 트래킹이 적는다).
        w.stock.insert(2101, (2013, 1));
        let t = shown(&no_item, &w);
        assert_eq!(t[t.len() - 3], ("품목".into(), "2013 (출발 재고)".into(), true));
        // 스테이션 PICK 은 준비 상태(CVOK & Req & ItemExist · 인계 · 예약)도 있어야 한다.
        assert!(!fires(&Rule { trigger: Trigger::Manual, manual_requests: 1, ..no_item.clone() }, &w), "준비 상태 모름");
        w.ready.insert(2101, ready::TargetReady { kind: "station".into(), id: 2101, pick: ready::Ready::no("Req 0"), ..Default::default() });
        let (on, terms) = evaluate(&Rule { trigger: Trigger::Manual, manual_requests: 1, ..no_item.clone() }, &w);
        assert!(!on && terms.last().unwrap() == &Term { label: "STATION 2101 PICK".into(), value: "Req 0".into(), ok: false }, "{terms:?}");
        w.ready.insert(2101, ready::TargetReady { kind: "station".into(), id: 2101, pick: ready::Ready::yes(), ..Default::default() });
        assert!(fires(&Rule { trigger: Trigger::Manual, manual_requests: 1, ..no_item }, &w));

        let c = Rule { trigger: Trigger::CellStock { cell: 409, item: Some(2013), min: 2 }, action: Action::Measure { target: cell(409), item: None }, ..rule("cs", 0.0, 0, 0) };
        assert_eq!(shown(&c, &w), vec![("셀 409 재고".into(), "0 개 / 조건 ≥ 2".into(), false), ("셀 409 품목".into(), "0 / 조건 2013".into(), false), ("대상".into(), "cell 409".into(), true),]);

        let nocv = Rule { trigger: Trigger::StationItem { station: 2101, require_cvok: false }, ..rule("it", 0.0, 401, 402) };
        assert_eq!(shown(&nocv, &w)[0], ("STATION 2101 ItemExist".into(), "1".into(), true));
        assert_eq!(shown(&nocv, &w)[1], ("STATION 2101 CVOK".into(), "무시".into(), true));

        let gone = Rule { trigger: Trigger::StationReq { station: 2999, require_cvok: true }, ..rule("no", 0.0, 401, 402) };
        assert_eq!(shown(&gone, &w)[0], ("STATION 2999".into(), "GRM 에서 못 읽음".into(), false));

        // 자동 셀은 값이 아니라 "자동" 이라고 말한다(품목·재고·칸은 고를 때 본다).
        let auto = Rule {
            action: Action::Transfer {
                from: cell(0),
                to: cell(0),
                item: None,
                count: 1,
                from_auto: Some(CellPick { order: "oldest".into(), ..Default::default() }),
                to_auto: Some(CellPick::default()),
                pallet_auto: false,
                merge: false,
            },
            ..rule("au", 0.0, 0, 0)
        };
        let a = shown(&auto, &w);
        assert_eq!(a[1], ("품목".into(), "출발 셀을 고를 때 확정".into(), true));
        assert_eq!(a[2], ("출발 셀".into(), "자동 선택(oldest)".into(), true));
        assert_eq!(a[3], ("도착 셀".into(), "자동 선택(가까운 순)".into(), true));
    }

    /// 조건은 규칙마다 끌 수 있다 — 끈 항목은 판정에서 빠지고 목록에 "무시" 로 남는다.
    #[test]
    fn conditions_can_be_turned_off_per_rule() {
        let mut w = World::default();
        w.stock.insert(401, (0, 0));
        w.room.insert(402, 0);
        w.usable.insert(401, false);
        w.usable.insert(402, true);
        let base = Rule { trigger: Trigger::Manual, manual_requests: 1, ..rule("c", 0.0, 401, 402) };

        // 기본(모두 켜짐) — Use 꺼진 출발에서 안 만든다.
        assert!(!fires(&base, &w));
        assert_eq!(source_ready(&w, &cell(401), 1, None, &Conditions::default()).as_deref(), Some("Use 꺼짐"));

        // Use 조건만 끄면 다음 걸림돌(재고)이 사유가 된다.
        let no_use = Conditions { target_use: false, ..Default::default() };
        assert_eq!(source_ready(&w, &cell(401), 1, None, &no_use).as_deref(), Some("재고 0 < 필요 1"));

        // 재고까지 끄면 품목이 남고, 품목까지 끄면 출발은 통과한다.
        let no_stock = Conditions { target_use: false, source_stock: false, ..Default::default() };
        assert_eq!(source_ready(&w, &cell(401), 1, None, &no_stock).as_deref(), Some("품목을 콘솔이 모름"));
        let loose = Conditions { item_known: false, ..no_stock };
        assert_eq!(source_ready(&w, &cell(401), 1, None, &loose), None);

        // 도착 칸 조건도 끌 수 있다(남은 칸 0 이어도 통과).
        assert_eq!(dest_ready(&w, &cell(402), 1, &Conditions::default()).as_deref(), Some("남은 칸 0 < 필요 1"));
        assert_eq!(dest_ready(&w, &cell(402), 1, &Conditions { dest_room: false, ..Default::default() }), None);

        // 다 끄면 조건이 참이 되고, 목록은 그 항목들을 "무시" 로 말한다.
        let all_off = Rule { cond: Conditions { item_known: false, source_stock: false, dest_room: false, target_use: false }, ..base };
        assert!(fires(&all_off, &w));
        assert!(rule_terms(&all_off, &w).iter().all(|t| t.ok), "{:?}", rule_terms(&all_off, &w));
        // 품목을 규칙이 정하지 않은 채 조건을 끄면 그 줄이 "무시" 라고 말한다.
        let no_item = Rule { action: Action::Transfer { from: cell(401), to: cell(402), item: None, count: 1, from_auto: None, to_auto: None, pallet_auto: false, merge: false }, ..all_off };
        let terms = rule_terms(&no_item, &w);
        assert!(terms.iter().any(|t| t.label == "품목" && t.value.contains("무시")), "{terms:?}");
    }

    /// 기본 규칙 한 벌 — 측정 먼저 · 출고 우선 · 출고에 가까운 셀 적재.
    #[test]
    fn default_rules_cover_the_operator_set() {
        let c = default_rules(2102, &[2101, 2003]);
        let ids: Vec<&str> = c.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["measure-first-2101", "store-near-out-2101", "measure-first-2003", "store-near-out-2003", "ship-2102"]);

        // 1. 미측정 규격은 측정 먼저 — 입고마다, 우선순위가 가장 높다.
        let m = c.rules.iter().find(|r| r.id == "measure-first-2101").unwrap();
        assert_eq!(m.trigger, Trigger::Unmeasured { target: Target { kind: "station".into(), id: 2101 } });
        assert!(matches!(m.action, Action::Measure { .. }));
        assert!(c.rules.iter().filter(|r| !r.id.starts_with("measure-first")).all(|r| r.priority < m.priority));

        // 2. 출고 우선 — 규칙 점수와 대상 가중 둘 다.
        let s = c.rules.iter().find(|r| r.id == "ship-2102").unwrap();
        assert!(s.priority > c.rules.iter().find(|r| r.id == "store-near-out-2101").unwrap().priority);
        assert_eq!(c.weights.target.get(&2102), Some(&20.0));

        // 5. 적재는 출고에 가까운 셀 먼저.
        let Action::Transfer { to_auto: Some(p), .. } = &c.rules.iter().find(|r| r.id == "store-near-out-2101").unwrap().action else { panic!("적재 규칙은 도착 셀 자동") };
        assert_eq!(p.near, Some(2102));

        // 규칙은 켜진 채로 심지만 자동 생성 자체는 꺼져 있다.
        assert!(!c.auto);
        assert!(c.rules.iter().all(|r| r.enabled));
    }

    /// 4. 회피가 최우선 — 막힌 로봇의 자리를 비켜 주는 후보가 먼저 나간다.
    #[test]
    fn avoidance_outranks_a_higher_scoring_job() {
        let robots = vec![robot(1, 0.0), robot(2, 9000.0)];
        // 1 호기는 2 호기 자리(9000)를 원해 막힌다. 2 호기에는 점수 높은 제자리 일과, 비켜 가는 일이 있다.
        let want = cand("a", 1, 8500.0, 9000.0, 10.0, 1);
        let stay = cand("stay", 2, 8800.0, 9200.0, 50.0, 2);
        // 비켜 가는 일 — 2 호기가 9000 에서 12000 으로 물러난다(도착점이 1 호기 후보 영역 밖).
        let away = cand("away", 2, 9000.0, 12000.0, 1.0, 3);

        // 보너스가 없으면 점수 높은 제자리 일(stay)이 나간다.
        let none = select_with(vec![want.clone(), stay.clone(), away.clone()], &robots, SEP, 0.0, &BTreeMap::new());
        assert_eq!(none.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["stay"]);

        // 보너스를 얹으면 비켜 가는 일(away)이 먼저 — 다음 판정에서 1 호기가 풀린다.
        let boosted = select_with(vec![want, stay, away], &robots, SEP, 100.0, &BTreeMap::new());
        let made: Vec<&str> = boosted.generate.iter().map(|c| c.rule_id.as_str()).collect();
        assert_eq!(made, vec!["away"]);
        assert!(boosted.generate[0].breakdown.iter().any(|(k, v)| k == "Avoidance" && *v == 100.0));
    }

    /// 멀티 피킹 합치기: 같은 품목 1개씩일 때만, 도착(PICK 스테이션)은 DROP 이 아니라 PICK 준비를 본다.
    #[test]
    fn merge_needs_same_item_one_each_and_a_pick_ready_target() {
        let st = |id: u16| Target { kind: "station".into(), id };
        let mut w = World::default();
        w.stock.insert(2102, (2011, 1));
        w.stock.insert(2101, (2011, 1));
        assert_eq!(merge_check(&w, &st(2102), &st(2101)), Ok(2011));
        w.stock.insert(2101, (2013, 1));
        assert!(merge_check(&w, &st(2102), &st(2101)).unwrap_err().contains("품목 다름"));
        w.stock.insert(2101, (2011, 2));
        assert!(merge_check(&w, &st(2102), &st(2101)).unwrap_err().contains("최대 2"), "기본(0) = 2");
        w.multi_pick_max = 3;
        assert_eq!(merge_check(&w, &st(2102), &st(2101)), Ok(2011), "최대 3이면 2개 위에 얹는다");
        w.stock.insert(2101, (2011, 3));
        assert!(merge_check(&w, &st(2102), &st(2101)).unwrap_err().contains("최대 3"));
        w.multi_pick_max = 0;
        w.stock.insert(2101, (0, 0));
        assert!(merge_check(&w, &st(2102), &st(2101)).unwrap_err().contains("모름"));
        w.stock.insert(2101, (2011, 1));
        w.items.insert(2011, ready::ItemFacts { stack_max: 1, ..Default::default() });
        assert!(merge_check(&w, &st(2102), &st(2101)).unwrap_err().contains("StackMax 1"));
        w.items.clear();

        let merge = Rule {
            action: Action::Transfer { from: st(2102), to: st(2101), item: None, count: 1, from_auto: None, to_auto: None, pallet_auto: false, merge: true },
            trigger: Trigger::Ready { require_measured: false },
            ..rule("merge-2102", 0.0, 0, 0)
        };
        let pick_ok = |id: u16| ready::TargetReady { kind: "station".into(), id, pick: ready::Ready::yes(), drop: ready::Ready::no("역할 pick — DROP 안 함"), ..Default::default() };
        w.ready.insert(2102, pick_ok(2102));
        w.ready.insert(2101, pick_ok(2101));
        let (on, terms) = evaluate(&merge, &w);
        assert!(on, "{terms:?}");
        assert!(terms.iter().any(|t| t.label == "STATION 2101 PICK" && t.ok));
        assert!(!terms.iter().any(|t| t.label.contains("2101 DROP")));
    }

    /// 빈 시간 정리 도착 = 같은 품목만, 가장 많이 쌓인 셀부터(풀스택 만들기).
    #[test]
    fn consolidation_fills_the_fullest_stack_first() {
        let cells = vec![cv(401, 1000.0, 2011, 1, Some(3), "a"), cv(402, 9000.0, 2011, 3, Some(1), "b"), cv(403, 2000.0, 0, 0, None, ""), cv(404, 1500.0, 2011, 4, Some(0), "c")];
        let f = CellPick { same_item_first: true, same_item_only: true, ..Default::default() };
        assert_eq!(choose_dest(&cells, &f, 2011, 1, Some(0.0), None, Some(401), &|_| true), Some(402), "404 is full, 403 is empty");
        // 멀티 피킹 2개 — 빈 셀만(같은 품목 칸이 있어도).
        let empty = CellPick { empty_only: true, ..Default::default() };
        assert_eq!(choose_dest(&cells, &empty, 2011, 2, Some(0.0), None, None, &|_| true), Some(403));
        let src = CellPick { order: "fewest".into(), ..Default::default() };
        assert_eq!(choose_source(&cells, &src, Some(2011), 1, Some(0.0), &|_| true), Some(401));
    }
}
