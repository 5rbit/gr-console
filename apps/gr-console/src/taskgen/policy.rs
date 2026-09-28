//! 정책 — 모든 셀 · 스테이션에 공통으로 도는 수요. 판정마다 요청 목록 · 스테이션 프로파일 · 정책에서 규칙을 파생한다
//! (저장하지 않는다). 파생 규칙 id 가 중복 방지 키다 — 스테이션 · 요청 단위라 한 스테이션 작업이 다른 스테이션을 막지 않는다.
//!
//! | id | 수요 |
//! |---|---|
//! | `req:<요청 id>@<스테이션 · 셀>` | 요청(상위 · 사용자) — 점수 `request_priority` + 요청 priority × 10 |
//! | `measure-<s>` | 측정 먼저: PICK 스테이션 화물의 품목에 비드 프로파일이 없다 |
//! | `in-<s>` | 입고 자동: PICK 스테이션 화물 → 적재 셀 |
//! | `out-<s>` | 출고 자동: DROP 스테이션 요청 → 가장 오래된 재고 |
//! | `consolidate` | 빈 시간 정리: 같은 품목을 적게 쌓인 셀에서 많이 쌓인 셀로 |

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::profile::{DropMode, StationProfile};
use super::requests::{ReqKind, TransferRequest};
use super::{Action, CellPick, Conditions, Rule, RuleOrigin, Trigger};
use crate::ledger::Target;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub inbound_auto: bool,
    pub inbound_dest: CellPick,
    /// 적재 셀을 가장 가까운 DROP 스테이션 기준으로 고른다.
    pub inbound_near_drop: bool,
    pub inbound_priority: f32,
    pub outbound_auto: bool,
    pub outbound_source: CellPick,
    pub outbound_priority: f32,
    pub measure_first: bool,
    pub measure_priority: f32,
    pub request_priority: f32,
    pub consolidate: bool,
    pub consolidate_idle_s: u32,
    pub consolidate_priority: f32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            inbound_auto: true,
            inbound_dest: CellPick { order: "nearest".into(), ..Default::default() },
            inbound_near_drop: true,
            inbound_priority: 40.0,
            outbound_auto: false,
            outbound_source: CellPick { order: "oldest".into(), ..Default::default() },
            outbound_priority: 60.0,
            measure_first: true,
            measure_priority: 80.0,
            request_priority: 1000.0,
            consolidate: false,
            consolidate_idle_s: 60,
            consolidate_priority: 5.0,
        }
    }
}

pub const CONSOLIDATE_ID: &str = "consolidate";

fn station(id: u16) -> Target {
    Target { kind: "station".into(), id }
}

fn cell0() -> Target {
    Target { kind: "cell".into(), id: 0 }
}

fn rule(id: String, name: String, origin: RuleOrigin, trigger: Trigger, action: Action, priority: f32) -> Rule {
    Rule { id, name, enabled: true, trigger, action, robots: vec![], priority, manual_requests: 0, cond: Conditions::default(), origin, request_id: None, station: None }
}

/// 파생에 쓰는 세상 — 등록 스테이션(프로파일), DROP 스테이션 X(적재 셀 "가까운" 기준), 로봇마다 쉰 시간.
pub struct Ctx<'a> {
    pub profiles: &'a BTreeMap<u16, StationProfile>,
    /// 가장 가까운 DROP 스테이션을 고르는 기준(없으면 로봇 기준).
    pub first_drop_station: Option<u16>,
    /// 빈 시간 정리를 켤 로봇들(쉰 시간 ≥ `consolidate_idle_s`).
    pub idle_robots: Vec<u8>,
    /// 요청 id → 아직 이송 지시가 없는 예정(보낼 차례를 기다리는 생성 작업) 수.
    pub queued_for_request: &'a BTreeMap<String, u32>,
}

/// 요청의 이번 판정 남은 수 — 수량 − 끝남 − 진행 중 지시 − 아직 지시가 없는 예정.
pub fn request_cap(r: &TransferRequest, queued: &BTreeMap<String, u32>) -> u32 {
    r.remaining().saturating_sub(queued.get(&r.id).copied().unwrap_or(0))
}

