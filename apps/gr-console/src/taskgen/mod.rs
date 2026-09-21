//! Task 생성 엔진 — 규칙(파라미터)으로 후보를 만들고, 상황별 가중치로 점수를 매기고, 두 로봇 영역이 겹치지 않는 것만
//! "예정" 으로 생성한다. 실제 제출은 `run.rs` 의 발행기가 실행기와 같은 규칙(큐 깊이 1 · 짝 · 이송 지시 · 영역)으로 한다.
//!
//! - 규칙 = 조건(`Trigger`) + 만들 것(`Action`) + 로봇 + 기본 우선순위. 코드가 아니라 저장된 JSON(버전)이다.
//! - 점수 = 규칙 기본값 + 스테이션 가중 + 품목 가중 + 로봇 가중 + 대기 시간(분당) − 거리(m 당) (`score`).
//!   같은 점수면 먼저 생긴(조건이 먼저 참이 된) 후보가 먼저 — 결정적.
//! - 영역(`select`): 후보 영역(짝이면 PICK∪DROP)이 다른 로봇이 잡은 영역(발행 · 예정)과 간격 안이면 지금은 만들지 않는다.
//!   겹치지 않는 후보를 먼저 골라 두 로봇이 동시에 돈다. 막힌 로봇은 상대가 비켜 줄 후보가 있으면 그 명령이 PLC 에
//!   **받아진 뒤**(Accepted — `area::reservation` 의 committed) 다음 판정에서 풀리고, 비켜 줄 것이 없으면 사유와 함께 기다린다.
//!   자동 회피 MOVE 는 만들지 않는다.

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
    /// GRM `OPCUA.STATION[n].Interlock.PI.Req` (스테이션 작업 요청).
    StationReq { station: u16 },
    /// GRM `PI.ItemExist` (스테이션에 화물 있음).
    StationItem { station: u16 },
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

/// 만들 것.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// PICK → DROP 짝(이송 지시 하나).
    Transfer {
        from: Target,
        to: Target,
        #[serde(default)]
        item: Option<u32>,
        #[serde(default = "one_u8")]
        count: u8,
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
    /// `Manual` 규칙의 남은 요청 수.
    #[serde(default)]
    pub manual_requests: u32,
}

fn yes() -> bool {
    true
}

/// 상황별 가중치.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    /// 스테이션(또는 셀) id → 가중(출발·도착 어느 쪽이든 그 대상이 들어가면 더한다).
    pub target: BTreeMap<u16, f32>,
    /// 품목 코드 → 가중(특정 규격 우선).
    pub item: BTreeMap<u32, f32>,
    /// 로봇 id → 가중(선호 호기).
    pub robot: BTreeMap<u8, f32>,
    /// 조건이 참이 된 뒤 1 분마다 더하는 값(오래 기다린 것 먼저).
    pub age_per_min: f32,
    /// 로봇 지금 X 에서 첫 목표까지 1 m 마다 빼는 값(가까운 로봇 먼저).
    pub distance_per_m: f32,
    /// 영역에 막혀 이번에 못 만든 후보에 표시로 빼는 값(설명용 — 막힌 후보는 어차피 만들지 않는다).
    pub blocked_penalty: f32,
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

/// 규칙 조건을 판단할 세상(스냅샷).
#[derive(Clone, Debug, Default)]
pub struct World {
    /// 스테이션 id → (Req, ItemExist).
    pub stations: BTreeMap<u16, (bool, bool)>,
    /// 셀 id → (품목, 개수) — 예상 재고.
    pub stock: BTreeMap<u16, (u32, u32)>,
    /// 셀 id → 남은 칸(StackMax − 예상 재고). 없으면 제한 없음.
    pub room: BTreeMap<u16, u32>,
}

pub fn fires(rule: &Rule, w: &World) -> bool {
    if !rule.enabled {
        return false;
    }
    let base = match &rule.trigger {
        Trigger::Manual => rule.manual_requests > 0,
        Trigger::StationReq { station } => w.stations.get(station).is_some_and(|s| s.0),
        Trigger::StationItem { station } => w.stations.get(station).is_some_and(|s| s.1),
        Trigger::CellStock { cell, item, min } => w.stock.get(cell).is_some_and(|(code, n)| *n >= *min && item.is_none_or(|i| i == *code)),
    };
    // 옮길 것은 출발에 재고가, 도착에 칸이 있어야 한다(스테이션은 모른다 — 통과).
    base && match &rule.action {
        Action::Transfer { from, to, count, .. } => {
            let have = from.kind != "cell" || w.stock.get(&from.id).is_some_and(|(_, n)| *n >= *count as u32);
            let room = to.kind != "cell" || w.room.get(&to.id).is_none_or(|r| *r >= *count as u32);
            have && room
        }
        _ => true,
    }
}

