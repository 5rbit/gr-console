//! 컨베이어 화물 코드 트래킹 — 스테이션에 올린 화물(재고 항목: 품목 코드 + 개수)이 컨베이어를 따라
//! 다음 스테이션으로 가면 재고 항목도 같이 옮긴다. GRM 의 `Tracking`(OD·TaskOffset) 과는 별개로, PLC 에는
//! 품목 코드를 따라가는 자리가 없어 콘솔이 GRM `Interlock.PI.ItemExist` 의 변화로 판정한다.
//!
//! 연결(A → B)은 **GRM 실제 파라미터**로 판정한다: `A.ConnectionNext = B 슬롯` 또는 `B.ConnectionPrev = A 슬롯`
//! (GRM `FB_Station` 은 도착 쪽의 ConnectionPrev 로 트래킹을 넘긴다) + 설정 `[conveyor] extra_links`
//! (GRM 에 없는 연결, 품목 코드만 — 테스트 베드 2003 → 2102). 콘솔 레지스트리 값은 쓰지 않는다 —
//! 현장 레지스트리가 GRM 과 달랐다(2026-09-21: 2102 next 2 / 2101 prev 1 vs GRM 2102 next 1 / 2101 prev 2).
//!
//! 규칙(한 스테이션 = 재고 한 칸):
//! - **도착**: B 의 ItemExist 가 켜지면(상승) B 로 이어진 앞 스테이션 A 중 재고가 있고 지금 ItemExist 가
//!   꺼진 것(여럿이면 가장 최근에 꺼진 것)의 항목을 B 로 옮긴다. 늦게 도착해도(컨베이어가 오래 멈춰도)
//!   잃지 않는다.
//! - **반출**: 들어오는 연결은 있고 나가는 연결은 없는 스테이션(라인 끝)의 ItemExist 가 꺼지면 그 재고를
//!   지운다. 연결이 하나도 없는 스테이션은 지우지 않는다(어디로 갔는지 모른다).
//! - **로봇 작업은 건드리지 않는다**: 그 스테이션을 대상으로 한 진행 중 Task 가 있거나 끝난 지
//!   `TASK_GRACE` 가 안 됐으면 그 변화는 로봇 PICK/DROP 이다 — 재고는 원장 완료 반영(`Stock::apply_task`)이
//!   맡는다. GRM `PO.CVNO` 는 쓰지 않는다: 로봇의 마지막 인터록 스테이션에 계속 켜져 있을 수 있다
//!   (현장 2102 가 로봇 없이 CVNO = TRUE 였다).
//! - 센서 떨림: 같은 값이 연속 `STABLE_POLLS` 번 읽혀야 변화로 본다. GRM 스냅샷이 오래됐거나(3 s) 끊기면
//!   상태를 버리고 다시 배운다(끊김 동안의 변화로 옮기지 않는다).
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::issue::station_live::{StationLive, station_live_rows};
use crate::state::AppState;

const POLL: Duration = Duration::from_millis(250);
const STABLE_POLLS: u8 = 2;
/// 끝난 Task 의 스테이션을 로봇 작업으로 더 보는 시간(완료 뒤 센서가 늦게 바뀌는 경우).
const TASK_GRACE: Duration = Duration::from_secs(5);

/// 한 번 읽은 스테이션 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub id: u16,
    /// 다음 스테이션 Id(`links`). 0 = 라인 끝, None = 연결 없음(모름).
    pub next: Option<u16>,
    pub item: bool,
    /// 이 스테이션 대상 진행 중(또는 방금 끝난) Task.
    pub robot: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Move { from: u16, to: u16 },
    Clear { from: u16 },
}

#[derive(Clone, Copy, Debug, Default)]
struct Deb {
    stable: bool,
    cand: bool,
    n: u8,
}

#[derive(Default)]
pub struct Tracker {
    deb: HashMap<u16, Deb>,
    /// ItemExist 가 마지막으로 꺼진 차례(단조 증가 카운터) — 여러 앞 스테이션 중 최근 것을 고른다.
    fell_at: HashMap<u16, u64>,
    tick: u64,
}

impl Tracker {
    /// 스냅샷을 못 믿을 때 — 배운 상태를 버린다.
    pub fn reset(&mut self) {
        self.deb.clear();
    }

