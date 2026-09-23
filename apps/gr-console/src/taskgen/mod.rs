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

pub mod routes;
pub mod run;

use std::collections::BTreeMap;

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
    /// 셀 재고가 `min` 개 이상(품목 지정 시 그 품목만).
    CellStock {
        cell: u16,
        #[serde(default)]
        item: Option<u32>,
        #[serde(default = "one")]
        min: u32,
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
    /// 셀 id → (품목, 개수) — 예상 재고.
    pub stock: BTreeMap<u16, (u32, u32)>,
    /// 셀 id → 남은 칸(StackMax − 예상 재고). 없으면 제한 없음.
    pub room: BTreeMap<u16, u32>,
}

pub fn fires(rule: &Rule, w: &World) -> bool {
    if !rule.enabled {
        return false;
    }
    let pi = |s: &u16| w.stations.get(s).copied().unwrap_or_default();
    let base = match &rule.trigger {
        Trigger::Manual => rule.manual_requests > 0,
        Trigger::StationReq { station, require_cvok } => pi(station).req && (!require_cvok || pi(station).cvok),
        Trigger::StationItem { station, require_cvok } => pi(station).item_exist && (!require_cvok || pi(station).cvok),
        Trigger::CellStock { cell, item, min } => w.stock.get(cell).is_some_and(|(code, n)| *n >= *min && item.is_none_or(|i| i == *code)),
    };
    // 고정 셀로 옮길 것은 출발에 재고가, 도착에 칸이 있어야 한다(자동 셀은 고를 때 본다, 스테이션은 모른다 — 통과).
    base && match &rule.action {
        Action::Transfer { from, to, count, from_auto, to_auto, .. } => {
            let have = from_auto.is_some() || from.kind != "cell" || w.stock.get(&from.id).is_some_and(|(_, n)| *n >= *count as u32);
            let room = to_auto.is_some() || to.kind != "cell" || w.room.get(&to.id).is_none_or(|r| *r >= *count as u32);
            have && room
        }
        _ => true,
    }
}

/// 조건이 보는 입력의 지금 값 — 화면이 "왜 안 참인가" 를 규칙 줄에서 말할 수 있게.
pub fn trigger_inputs(rule: &Rule, w: &World) -> String {
    let b = |v: bool| if v { 1 } else { 0 };
    let pi = |s: &u16| w.stations.get(s).copied().unwrap_or_default();
    let mut s = match &rule.trigger {
        Trigger::Manual => format!("요청 {} 건 남음", rule.manual_requests),
        Trigger::StationReq { station, require_cvok } => {
            let p = pi(station);
            format!("STATION {station}: Req={} CVOK={}{}", b(p.req), b(p.cvok), if *require_cvok { "" } else { " (CVOK 무시)" })
        }
        Trigger::StationItem { station, require_cvok } => {
            let p = pi(station);
            format!("STATION {station}: ItemExist={} CVOK={}{}", b(p.item_exist), b(p.cvok), if *require_cvok { "" } else { " (CVOK 무시)" })
        }
        Trigger::CellStock { cell, item, min } => match w.stock.get(cell) {
            Some((code, n)) => format!("셀 {cell}: 재고 {n} 개(품목 {code}) / 조건 ≥ {min}{}", item.map(|i| format!(" 품목 {i}")).unwrap_or_default()),
            None => format!("셀 {cell}: 재고 없음 / 조건 ≥ {min}"),
        },
    };
    // 고정 셀 이송은 출발 재고·도착 칸도 조건이다(`fires`) — 같이 보여 준다.
    if let Action::Transfer { from, to, count, from_auto, to_auto, .. } = &rule.action {
        if from_auto.is_none() && from.kind == "cell" {
            let have = w.stock.get(&from.id).map(|(_, n)| *n).unwrap_or(0);
            s.push_str(&format!(" · 출발 셀 {} 재고 {have}/{count}", from.id));
        }
        if to_auto.is_none() && to.kind == "cell" {
            match w.room.get(&to.id) {
                Some(r) => s.push_str(&format!(" · 도착 셀 {} 남은 칸 {r}/{count}", to.id)),
                None => s.push_str(&format!(" · 도착 셀 {} 칸 제한 없음", to.id)),
            }
        }
    }
    s
}