/// 후보 하나(규칙 × 로봇).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Candidate {
    pub rule_id: String,
    pub rule_name: String,
    pub robot: u8,
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
}

fn target_of(a: &Action) -> Vec<&Target> {
    match a {
        Action::Transfer { from, to, .. } => vec![from, to],
        Action::Move { to } => vec![to],
        Action::Measure { target, .. } => vec![target],
    }
}

fn item_of(a: &Action) -> Option<u32> {
    match a {
        Action::Transfer { item, .. } | Action::Measure { item, .. } => *item,
        Action::Move { .. } => None,
    }
}

/// 점수와 근거.
pub fn score(rule: &Rule, robot: u8, w: &Weights, age_min: f32, distance_m: f32) -> (f32, Vec<(String, f32)>) {
    let mut b: Vec<(String, f32)> = vec![("Rule.Priority".into(), rule.priority)];
    for t in target_of(&rule.action) {
        if let Some(v) = w.target.get(&t.id) {
            b.push((format!("Target {}", t.id), *v));
        }
    }
    if let Some(v) = item_of(&rule.action).and_then(|i| w.item.get(&i)) {
        b.push((format!("Item {}", item_of(&rule.action).unwrap_or(0)), *v));
    }
    if let Some(v) = w.robot.get(&robot) {
        b.push((format!("Robot {robot}"), *v));
    }
    if w.age_per_min != 0.0 && age_min > 0.0 {
        b.push(("Age".into(), w.age_per_min * age_min));
    }
    if w.distance_per_m != 0.0 && distance_m > 0.0 {
        b.push(("Distance".into(), -w.distance_per_m * distance_m));
    }
    (b.iter().map(|(_, v)| v).sum(), b)
}

/// 점수 내림차순, 같으면 먼저 생긴 순.
pub fn rank(c: &mut [Candidate]) {
    c.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(a.order.cmp(&b.order)).then(a.robot.cmp(&b.robot)));
}

/// 후보 영역.
pub fn candidate_area(r: &RobotView, target_x: f32, drop_x: Option<f32>) -> Interval {
    task_area(r.pending.last().copied(), r.current_x, target_x, drop_x)
}

/// 다른 로봇 `o` 가 잡은 영역(발행 · 예정 · 이번에 고른 것).
fn reserved(o: &RobotView, picked: Option<Interval>) -> Option<Reservation> {
    let mut r = reservation(o.id, &o.name, o.current_x, &o.pending, o.committed, None);
    for extra in [o.scheduled, picked].into_iter().flatten() {
        r = Some(match r {
            Some(mut x) => {
                x.area = x.area.with(extra.lo).with(extra.hi);
                x.idle = false;
                x
            }
            None => Reservation { robot: o.id, name: o.name.clone(), area: extra, idle: false, holds_pair: false },
        });
    }
    r
}

/// 판정 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Selection {
    /// 이번에 "예정" 으로 만들 후보(로봇마다 최대 하나, 규칙마다 하나).
    pub generate: Vec<Candidate>,
    /// 못 만든 후보와 사유.
    pub waiting: Vec<(Candidate, String)>,
}

