//! 두 로봇 작업 영역 스케줄링 — 공유 X 축 위의 구간으로 본다(충돌 방지 인터록에 기대지 않고 겹치지 않게 낸다).
//!
//! GR1/GR2 PLC 의 충돌 방지(읽기 전용 참고, `CL_AntiCollision.scl`, `AntiColChangePos_V4.scl`, `isTaskAvoidReq.scl`):
//! - 축 = X. ID 1(GR1)이 X 작은 쪽, ID 2(GR2)가 큰 쪽이고 GR2 가 우선(`isHighPriorityRobot := ID = 2`).
//! - 회피 간격 = `XLengthFront/Rear`(p11/p12 = 1903) + `AntiColMargin_Avoid`(p16 = 400) + `AntiColMargin_Default`(p13 = 100)
//!   = **2403 mm** (로봇 X 사이). 상대의 현재·목표 위치에서 이만큼 떨어진 곳까지만 가고(목표를 깎는다) 기다린다.
//!   콘솔 기본 간격은 이보다 넉넉한 **5000 mm**(안전), PLC 값보다 작게는 저장하지 못한다(`params::check_plc_floor`).
//!   목표가 상대 영역 안이면 `Task.Status.AvoidReq`(GCS 회피 요청), 60 s 이어지면 교착 알람 1117/1119, 접근 알람 1118.
//! - 고장 한계 = XLength + p13 + `AntiColMargin_Pos`(p15 = 300) + 양쪽 감속거리 → 1110/1111. XAC 거리 센서 → 1108/1109.
//! - 위치 교환: GRM 을 거쳐 `MACHINE.AntiCol.GR[n].Position/TargetPosition/Speed/DecelDist`(`FB_Comm_GRM`).
//!
//! 콘솔 규칙: Task 하나의 영역 = 시작 X(앞 Task 목표, 없으면 지금 X) ~ 목표 X. 짝(PICK→DROP)은 DROP 이 끝날 때까지 두 구간의
//! 합을 잡는다. 다른 로봇이 잡은 구간(지금 X + 진행 중 Task 목표 + 걸린 짝의 DROP 목표)과 **간격 < separation** 이면 겹침.

use serde::{Deserialize, Serialize};

/// 간격 설정 — 기본은 안전값 5000 mm(PLC PARA 합 2403 mm 는 참고·하한). 값은 `params.rs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaConfig {
    /// 두 로봇 X 사이 최소 간격(mm).
    pub separation_mm: f32,
    /// 끄면 영역 검사를 하지 않는다(단일 로봇이면 어차피 안 한다).
    pub enabled: bool,
}

impl Default for AreaConfig {
    fn default() -> Self {
        AreaConfig { separation_mm: crate::params::SAFE_SEPARATION_MM, enabled: true }
    }
}

/// 간격·사용 — 파라미터(`params.rs`) 한 곳에서.
pub fn load(db: &crate::db::Db) -> AreaConfig {
    let p = crate::params::current(db);
    AreaConfig { separation_mm: p.anticol_separation_mm, enabled: p.anticol_enabled }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub lo: f32,
    pub hi: f32,
}

impl Interval {
    pub fn point(x: f32) -> Interval {
        Interval { lo: x, hi: x }
    }
    pub fn span(a: f32, b: f32) -> Interval {
        Interval { lo: a.min(b), hi: a.max(b) }
    }
    pub fn with(self, x: f32) -> Interval {
        Interval { lo: self.lo.min(x), hi: self.hi.max(x) }
    }
    /// 두 구간 사이 거리(겹치면 0).
    pub fn gap(self, o: Interval) -> f32 {
        (self.lo.max(o.lo) - self.hi.min(o.hi)).max(0.0)
    }
    pub fn label(self) -> String {
        format!("{:.0}..{:.0}", self.lo, self.hi)
    }
}

/// 점들의 덮개(없으면 `None`).
pub fn hull(xs: impl IntoIterator<Item = f32>) -> Option<Interval> {
    xs.into_iter().filter(|x| x.is_finite()).fold(None, |acc: Option<Interval>, x| Some(acc.map_or(Interval::point(x), |i| i.with(x))))
}