    /// 한 번 읽은 값으로 한 걸음. `has_stock` = 그 스테이션에 재고 항목(개수 > 0)이 있는가.
    pub fn step(&mut self, samples: &[Sample], has_stock: &dyn Fn(u16) -> bool) -> Vec<Action> {
        self.tick += 1;
        let mut rose = Vec::new();
        let mut fell = Vec::new();
        for s in samples {
            let Some(d) = self.deb.get_mut(&s.id) else {
                // 처음 보는 값은 배우기만 한다(재기동 직후 켜져 있는 것을 도착으로 보지 않는다).
                self.deb.insert(s.id, Deb { stable: s.item, cand: s.item, n: STABLE_POLLS });
                continue;
            };
            if s.item == d.cand {
                d.n = d.n.saturating_add(1);
            } else {
                d.cand = s.item;
                d.n = 1;
            }
            if d.n >= STABLE_POLLS && d.cand != d.stable {
                d.stable = d.cand;
                if d.stable { rose.push(s) } else { fell.push(s) }
            }
        }
        let mut out = Vec::new();
        for s in &fell {
            self.fell_at.insert(s.id, self.tick);
            if !s.robot && s.next == Some(0) && has_stock(s.id) {
                out.push(Action::Clear { from: s.id });
            }
        }
        for b in &rose {
            if b.robot {
                continue;
            }
            let from = samples
                .iter()
                .filter(|a| a.id != b.id && a.next == Some(b.id) && !a.robot && has_stock(a.id))
                .filter(|a| self.deb.get(&a.id).is_some_and(|d| !d.stable))
                .max_by_key(|a| self.fell_at.get(&a.id).copied().unwrap_or(0));
            if let Some(a) = from {
                out.push(Action::Move { from: a.id, to: b.id });
            }
        }
        out
    }
}

/// GRM 연결 `(id, prev 슬롯, next 슬롯)` → 다음 스테이션 Id. 슬롯 = `id MOD 100`.
/// 나가는 연결: 추가 연결 `[A, B]` > `A.next = slot(B)` 또는 `B.prev = slot(A)`. 없으면 들어오는 연결(추가 연결
/// 포함)이 있을 때만 라인 끝(0), 둘 다 없으면 None. 추가 연결은 양 끝이 `rows` 에 있을 때만 쓴다.
pub fn links(rows: &[(u16, u8, u8)], extra: &[[u16; 2]]) -> HashMap<u16, Option<u16>> {
    let slot = |id: u16| (id % 100) as u8;
    let known = |id: u16| rows.iter().any(|&(x, _, _)| x == id);
    let extra: Vec<[u16; 2]> = extra.iter().copied().filter(|&[a, b]| a != b && known(a) && known(b)).collect();
    rows.iter()
        .map(|&(a, _, a_next)| {
            let grm = rows.iter().find(|&&(b, b_prev, _)| b != a && ((a_next != 0 && slot(b) == a_next) || (b_prev != 0 && b_prev == slot(a)))).map(|&(b, _, _)| b);
            let out = extra.iter().find(|&&[f, _]| f == a).map(|&[_, t]| t).or(grm);
            let incoming = extra.iter().any(|&[_, t]| t == a)
                || rows.iter().any(|&(c, _, c_next)| c != a && c_next != 0 && c_next == slot(a))
                || rows.iter().find(|&&(x, _, _)| x == a).is_some_and(|&(_, a_prev, _)| a_prev != 0 && rows.iter().any(|&(c, _, _)| c != a && slot(c) == a_prev));
            (a, out.or(incoming.then_some(0)))
        })
        .collect()
}

/// GRM 에서 찾은 스테이션이 하나 이상이고, 찾은 것이 모두 연결·최신이면 믿는다(못 찾은 스테이션은 빼고 본다).
fn usable(rows: &[StationLive]) -> bool {
    let live: Vec<_> = rows.iter().filter(|r| r.live).collect();
    !live.is_empty() && live.iter().all(|r| !r.stale)
}

/// 진행 중이거나 끝난 지 `TASK_GRACE` 안 된 Task 의 대상 스테이션.
fn busy_stations(st: &AppState) -> HashSet<u16> {
    let now = time::OffsetDateTime::now_utc();
    let recent = |at: &Option<String>| at.as_deref().and_then(crate::ledger::parse_rfc3339).is_some_and(|t| now - t < time::Duration::try_from(TASK_GRACE).unwrap_or(time::Duration::ZERO));
    st.robots.iter().flat_map(|r| r.ledger.list()).filter(|e| e.state.is_active() || (e.state.is_terminal() && recent(&e.ended_at))).map(|e| e.plc_task.cell.id).filter(|&id| id != 0).collect()
}