/// 요청 목록(수행 순서) + 프로파일 + 정책 → 파생 규칙.
pub fn derive(p: &Policy, reqs: &[TransferRequest], ctx: &Ctx) -> Vec<Rule> {
    let mut out = Vec::new();
    let picks: Vec<u16> = ctx.profiles.values().filter(|x| x.role.picks()).map(|x| x.station_id).collect();
    let drops: Vec<u16> = ctx.profiles.values().filter(|x| x.role.drops()).map(|x| x.station_id).collect();
    let weight = |s: u16| ctx.profiles.get(&s).map(|x| x.weight).unwrap_or(0.0);
    let pallet = |s: u16| ctx.profiles.get(&s).is_some_and(|x| x.drop_mode == DropMode::Pallet);
    let inbound_dest = || {
        let mut d = p.inbound_dest.clone();
        if p.inbound_near_drop && d.near.is_none() {
            d.near = ctx.first_drop_station;
        }
        d
    };
    // 1. 요청 — 먼저.
    for r in reqs.iter().filter(|r| r.state.is_open() && request_cap(r, ctx.queued_for_request) > 0) {
        let prio = p.request_priority + r.priority as f32 * 10.0;
        let tag = format!("{} {}", if r.source == super::requests::ReqSource::Host { "상위" } else { "사용자" }, r.kind.label());
        let mut push = |target: u16, action: Action, st: Option<u16>| {
            let mut x = rule(
                format!("req:{}@{target}", r.id),
                format!("{tag} {} → {}", r.id, super::run::describe_action(&action)),
                RuleOrigin::Request,
                Trigger::Ready { require_measured: false },
                action,
                prio,
            );
            x.request_id = Some(r.id.clone());
            x.station = st;
            out.push(x);
        };
        match r.kind {
            ReqKind::Inbound => {
                let from: Vec<u16> = match &r.from {
                    Some(t) => vec![t.id],
                    None => picks.clone(),
                };
                for s in from {
                    let (to, to_auto) = match &r.to {
                        Some(t) => (t.clone(), None),
                        None => (cell0(), Some(inbound_dest())),
                    };
                    push(s, Action::Transfer { from: station(s), to, item: r.item_code, count: 1, from_auto: None, to_auto, pallet_auto: false }, Some(s));
                }
            }
            ReqKind::Outbound => {
                let to: Vec<u16> = match &r.to {
                    Some(t) => vec![t.id],
                    None => drops.clone(),
                };
                for s in to {
                    let (from, from_auto) = match &r.from {
                        Some(t) => (t.clone(), None),
                        None => (cell0(), Some(p.outbound_source.clone())),
                    };
                    push(s, Action::Transfer { from, to: station(s), item: r.item_code, count: 1, from_auto, to_auto: None, pallet_auto: pallet(s) }, Some(s));
                }
            }
            ReqKind::Move => {
                if let (Some(f), Some(t)) = (&r.from, &r.to) {
                    push(f.id, Action::Transfer { from: f.clone(), to: t.clone(), item: r.item_code, count: 1, from_auto: None, to_auto: None, pallet_auto: false }, None);
                }
            }
        }
    }
    // 2. 측정 먼저 · 입고 자동 — PICK 스테이션마다.
    for &s in &picks {
        if p.measure_first {
            let mut x = rule(
                format!("measure-{s}"),
                format!("측정 먼저 — {s}"),
                RuleOrigin::Policy,
                Trigger::Unmeasured { target: station(s) },
                Action::Measure { target: station(s), item: None },
                p.measure_priority + weight(s),
            );
            x.station = Some(s);
            out.push(x);
        }
        if p.inbound_auto {
            let a = Action::Transfer { from: station(s), to: cell0(), item: None, count: 1, from_auto: None, to_auto: Some(inbound_dest()), pallet_auto: false };
            let mut x = rule(format!("in-{s}"), format!("입고 — {s} → 셀"), RuleOrigin::Policy, Trigger::Ready { require_measured: p.measure_first }, a, p.inbound_priority + weight(s));
            x.station = Some(s);
            out.push(x);
        }
    }
    // 3. 출고 자동 — DROP 스테이션마다(요청 없이 가장 오래된 재고).
    if p.outbound_auto {
        for &s in &drops {
            let a = Action::Transfer { from: cell0(), to: station(s), item: None, count: 1, from_auto: Some(p.outbound_source.clone()), to_auto: None, pallet_auto: pallet(s) };
            let mut x = rule(format!("out-{s}"), format!("출고 — 셀 → {s}"), RuleOrigin::Policy, Trigger::Ready { require_measured: false }, a, p.outbound_priority + weight(s));
            x.station = Some(s);
            out.push(x);
        }
    }
    // 4. 빈 시간 정리 — 쉬는 로봇만.
    if p.consolidate && !ctx.idle_robots.is_empty() {
        let a = Action::Transfer {
            from: cell0(),
            to: cell0(),
            item: None,
            count: 1,
            from_auto: Some(CellPick { order: "fewest".into(), ..Default::default() }),
            to_auto: Some(CellPick { same_item_first: true, same_item_only: true, ..Default::default() }),
            pallet_auto: false,
        };
        let mut x = rule(CONSOLIDATE_ID.into(), "빈 시간 정리 — 같은 품목 모으기".into(), RuleOrigin::Policy, Trigger::Ready { require_measured: false }, a, p.consolidate_priority);
        x.robots = ctx.idle_robots.clone();
        out.push(x);
    }
    out
}