/// 새 Task 의 영역 — 시작(앞 Task 목표 → 지금 X → 목표 순), 목표, 짝 DROP 목표의 덮개.
pub fn task_area(last_pending_target: Option<f32>, current: Option<f32>, target: f32, pair_drop: Option<f32>) -> Interval {
    let start = last_pending_target.or(current).unwrap_or(target);
    let i = Interval::span(start, target);
    match pair_drop {
        Some(d) => i.with(d),
        None => i,
    }
}

/// 다른 로봇 하나가 잡은 영역.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reservation {
    pub robot: u8,
    pub name: String,
    pub area: Interval,
    /// 진행 중 Task 도 걸린 짝도 없다 — 서 있을 뿐.
    pub idle: bool,
    /// 짝(PICK 나감, DROP 아직)이 걸려 있다.
    pub holds_pair: bool,
    /// 이 로봇의 추가 여유(mm, `params.robot_margin_mm`) — 간격에 더한다.
    #[serde(default)]
    pub margin: f32,
}

/// `committed` = PLC 가 받은(Accepted/Queued/Running) 명령이 있다 — 그 로봇은 목표로 떠나기로 했으므로 **지금 X 는 빼고**
/// 목표들만 잡는다. 두 로봇은 X 에서 서로를 건너지 못하므로 목표가 비켜 있으면 가는 길도 비켜 있다. 뒤 호기의 명령은
/// 앞 호기가 명령을 받은 뒤에야 나간다(사용자 규칙 2026-09-21: "상대로봇 명령 수령 확인 후 뒤 호기 명령 생성").
pub fn reservation(robot: u8, name: &str, current: Option<f32>, pending_targets: &[f32], committed: bool, pair_drop: Option<f32>) -> Option<Reservation> {
    let here = if committed && !pending_targets.is_empty() { None } else { current };
    let area = hull(here.into_iter().chain(pending_targets.iter().copied()).chain(pair_drop))?;
    Some(Reservation { robot, name: name.into(), area, idle: pending_targets.is_empty() && pair_drop.is_none(), holds_pair: pair_drop.is_some(), margin: 0.0 })
}

/// 첫 번째로 겹치는 다른 로봇(없으면 `None`). `sep` 에 내 여유를 더해 넘기고, 상대 여유는 여기서 더한다.
pub fn blocker(mine: Interval, others: &[Reservation], sep: f32) -> Option<&Reservation> {
    others.iter().find(|r| mine.gap(r.area) < sep + r.margin)
}

pub fn wait_reason(r: &Reservation, mine: Interval, sep: f32) -> String {
    format!("영역 대기: {} X {} 사용 중 — 이 Task X {} (간격 {:.0} mm)", r.name, r.area.label(), mine.label(), sep)
}

/// 계획의 이웃한 두 스텝(다른 로봇)의 목표 X 가 간격 안이면 경고 문구 — 화면 정적 검사와 같다.
pub fn static_warning(a_name: &str, a_x: f32, b_name: &str, b_x: f32, sep: f32) -> Option<String> {
    let d = (a_x - b_x).abs();
    (d < sep).then(|| format!("{a_name} X {a_x:.0} 와 {b_name} X {b_x:.0} 거리 {d:.0} < {sep:.0} mm — 영역 대기 예정"))
}

// ---------------------------------------------------------------------------------------------
// AppState 연결(스냅샷 · 원장 · 레지스트리)
// ---------------------------------------------------------------------------------------------

use crate::ledger::{LedgerEntry, Target, TaskState};
use crate::state::{AppState, RobotCtx};

/// 로봇의 지금 X — `OPCUA.STAT.Drive[0].Position`(X 축).
pub fn robot_x(st: &AppState, r: &RobotCtx) -> Option<f32> {
    let stat = st.robot_plc(r).ok()?.decode_path("OPCUA", "STAT")?;
    let v = gr_proto::StatusView::from_json(&stat).ok()?;
    v.drive.first().map(|d| d.position).filter(|x| x.is_finite())
}