fn apply(st: &AppState, a: &Action) -> Result<(), crate::error::ApiError> {
    match *a {
        Action::Move { from, to } => {
            let Some(e) = st.stock.get(from)? else { return Ok(()) };
            let note = st.stock.get(to)?.map(|t| t.note).unwrap_or_default();
            st.stock.set(to, e.item_code, e.count, &note, &format!("CV {from} → {to}"))?;
            st.stock.remove(from)?;
            tracing::info!("conveyor: item {} x{} {from} → {to}", e.item_code, e.count);
        }
        Action::Clear { from } => {
            if let Some(e) = st.stock.get(from)? {
                st.stock.remove(from)?;
                tracing::info!("conveyor: item {} x{} left line at {from}", e.item_code, e.count);
            }
        }
    }
    Ok(())
}

/// GRM 이 있을 때만 돈다.
pub fn spawn(st: AppState) {
    if st.grm_plc().is_none() {
        return;
    }
    if !st.cfg.conveyor.extra_links.is_empty() {
        tracing::info!("conveyor: extra links {:?}", st.cfg.conveyor.extra_links);
    }
    tokio::spawn(async move {
        let mut t = Tracker::default();
        let mut tick = tokio::time::interval(POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Ok(rows) = station_live_rows(&st) else {
                t.reset();
                continue;
            };
            crate::issue::station_live::publish(&rows);
            if !usable(&rows) {
                t.reset();
                continue;
            }
            let busy = busy_stations(&st);
            let samples: Vec<Sample> = rows.iter().filter(|r| r.live).map(|r| Sample { id: r.id, next: r.next_id, item: r.pi.item_exist, robot: busy.contains(&r.id) }).collect();
            let stock: HashSet<u16> = match st.stock.list() {
                Ok(l) => l.into_iter().filter(|e| e.count > 0).map(|e| e.cell_id).collect(),
                Err(_) => continue,
            };
            for a in t.step(&samples, &|id| stock.contains(&id)) {
                if let Err(e) = apply(&st, &a) {
                    tracing::warn!("conveyor: {a:?}: {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: u16, next: u16, item: bool) -> Sample {
        Sample { id, next: Some(next), item, robot: false }
    }

    /// 같은 값을 안정될 만큼 여러 번 먹인다.
    fn feed(t: &mut Tracker, v: &[Sample], stock: &[u16]) -> Vec<Action> {
        let mut out = Vec::new();
        for _ in 0..STABLE_POLLS {
            out.extend(t.step(v, &|id| stock.contains(&id)));
        }
        out
    }

    #[test]
    fn item_code_follows_the_conveyor_and_leaves_at_the_end() {
        // 2102 → 2101 → 끝
        let mut t = Tracker::default();
        assert!(feed(&mut t, &[s(2102, 2101, true), s(2101, 0, false)], &[2102]).is_empty(), "first read only learns");
        // 2102 에서 빠지고 아직 2101 에 안 옴 — 옮기지 않는다(잃지도 않는다)
        assert!(feed(&mut t, &[s(2102, 2101, false), s(2101, 0, false)], &[2102]).is_empty());
        // 2101 도착 → 옮김
        assert_eq!(feed(&mut t, &[s(2102, 2101, false), s(2101, 0, true)], &[2102]), vec![Action::Move { from: 2102, to: 2101 }]);
        // 라인 끝에서 빠짐 → 지움
        assert_eq!(feed(&mut t, &[s(2102, 2101, false), s(2101, 0, false)], &[2101]), vec![Action::Clear { from: 2101 }]);
    }

    #[test]
    fn robot_work_and_flicker_do_not_move_stock() {
        let mut t = Tracker::default();
        feed(&mut t, &[s(2102, 2101, true), s(2101, 0, false)], &[2102]);
        // 한 번만 튄 값은 무시
        assert!(t.step(&[s(2102, 2101, false), s(2101, 0, true)], &|id| id == 2102).is_empty());
        assert!(feed(&mut t, &[s(2102, 2101, true), s(2101, 0, false)], &[2102]).is_empty());
        // 로봇이 2101 에 DROP 하는 중(ItemExist 상승) — 앞 스테이션 재고를 끌어오지 않는다
        let busy = Sample { robot: true, ..s(2101, 0, true) };
        assert!(feed(&mut t, &[s(2102, 2101, false), busy.clone()], &[2102]).is_empty());
        // 로봇이 라인 끝에서 PICK(빠짐) — 원장이 맡는다
        let picked = Sample { robot: true, ..s(2101, 0, false) };
        assert!(feed(&mut t, &[s(2102, 2101, false), picked], &[2101]).is_empty());
    }

    #[test]
    fn nothing_to_move_without_upstream_stock_or_when_upstream_still_occupied() {
        let mut t = Tracker::default();
        feed(&mut t, &[s(2102, 2101, true), s(2101, 0, false)], &[]);
        assert!(feed(&mut t, &[s(2102, 2101, true), s(2101, 0, true)], &[2102]).is_empty(), "2102 still holds its tire");
        let mut t = Tracker::default();
        feed(&mut t, &[s(2102, 2101, true), s(2101, 0, false)], &[]);
        feed(&mut t, &[s(2102, 2101, false), s(2101, 0, false)], &[]);
        assert!(feed(&mut t, &[s(2102, 2101, false), s(2101, 0, true)], &[]).is_empty(), "no stock upstream");
    }

    #[test]
    fn merge_takes_the_most_recent_departure() {
        let mut t = Tracker::default();
        let base = [s(2103, 2101, true), s(2102, 2101, true), s(2101, 0, false)];
        feed(&mut t, &base, &[2102, 2103]);
        feed(&mut t, &[s(2103, 2101, false), s(2102, 2101, true), s(2101, 0, false)], &[2102, 2103]);
        feed(&mut t, &[s(2103, 2101, false), s(2102, 2101, false), s(2101, 0, false)], &[2102, 2103]);
        assert_eq!(feed(&mut t, &[s(2103, 2101, false), s(2102, 2101, false), s(2101, 0, true)], &[2102, 2103]), vec![Action::Move { from: 2102, to: 2101 }]);
    }

    #[test]
    fn links_follow_grm_prev_or_next() {
        // 현장 GRM(2026-09-21): 2102 next 1, 2101 prev 2, 2003(ConvNo 2103) 연결 없음
        let site = links(&[(2101, 2, 0), (2102, 0, 1), (2003, 0, 0), (2021, 0, 0)], &[]);
        assert_eq!(site[&2102], Some(2101));
        assert_eq!(site[&2101], Some(0), "line end — has an incoming link");
        assert_eq!(site[&2003], None, "no link: never cleared, never moved");
        // 테스트 베드 2103 → 2102 → 2101: 2003 next 2 만 넣어도, 2102 prev 3 만 넣어도 된다
        let by_next = links(&[(2101, 2, 0), (2102, 0, 1), (2003, 0, 2)], &[]);
        let by_prev = links(&[(2101, 2, 0), (2102, 3, 1), (2003, 0, 0)], &[]);
        for m in [by_next, by_prev] {
            assert_eq!((m[&2003], m[&2102], m[&2101]), (Some(2102), Some(2101), Some(0)));
        }
        // 자기 자신을 가리키는 값(옛 레지스트리 2102 next 2)은 연결이 아니다
        assert_eq!(links(&[(2102, 0, 2)], &[])[&2102], None);
    }

    #[test]
    fn extra_link_bridges_the_test_bed_without_grm() {
        // 현장 GRM 그대로 + 설정 [[2003, 2102]]
        let rows = [(2101, 2, 0), (2102, 0, 1), (2003, 0, 0), (2021, 0, 0)];
        let m = links(&rows, &[[2003, 2102]]);
        assert_eq!((m[&2003], m[&2102], m[&2101], m[&2021]), (Some(2102), Some(2101), Some(0), None));
        // 모르는 Id·자기 자신은 무시
        let m = links(&rows, &[[2003, 2003], [2099, 2102], [2003, 2199]]);
        assert_eq!(m[&2003], None);
    }

    #[test]
    fn test_bed_2103_2102_2101_hands_the_code_down_the_line() {
        let m = links(&[(2101, 2, 0), (2102, 0, 1), (2003, 0, 2)], &[]);
        let at = |a: bool, b: bool, c: bool| {
            vec![Sample { id: 2003, next: m[&2003], item: a, robot: false }, Sample { id: 2102, next: m[&2102], item: b, robot: false }, Sample { id: 2101, next: m[&2101], item: c, robot: false }]
        };
        let mut t = Tracker::default();
        feed(&mut t, &at(true, false, false), &[2003]); // DROP 완료 뒤 2103 에 화물
        feed(&mut t, &at(false, false, false), &[2003]);
        assert_eq!(feed(&mut t, &at(false, true, false), &[2003]), vec![Action::Move { from: 2003, to: 2102 }]);
        feed(&mut t, &at(false, false, false), &[2102]);
        assert_eq!(feed(&mut t, &at(false, false, true), &[2102]), vec![Action::Move { from: 2102, to: 2101 }]);
        // 2101 에서 로봇 PICK — 진행 중 Task 라 그대로 두고 원장 완료가 뺀다
        let mut picked = at(false, false, false);
        picked[2].robot = true;
        assert!(feed(&mut t, &picked, &[2101]).is_empty());
    }
}