/// 영역을 지키며 고른다 — 순위대로, 로봇·규칙마다 하나, 다른 로봇 영역(+ 이번에 고른 것)과 간격 안이면 넘긴다.
/// 막힌 로봇의 사유: 상대가 비켜 줄 후보를 이번에 만들면 "상대 명령 수령 대기", 아니면 "비켜 줄 작업 없음 — 대기".
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
        match blocker(c.area, &others, sep) {
            Some(b) => {
                let why = format!("영역 겹침: {} X {} 사용 중 — 이 후보 X {} (간격 {sep:.0} mm)", b.name, b.area.label(), c.area.label());
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
    // 막힌 후보의 사유를 다듬는다 — 상대가 이번에 비켜 가는 작업을 만들었나.
    for (c, by) in blocked {
        // 상대의 **도착점**(짝이면 DROP)이 이 후보 영역에서 간격 밖이면 비켜 가는 작업이다.
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
    fn rule(id: &str, prio: f32, from: u16, to: u16) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            trigger: Trigger::Manual,
            action: Action::Transfer { from: cell(from), to: cell(to), item: Some(2011), count: 1 },
            robots: vec![],
            priority: prio,
            manual_requests: 1,
        }
    }
    fn cand(rule_id: &str, robot: u8, lo: f32, hi: f32, score: f32, order: u64) -> Candidate {
        Candidate { rule_id: rule_id.into(), rule_name: rule_id.into(), robot, target_x: lo, drop_x: Some(hi), area: Interval::span(lo, hi), score, breakdown: vec![], order, age_min: 0.0 }
    }
    fn robot(id: u8, x: f32) -> RobotView {
        RobotView { id, name: format!("GR{id}"), current_x: Some(x), ..Default::default() }
    }

    #[test]
    fn triggers_and_room() {
        let mut w = World::default();
        w.stations.insert(2101, (true, false));
        w.stock.insert(401, (2011, 3));
        w.room.insert(402, 0);
        let mut r = rule("a", 0.0, 401, 403);
        assert!(fires(&r, &w), "manual request + stock at the source");
        r.manual_requests = 0;
        assert!(!fires(&r, &w));
        r.trigger = Trigger::StationReq { station: 2101 };
        assert!(fires(&r, &w));
        r.trigger = Trigger::StationItem { station: 2101 };
        assert!(!fires(&r, &w));
        r.trigger = Trigger::CellStock { cell: 401, item: Some(2011), min: 3 };
        assert!(fires(&r, &w));
        r.trigger = Trigger::CellStock { cell: 401, item: Some(9999), min: 1 };
        assert!(!fires(&r, &w));
        // 도착 칸 없음 → 안 만든다
        r.trigger = Trigger::Manual;
        r.manual_requests = 1;
        r.action = Action::Transfer { from: cell(401), to: cell(402), item: None, count: 1 };
        assert!(!fires(&r, &w));
        r.enabled = false;
        r.action = Action::Move { to: cell(401) };
        assert!(!fires(&r, &w));
    }

    #[test]
    fn scoring_uses_station_item_robot_age_distance() {
        let mut w = Weights::default();
        w.target.insert(2101, 10.0);
        w.item.insert(2011, 5.0);
        w.robot.insert(2, 1.0);
        w.age_per_min = 0.5;
        w.distance_per_m = 2.0;
        let mut r = rule("a", 3.0, 401, 2101);
        r.action = Action::Transfer { from: cell(401), to: Target { kind: "station".into(), id: 2101 }, item: Some(2011), count: 1 };
        let (s, b) = score(&r, 2, &w, 4.0, 1.5);
        assert_eq!(s, 3.0 + 10.0 + 5.0 + 1.0 + 2.0 - 3.0);
        assert!(b.iter().any(|(k, _)| k == "Target 2101") && b.iter().any(|(k, v)| k == "Distance" && *v == -3.0));
        // 같은 점수면 먼저 생긴 것
        let mut c = vec![cand("x", 1, 0.0, 1.0, 5.0, 2), cand("y", 1, 0.0, 1.0, 9.0, 3), cand("z", 1, 0.0, 1.0, 5.0, 1)];
        rank(&mut c);
        assert_eq!(c.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["y", "z", "x"]);
    }

    /// 두 로봇 — 떨어진 후보는 같이, 겹치는 후보는 하나만(점수 높은 것).
    #[test]
    fn zone_aware_selection_maximises_concurrency() {
        let robots = vec![robot(1, 2000.0), robot(2, 14000.0)];
        // 떨어져 있다: 둘 다
        let s = select(vec![cand("a", 1, 2000.0, 5000.0, 5.0, 1), cand("b", 2, 10000.0, 14000.0, 4.0, 2)], &robots, SEP);
        assert_eq!(s.generate.len(), 2);
        // GR1 최고 후보가 GR2 후보와 겹치면, GR1 은 겹치지 않는 다음 후보를 고른다
        let s = select(vec![cand("b", 2, 9000.0, 14000.0, 9.0, 1), cand("a1", 1, 2000.0, 8000.0, 8.0, 2), cand("a2", 1, 2000.0, 5000.0, 1.0, 3)], &robots, SEP);
        let got: Vec<&str> = s.generate.iter().map(|c| c.rule_id.as_str()).collect();
        assert_eq!(got, vec!["b", "a2"]);
        assert!(s.waiting.iter().any(|(c, why)| c.rule_id == "a1" && why.contains("영역 겹침")));
        // 규칙 하나는 한 로봇만
        let s = select(vec![cand("r", 1, 2000.0, 3000.0, 5.0, 1), cand("r", 2, 12000.0, 14000.0, 5.0, 1)], &robots, SEP);
        assert_eq!(s.generate.len(), 1);
    }

    /// 회피 순서: GR1 이 GR2 자리가 필요 — GR2 가 비켜 가는 후보를 이번에 만들고 GR1 은 "수령 뒤" 로 기다린다.
    /// GR2 명령이 받아지면(committed) 다음 판정에서 GR1 이 생성된다. 두 로봇이 겹치는 영역에 동시에 나가는 일은 없다.
    #[test]
    fn avoidance_is_sequenced_after_the_other_robot_accepts() {
        let mut robots = vec![robot(1, 2000.0), robot(2, 8000.0)];
        let need = cand("a", 1, 2000.0, 7000.0, 9.0, 1);
        let away = cand("b", 2, 8000.0, 13000.0, 1.0, 2);
        let s = select(vec![need.clone(), away.clone()], &robots, SEP);
        assert_eq!(s.generate.iter().map(|c| c.rule_id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        let why = &s.waiting.iter().find(|(c, _)| c.rule_id == "a").unwrap().1;
        assert!(why.contains("PLC 수령(Accepted) 뒤"), "{why}");
        // GR2 에 예정(발행 전): 지금 X 8000 + 예정 영역 → 아직 막힘
        robots[1].scheduled = Some(away.area);
        robots[1].busy = true;
        assert!(select(vec![need.clone()], &robots, SEP).generate.is_empty());
        // 발행됐지만 아직 안 받음(Submitted): 지금 X 포함 → 막힘
        robots[1].scheduled = None;
        robots[1].pending = vec![13000.0];
        assert!(select(vec![need.clone()], &robots, SEP).generate.is_empty());
        // PLC 가 받음(Accepted) → 목표만 잡는다 → GR1 생성
        robots[1].committed = true;
        assert_eq!(select(vec![need.clone()], &robots, SEP).generate.len(), 1);
    }

    /// 비켜 줄 작업이 없으면 기다린다(자동 회피 없음). 서로의 자리가 필요해도 둘 다 겹쳐 나가지는 않는다.
    #[test]
    fn no_deadlock_no_overlap() {
        let robots = vec![robot(1, 4000.0), robot(2, 7000.0)];
        let s = select(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1)], &robots, SEP);
        assert!(s.generate.is_empty());
        assert!(s.waiting[0].1.contains("비켜 줄 작업 없음"));
        // 서로 상대 자리를 원함: 하나도 만들지 않고 둘 다 사유
        let s = select(vec![cand("a", 1, 4000.0, 7500.0, 5.0, 1), cand("b", 2, 3500.0, 7000.0, 4.0, 2)], &robots, SEP);
        assert!(s.generate.is_empty() && s.waiting.len() == 2);
    }

    #[test]
    fn config_round_trips_as_versioned_json() {
        let mut c = GenConfig { version: 3, auto: false, ..Default::default() };
        c.weights.target.insert(2101, 10.0);
        c.rules.push(Rule { trigger: Trigger::StationReq { station: 2101 }, ..rule("st", 2.0, 401, 402) });
        c.rules.push(Rule { trigger: Trigger::CellStock { cell: 401, item: None, min: 2 }, action: Action::Measure { target: cell(401), item: None }, ..rule("m", 0.0, 0, 0) });
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("\"kind\":\"station_req\"") && j.contains("\"kind\":\"transfer\""), "{j}");
        let back: GenConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        // 모자란 필드는 기본값(enabled = true, count = 1, min = 1)
        let r: Rule =
            serde_json::from_str(r#"{"id":"x","name":"x","trigger":{"kind":"cell_stock","cell":1},"action":{"kind":"transfer","from":{"kind":"cell","id":1},"to":{"kind":"cell","id":2}}}"#).unwrap();
        assert!(r.enabled);
        assert_eq!(r.trigger, Trigger::CellStock { cell: 1, item: None, min: 1 });
    }
}