fn pending(s: TaskState) -> bool {
    matches!(s, TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running)
}

/// 원장 Task 의 목표 X(대상이 있는 Task 만 — UP 은 X 를 안 바꾼다).
pub fn entry_x(e: &LedgerEntry) -> Option<f32> {
    (e.plc_task.cell.id != 0 && gr_proto::TaskType::from_code(e.plc_task.task_type) != Some(gr_proto::TaskType::Up)).then_some(e.plc_task.position[0])
}

/// 로봇의 진행 중 Task 목표 X(원장 순서).
pub fn pending_targets(r: &RobotCtx) -> Vec<f32> {
    let mut v: Vec<LedgerEntry> = r.ledger.list().into_iter().filter(|e| pending(e.state)).collect();
    v.sort_by_key(|e| e.seq);
    v.iter().filter_map(entry_x).collect()
}

/// PLC 가 받은 명령(Accepted/Queued/Running)이 있나.
pub fn committed(r: &RobotCtx) -> bool {
    r.ledger.list().iter().any(|e| matches!(e.state, TaskState::Accepted | TaskState::Queued | TaskState::Running) && entry_x(e).is_some())
}

/// 대상(셀·스테이션)의 레지스트리 X.
pub fn target_x(st: &AppState, t: &Target) -> Option<f32> {
    match t.kind.as_str() {
        "cell" => st.registry.cell(t.id).ok().flatten().map(|c| c.cell.position[0]),
        "station" => st.registry.station(t.id).ok().flatten().map(|s| s.para.info.position[0]),
        _ => None,
    }
}

/// `me` 를 뺀 로봇들이 잡은 영역. `pair_drop` = 로봇별 걸린 짝의 DROP 목표 X(실행기가 들고 있다).
pub fn others(st: &AppState, me: u8, pair_drop: &std::collections::BTreeMap<u8, f32>) -> Vec<Reservation> {
    let p = crate::params::current(&st.db);
    st.robots
        .iter()
        .filter(|r| r.id != me)
        .filter_map(|r| {
            reservation(r.id, &r.name, robot_x(st, r), &pending_targets(r), committed(r), pair_drop.get(&r.id).copied()).map(|mut x| {
                x.margin = p.margin(r.id);
                x
            })
        })
        .collect()
}

/// 로봇 `r` 이 목표 `x`(짝이면 DROP 목표까지)로 가도 되나 — 막는 로봇과 이 Task 영역, 또는 `None`.
pub fn check(st: &AppState, cfg: &AreaConfig, r: &RobotCtx, x: f32, pair_drop: Option<f32>, pairs: &std::collections::BTreeMap<u8, f32>) -> Option<(Reservation, Interval)> {
    if !cfg.enabled || st.robots.len() < 2 {
        return None;
    }
    let mine = task_area(pending_targets(r).last().copied(), robot_x(st, r), x, pair_drop);
    let os = others(st, r.id, pairs);
    let own = crate::params::current(&st.db).margin(r.id);
    blocker(mine, &os, cfg.separation_mm + own).cloned().map(|b| (b, mine))
}

/// 가장 빠듯한 상대 — `gap − 필요 간격` 이 가장 작은 로봇과 (간격, 필요 간격). 막힘 판정은 `blocker` 와 같다.
pub fn tightest(mine: Interval, others: &[Reservation], sep: f32) -> Option<(&Reservation, f32, f32)> {
    others.iter().map(|r| (r, mine.gap(r.area), sep + r.margin)).min_by(|a, b| (a.1 - a.2).total_cmp(&(b.1 - b.2)))
}

/// 화면용 한 로봇 — 지금 X 와 다른 로봇이 보는 이 로봇의 영역.
#[derive(Clone, Debug, Serialize)]
pub struct RobotAreaView {
    pub id: u8,
    pub name: String,
    pub x: Option<f32>,
    pub area: Option<Interval>,
    pub idle: bool,
    pub holds_pair: bool,
    pub margin: f32,
}