/// 기본 규칙 한 벌(`default_rules`)이 만든 규칙 → 프로파일 · 정책. `(프로파일, 끌 규칙 id)`.
pub fn migrate(rules: &[Rule], known: &BTreeMap<u16, StationProfile>) -> (Vec<StationProfile>, Vec<String>) {
    let mut roles: BTreeMap<u16, super::profile::StationRole> = BTreeMap::new();
    let mut off = Vec::new();
    for r in rules.iter().filter(|r| r.enabled) {
        let tail = |p: &str| r.id.strip_prefix(p).and_then(|x| x.parse::<u16>().ok());
        let (id, pick) = if let Some(s) = tail("measure-first-").or_else(|| tail("store-near-out-")) {
            (s, true)
        } else if let Some(s) = tail("ship-") {
            (s, false)
        } else {
            continue;
        };
        use super::profile::StationRole::*;
        let cur = roles.get(&id).copied().or_else(|| known.get(&id).map(|p| p.role));
        let next = match (cur, pick) {
            (Some(Drop), true) | (Some(Pick), false) | (Some(Both), _) => Both,
            (_, true) => Pick,
            (_, false) => Drop,
        };
        roles.insert(id, next);
        off.push(r.id.clone());
    }
    let profiles = roles.into_iter().map(|(id, role)| StationProfile { role, ..known.get(&id).cloned().unwrap_or(StationProfile { station_id: id, ..Default::default() }) }).collect();
    (profiles, off)
}

#[cfg(test)]
mod tests {
    use super::super::profile::StationRole;
    use super::super::requests::{ReqSource, ReqState};
    use super::*;

    fn prof(id: u16, role: StationRole) -> StationProfile {
        StationProfile { station_id: id, role, ..Default::default() }
    }

    fn req(id: &str, kind: ReqKind, qty: u32) -> TransferRequest {
        TransferRequest {
            id: id.into(),
            seq: 1,
            source: ReqSource::Host,
            ext_ref: None,
            kind,
            item_code: Some(2011),
            qty,
            done: 0,
            active: 0,
            failed: 0,
            from: None,
            to: None,
            priority: 2,
            state: ReqState::Open,
            note: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            ended_at: None,
            reason: None,
        }
    }

    #[test]
    fn requests_come_first_per_station_and_policy_per_role() {
        let mut profiles = BTreeMap::new();
        profiles.insert(2101, prof(2101, StationRole::Pick));
        profiles.insert(2103, prof(2103, StationRole::Drop));
        profiles.insert(2105, prof(2105, StationRole::Off));
        let q = BTreeMap::new();
        let ctx = Ctx { profiles: &profiles, first_drop_station: Some(2103), idle_robots: vec![], queued_for_request: &q };
        let rules = derive(&Policy::default(), &[req("RQ-1", ReqKind::Outbound, 2), req("RQ-2", ReqKind::Inbound, 1)], &ctx);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["req:RQ-1@2103", "req:RQ-2@2101", "measure-2101", "in-2101"]);
        let r = &rules[0];
        assert_eq!((r.origin, r.priority, r.request_id.as_deref(), r.station), (RuleOrigin::Request, 1020.0, Some("RQ-1"), Some(2103)));
        let Action::Transfer { to_auto: Some(d), .. } = &rules[1].action else { panic!() };
        assert_eq!(d.near, Some(2103), "inbound stores near the drop station");
        assert!(matches!(rules[3].trigger, Trigger::Ready { require_measured: true }));
        // 출고 자동을 켜면 DROP 스테이션마다 한 규칙.
        let p = Policy { outbound_auto: true, measure_first: false, ..Default::default() };
        let ids: Vec<String> = derive(&p, &[], &ctx).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["in-2101", "out-2103"]);
    }

    #[test]
    fn request_cap_counts_queued_items() {
        let mut r = req("RQ-9", ReqKind::Inbound, 3);
        r.done = 1;
        let mut q = BTreeMap::new();
        assert_eq!(request_cap(&r, &q), 2);
        q.insert("RQ-9".into(), 2);
        assert_eq!(request_cap(&r, &q), 0);
        let profiles = BTreeMap::new();
        let ctx = Ctx { profiles: &profiles, first_drop_station: None, idle_robots: vec![], queued_for_request: &q };
        assert!(derive(&Policy::default(), &[r], &ctx).is_empty(), "nothing left to derive");
    }

    #[test]
    fn consolidate_only_for_idle_robots() {
        let profiles = BTreeMap::new();
        let q = BTreeMap::new();
        let p = Policy { consolidate: true, ..Default::default() };
        assert!(derive(&p, &[], &Ctx { profiles: &profiles, first_drop_station: None, idle_robots: vec![], queued_for_request: &q }).is_empty());
        let r = derive(&p, &[], &Ctx { profiles: &profiles, first_drop_station: None, idle_robots: vec![2], queued_for_request: &q });
        assert_eq!((r[0].id.as_str(), r[0].robots.clone()), (CONSOLIDATE_ID, vec![2]));
    }

    #[test]
    fn default_rule_set_migrates_to_profiles() {
        let c = super::super::default_rules(2102, &[2101, 2003]);
        let (profiles, off) = migrate(&c.rules, &BTreeMap::new());
        assert_eq!(profiles.iter().map(|p| (p.station_id, p.role)).collect::<Vec<_>>(), vec![(2003, StationRole::Pick), (2101, StationRole::Pick), (2102, StationRole::Drop)]);
        assert_eq!(off.len(), 5);
    }
}