/// 규칙 하나의 지금 상태(모니터).
#[derive(Clone, Debug, Serialize)]
pub struct RuleStatus {
    pub rule_id: String,
    pub fires: bool,
    pub inputs: String,
    /// `off` 규칙 꺼짐 · `idle` 조건 거짓 · `busy` 이 규칙 작업이 아직 돈다 · `queued` 예정에 있다 ·
    /// `skipped` 후보 못 됨 · `waiting` 후보지만 대기 · `ready` 지금 만들 수 있다(자동이 꺼져 있으면 만들지 않는다).
    pub state: String,
    pub reason: Option<String>,
    /// 조건이 참이 된 뒤 지난 시간(분).
    pub age_min: f32,
    /// 엔진이 도는 동안의 누적.
    pub generated: u64,
    pub last_generated_at: Option<String>,
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

/// 출발 셀 — 품목(있으면 같은 품목) · 개수 충분 · 영역 밖(`free`) 중 `oldest`(재고 갱신이 가장 오래된) 또는 `nearest`.
pub fn choose_source(cells: &[CellView], f: &CellPick, item: Option<u32>, count: u32, robot_x: Option<f32>, free: &dyn Fn(f32) -> bool) -> Option<u16> {
    let mut v: Vec<&CellView> = cells.iter().filter(|c| in_filter(c, f) && c.count >= count.max(1) && c.item != 0 && item.is_none_or(|i| i == c.item) && free(c.x)).collect();
    let dist = |c: &CellView| robot_x.map(|x| (c.x - x).abs()).unwrap_or(0.0);
    if f.order == "nearest" {
        v.sort_by(|a, b| dist(a).total_cmp(&dist(b)).then(a.id.cmp(&b.id)));
    } else {
        v.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
    }
    v.first().map(|c| c.id)
}

/// 도착 셀 — 칸 충분(StackMax) · 비었거나 같은 품목 · 영역 밖 중 (`same_item_first` 면 같은 품목 먼저) 가까운 순.
pub fn choose_dest(cells: &[CellView], f: &CellPick, item: u32, count: u32, robot_x: Option<f32>, exclude: Option<u16>, free: &dyn Fn(f32) -> bool) -> Option<u16> {
    let ok = |c: &&CellView| in_filter(c, f) && Some(c.id) != exclude && (c.count == 0 || (item != 0 && c.item == item)) && c.room.is_none_or(|r| r >= count.max(1)) && free(c.x);
    let mut v: Vec<&CellView> = cells.iter().filter(ok).collect();
    let dist = |c: &CellView| robot_x.map(|x| (c.x - x).abs()).unwrap_or(0.0);
    v.sort_by(|a, b| {
        let same = |c: &CellView| if f.same_item_first { (c.count == 0) as u8 } else { (c.count != 0) as u8 };
        same(a).cmp(&same(b)).then(dist(a).total_cmp(&dist(b))).then(a.id.cmp(&b.id))
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
}

/// 영역을 지키며 고른다 — 순위대로, 로봇·규칙마다 하나, 다른 로봇 영역(+ 이번에 고른 것)과 간격 안이면 넘긴다.
/// 막힌 로봇의 사유: 상대가 비켜 가는 후보를 이번에 만들면 "상대 명령 수령 대기", 아니면 "비켜 줄 작업 없음 — 대기".
pub fn select(mut cands: Vec<Candidate>, robots: &[RobotView], sep: f32) -> Selection {
    rank(&mut cands);
    let mut out = Selection::default();
    let mut picked: BTreeMap<u8, Interval> = BTreeMap::new();
    let mut used_rules: Vec<String> = Vec::new();
    let mut blocked: Vec<(Candidate, u8)> = Vec::new();
    for c in cands {
        let Some(me) = robots.iter().find(|r| r.id == c.robot) else { continue };
        if me.busy || picked.contains_key(&c.robot) {
            out.waiting.push((c, "이 로봇에 예정·진행 중 생성 작업 있음".into()));
            continue;
        }
        if used_rules.contains(&c.rule_id) {
            out.waiting.push((c, "같은 규칙을 다른 로봇이 맡음".into()));
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
                out.generate.push(c);
            }
        }
    }
    // 막힌 후보의 사유를 다듬는다 — 상대가 이번에 비켜 가는(도착점이 이 후보 영역에서 간격 밖) 작업을 만들었나.
    for (c, by) in blocked {
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
        Action::Transfer { from: cell(from), to: cell(to), item: Some(2011), count: 1, from_auto: None, to_auto: None, pallet_auto: false }
    }
    fn rule(id: &str, prio: f32, from: u16, to: u16) -> Rule {
        Rule { id: id.into(), name: id.into(), enabled: true, trigger: Trigger::Manual, action: transfer(from, to), robots: vec![], priority: prio, manual_requests: 1 }
    }
    fn cand(rule_id: &str, robot: u8, lo: f32, hi: f32, score: f32, order: u64) -> Candidate {
        Candidate {
            rule_id: rule_id.into(),
            rule_name: rule_id.into(),
            robot,
            first: cell(1),
            second: Some(cell(2)),
            target_x: lo,
            drop_x: Some(hi),
            area: Interval::span(lo, hi),
            score,
            breakdown: vec![],
            order,
            age_min: 0.0,
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
        assert_eq!(choose_dest(&cells, &same, 2011, 2, Some(0.0), None, &free), Some(402));
        let empty_first = CellPick::default();
        assert_eq!(choose_dest(&cells, &empty_first, 2011, 1, Some(0.0), None, &free), Some(403));
        // 출발 셀은 도착에서 뺀다 · 다른 품목 셀은 안 된다 · 영역
        assert_eq!(choose_dest(&cells, &same, 2011, 1, Some(0.0), Some(401), &left), Some(403));
        // 필터: 구역 2 셀 없음
        let sec2 = CellPick { section: Some(2), ..Default::default() };
        assert_eq!(choose_dest(&cells, &sec2, 2011, 1, None, None, &free), None);
    }

    #[test]
    fn scoring_uses_station_item_robot_age_distance() {
        let mut w = Weights::default();
        w.target.insert(2101, 10.0);
        w.item.insert(2011, 5.0);
        w.robot.insert(2, 1.0);
        let k = Knobs { age_per_min: 0.5, distance_per_m: 2.0 };
        let mut r = rule("a", 3.0, 401, 2101);
        r.action = Action::Transfer { from: cell(401), to: Target { kind: "station".into(), id: 2101 }, item: Some(2011), count: 1, from_auto: None, to_auto: None, pallet_auto: false };
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
        let s = select(vec![cand("a", 1, 2000.0, 5000.0, 5.0, 1), cand("b", 2, 10000.0, 14000.0, 4.0, 2)], &robots, SEP);
        assert_eq!(s.generate.len(), 2);
        let s = select(vec![cand("b", 2, 9000.0, 14000.0, 9.0, 1), cand("a1", 1, 2000.0, 8000.0, 8.0, 2), cand("a2", 1, 2000.0, 5000.0, 1.0, 3)], &robots, SEP);
        assert_eq!(s.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["b", "a2"]);
        assert!(s.waiting.iter().any(|(c, why)| c.rule_id == "a1" && why.contains("영역 겹침")));
        let s = select(vec![cand("r", 1, 2000.0, 3000.0, 5.0, 1), cand("r", 2, 12000.0, 14000.0, 5.0, 1)], &robots, SEP);
        assert_eq!(s.generate.len(), 1);
        // 로봇 여유가 간격에 더해진다
        let mut wide = robots.clone();
        wide[1].margin = 3000.0;
        let s = select(vec![cand("b", 2, 10000.0, 14000.0, 9.0, 1), cand("a", 1, 2000.0, 5000.0, 1.0, 2)], &wide, SEP);
        assert_eq!(s.generate.len(), 1, "5000..10000 gap < 2403 + 3000");
    }

    /// 회피 순서: GR1 이 GR2 자리가 필요 — GR2 가 비켜 가는 후보를 이번에 만들고 GR1 은 "수령 뒤" 로 기다린다.
    #[test]
    fn avoidance_is_sequenced_after_the_other_robot_accepts() {
        let mut robots = vec![robot(1, 2000.0), robot(2, 8000.0)];
        let need = cand("a", 1, 2000.0, 7000.0, 9.0, 1);
        let away = cand("b", 2, 8000.0, 13000.0, 1.0, 2);
        let s = select(vec![need.clone(), away.clone()], &robots, SEP);
        assert_eq!(s.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        let why = &s.waiting.iter().find(|(c, _)| c.rule_id == "a").unwrap().1;
        assert!(why.contains("PLC 수령(Accepted) 뒤"), "{why}");
        robots[1].scheduled = Some(away.area);
        robots[1].busy = true;
        assert!(select(vec![need.clone()], &robots, SEP).generate.is_empty());
        robots[1].scheduled = None;
        robots[1].pending = vec![13000.0];
        assert!(select(vec![need.clone()], &robots, SEP).generate.is_empty());
        robots[1].committed = true;
        assert_eq!(select(vec![need.clone()], &robots, SEP).generate.len(), 1);
    }

    #[test]
    fn no_deadlock_no_overlap() {
        let robots = vec![robot(1, 4000.0), robot(2, 7000.0)];
        let s = select(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1)], &robots, SEP);
        assert!(s.generate.is_empty());
        assert!(s.waiting[0].1.contains("비켜 줄 작업 없음"));
        let s = select(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1), cand("b", 2, 3500.0, 7000.0, 4.0, 2)], &robots, SEP);
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

        let r = Rule { trigger: Trigger::StationReq { station: 2101, require_cvok: true }, ..rule("st", 0.0, 401, 402) };
        let s = trigger_inputs(&r, &w);
        assert!(s.contains("Req=0") && s.contains("CVOK=1"), "{s}");
        assert!(s.contains("출발 셀 401 재고 3/1") && s.contains("도착 셀 402 남은 칸 1/1"), "{s}");

        let m = Rule { trigger: Trigger::Manual, manual_requests: 2, ..rule("man", 0.0, 401, 402) };
        assert!(trigger_inputs(&m, &w).starts_with("요청 2 건"), "{}", trigger_inputs(&m, &w));

        let c = Rule { trigger: Trigger::CellStock { cell: 409, item: None, min: 2 }, action: Action::Measure { target: cell(409), item: None }, ..rule("cs", 0.0, 0, 0) };
        assert!(trigger_inputs(&c, &w).contains("셀 409: 재고 없음"), "{}", trigger_inputs(&c, &w));

        let nocv = Rule { trigger: Trigger::StationItem { station: 2101, require_cvok: false }, ..rule("it", 0.0, 401, 402) };
        assert!(trigger_inputs(&nocv, &w).contains("CVOK 무시"), "{}", trigger_inputs(&nocv, &w));
    }
}