/// 화면용 판정 — `robot` 이 목표 `x` 로 지금 나갈 수 있나(제출 게이트 `check` 와 같은 입력).
#[derive(Clone, Debug, Serialize)]
pub struct CheckView {
    pub robot: u8,
    pub x: f32,
    pub mine: Interval,
    /// 가장 빠듯한 상대(막히지 않았어도).
    pub nearest: Option<u8>,
    pub gap: Option<f32>,
    /// 그 상대와 필요한 간격 = separation + 양쪽 여유.
    pub need: Option<f32>,
    pub blocked: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AreaView {
    pub separation_mm: f32,
    pub enabled: bool,
    /// 검사가 실제로 도는가(사용 + 로봇 둘 이상).
    pub active: bool,
    /// 간격 하한 = PLC 계산 간격(두 로봇 중 큰 값), 못 읽으면 없음.
    pub plc_min: Option<f64>,
    pub robots: Vec<RobotAreaView>,
    pub check: Option<CheckView>,
}

pub fn view(st: &AppState, robot: Option<u8>, x: Option<f32>) -> Result<AreaView, crate::error::ApiError> {
    let cfg = load(&st.db);
    let p = crate::params::current(&st.db);
    let active = cfg.enabled && st.robots.len() >= 2;
    let robots = st
        .robots
        .iter()
        .map(|r| {
            let x = robot_x(st, r);
            let res = reservation(r.id, &r.name, x, &pending_targets(r), committed(r), None);
            RobotAreaView {
                id: r.id,
                name: r.name.clone(),
                x,
                area: res.as_ref().map(|v| v.area),
                idle: res.as_ref().is_none_or(|v| v.idle),
                holds_pair: res.as_ref().is_some_and(|v| v.holds_pair),
                margin: p.margin(r.id),
            }
        })
        .collect();
    let check = match (robot, x.filter(|v| v.is_finite())) {
        (Some(id), Some(x)) => {
            let r = st.robot(Some(id))?;
            let mine = task_area(pending_targets(r).last().copied(), robot_x(st, r), x, None);
            let os = others(st, r.id, &Default::default());
            let sep = cfg.separation_mm + p.margin(r.id);
            let t = tightest(mine, &os, sep);
            let reason = if active { blocker(mine, &os, sep).map(|b| wait_reason(b, mine, cfg.separation_mm)) } else { None };
            Some(CheckView { robot: id, x, mine, nearest: t.map(|v| v.0.robot), gap: t.map(|v| v.1), need: t.map(|v| v.2), blocked: reason.is_some(), reason })
        }
        _ => None,
    };
    Ok(AreaView { separation_mm: cfg.separation_mm, enabled: cfg.enabled, active, plc_min: crate::taskgen::routes::plc_max(st).filter(|v| *v > 0.0), robots, check })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEP: f32 = 2403.0;

    #[test]
    fn robot_margins_widen_the_gap() {
        let mut other = reservation(2, "GR2", Some(10000.0), &[], false, None).unwrap();
        let mine = Interval::span(5000.0, 7500.0);
        assert!(blocker(mine, std::slice::from_ref(&other), SEP).is_none(), "gap 2500 >= 2403");
        other.margin = 200.0;
        assert!(blocker(mine, std::slice::from_ref(&other), SEP).is_some(), "2500 < 2403 + 200");
        other.margin = 0.0;
        assert!(blocker(mine, std::slice::from_ref(&other), SEP + 100.0).is_some(), "own margin");
    }

    #[test]
    fn default_separation_is_the_safe_value() {
        assert_eq!(AreaConfig::default().separation_mm, 5000.0);
    }

    #[test]
    fn intervals_and_gaps() {
        let a = Interval::span(7500.0, 4000.0);
        assert_eq!((a.lo, a.hi), (4000.0, 7500.0));
        assert_eq!(a.gap(Interval::point(9000.0)), 1500.0);
        assert_eq!(a.gap(Interval::span(5000.0, 6000.0)), 0.0);
        assert_eq!(hull([3.0, 1.0, f32::NAN, 2.0]), Some(Interval { lo: 1.0, hi: 3.0 }));
        assert_eq!(hull([]), None);
    }

    #[test]
    fn task_area_starts_at_the_previous_target_and_covers_the_pair() {
        // 쉬는 로봇: 지금 X → 목표
        assert_eq!(task_area(None, Some(1000.0), 4000.0, None), Interval { lo: 1000.0, hi: 4000.0 });
        // 대기열이 있으면 앞 Task 목표에서 출발
        assert_eq!(task_area(Some(6000.0), Some(1000.0), 4000.0, None), Interval { lo: 4000.0, hi: 6000.0 });
        // PICK 은 짝 DROP 목표까지 잡는다
        assert_eq!(task_area(None, Some(1000.0), 4000.0, Some(8000.0)), Interval { lo: 1000.0, hi: 8000.0 });
        // 위치를 모르면 목표 한 점
        assert_eq!(task_area(None, None, 4000.0, None), Interval::point(4000.0));
    }

    #[test]
    fn reservation_and_blocking() {
        // GR2: 지금 12000, 대기 Task 목표 9000 → 9000..12000
        let gr2 = reservation(2, "GR2", Some(12000.0), &[9000.0], false, None).unwrap();
        assert!(!gr2.idle);
        let mine = Interval::span(2000.0, 6000.0);
        assert!(blocker(mine, std::slice::from_ref(&gr2), SEP).is_none(), "gap 3000 >= 2403");
        let mine = Interval::span(2000.0, 7000.0);
        let b = blocker(mine, std::slice::from_ref(&gr2), SEP).unwrap();
        assert!(wait_reason(b, mine, SEP).contains("GR2 X 9000..12000"));
        // 걸린 짝의 DROP 목표도 잡는다
        let holding = reservation(2, "GR2", Some(12000.0), &[], false, Some(6000.0)).unwrap();
        assert!(holding.holds_pair && !holding.idle && holding.area.lo == 6000.0);
    }

    /// 앞 호기가 명령을 받으면(committed) 지금 X 는 빼고 목표만 잡는다 — 뒤 호기가 그 뒤에 나갈 수 있다.
    #[test]
    fn committed_robot_reserves_only_its_targets() {
        let mine = Interval::span(4000.0, 7000.0);
        let parked = reservation(2, "GR2", Some(8000.0), &[12000.0], false, None).unwrap();
        assert!(blocker(mine, std::slice::from_ref(&parked), SEP).is_some(), "not yet accepted: still at 8000");
        let leaving = reservation(2, "GR2", Some(8000.0), &[12000.0], true, None).unwrap();
        assert_eq!(leaving.area, Interval::point(12000.0));
        assert!(blocker(mine, std::slice::from_ref(&leaving), SEP).is_none());
        // 받은 명령의 목표가 내 쪽이면 여전히 막힌다
        let coming = reservation(2, "GR2", Some(12000.0), &[8000.0], true, None).unwrap();
        assert!(blocker(mine, std::slice::from_ref(&coming), SEP).is_some());
    }

    /// 화면 판정은 게이트와 같은 결론 — 오늘 현장: GR1 X 1500 에 서 있고 GR2 짝이 6305..10301.
    #[test]
    fn tightest_matches_blocker() {
        let gr1 = reservation(1, "GR1", Some(1500.0), &[], false, None).unwrap();
        let mine = Interval::span(6305.0, 10301.0);
        let (r, gap, need) = tightest(mine, std::slice::from_ref(&gr1), 5000.0).unwrap();
        assert_eq!((r.robot, gap, need), (1, 4805.0, 5000.0));
        assert!(blocker(mine, std::slice::from_ref(&gr1), 5000.0).is_some());
        assert!(blocker(mine, std::slice::from_ref(&gr1), 4800.0).is_none());
        assert!(tightest(mine, &[], 5000.0).is_none());
    }

    #[test]
    fn static_warning_for_neighbouring_steps() {
        assert!(static_warning("GR1", 5000.0, "GR2", 6000.0, SEP).unwrap().contains("거리 1000"));
        assert!(static_warning("GR1", 5000.0, "GR2", 9000.0, SEP).is_none());
    }
}
