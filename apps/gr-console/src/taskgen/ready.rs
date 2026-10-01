//! 준비 상태 — 대상(셀 · 스테이션)마다 지금 PICK · DROP · MEASURE 명령을 받을 수 있나와 그 사유.
//! 엔진의 판정 문과 레이아웃 맵 표시(▲ ▼)가 같은 값을 쓴다(`GET /api/taskgen/ready`, mux `ready`).
//!
//! 스테이션은 GRM 인터록을 본다 — GR 은 모든 스테이션 작업을 `CVOK & Req` 가 있어야 내려가고(PL_Task_V2 400),
//! 작업 뒤 `PO.Comp` → `Req 0` 로 인계한다(600). 그래서 다음 작업은 인계가 끝난 뒤(`CVNO 0 & Comp 0`)에만 만든다.
//! `Req` 는 픽 스테이션이면 "화물 있음", 드롭 스테이션이면 "비어 있음" — 어느 쪽인지는 프로파일(역할)이 정한다.
//! 셀은 인터록이 없다 — 미래 재고(진행 중 · 초안 · 예정까지 접은 값)와 Use 만 본다.

use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex, PoisonError};

use serde::Serialize;
use tokio::sync::broadcast;

use super::CellView;
use super::profile::{DropMode, StationProfile, StationRole};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Ready {
    pub ok: bool,
    pub why: Option<String>,
    /// 스테이션 신호 조건식 — 항목마다 지금 충족하나(화면이 `CVOK & Req & ItemExist · CVNO 0 & Comp 0` 를 색으로 그린다).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expr: Vec<Cond>,
}

/// 조건식 한 항목 — `neg` 면 꺼져 있어야 충족(`CVNO 0`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cond {
    pub name: &'static str,
    pub neg: bool,
    pub ok: bool,
}

impl Ready {
    pub fn yes() -> Ready {
        Ready { ok: true, why: None, expr: Vec::new() }
    }
    pub fn note(why: impl Into<String>) -> Ready {
        Ready { ok: true, why: Some(why.into()), expr: Vec::new() }
    }
    pub fn no(why: impl Into<String>) -> Ready {
        Ready { ok: false, why: Some(why.into()), expr: Vec::new() }
    }
    fn with(mut self, expr: Vec<Cond>) -> Ready {
        self.expr = expr;
        self
    }
}

/// 신호 조건식 — PICK · MEASURE: `CVOK & Req & ItemExist & CVNO 0 & Comp 0`, DROP: `CVOK & Req & CVNO 0 & Comp 0`
/// (하나씩 스테이션은 `ItemExist 0` 도). GRM 을 못 읽으면 비운다.
pub fn signal_expr(s: &StationSig, op: &str, mode: DropMode) -> Vec<Cond> {
    if !s.live {
        return Vec::new();
    }
    let on = |name, v: bool| Cond { name, neg: false, ok: v };
    let off = |name, v: bool| Cond { name, neg: true, ok: !v };
    let mut v = vec![on("CVOK", s.cvok), on("Req", s.req)];
    match op {
        "drop" if mode == DropMode::Single => v.push(off("ItemExist", s.item_exist)),
        "drop" => {}
        _ => v.push(on("ItemExist", s.item_exist)),
    }
    v.push(off("CVNO", s.cvno));
    v.push(off("Comp", s.comp));
    v
}

/// 이 대상을 쓰는 작업 — 모든 경로(단일 · 순차 · 스케줄러)의 살아 있는 Task, 또는 아직 원장에 없는 예정.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hold {
    pub robot: u8,
    pub robot_name: String,
    pub op: String,
    /// 원장 상태(`draft` · `submitted` …) 또는 `queued`(예정 큐).
    pub state: String,
    pub order: Option<String>,
    pub task: Option<String>,
}

impl Hold {
    pub fn label(&self) -> String {
        format!("{} {} ({}){}", self.robot_name, self.op, self.state, self.order.as_deref().map(|o| format!(" {o}")).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TargetReady {
    pub kind: String,
    pub id: u16,
    pub pick: Ready,
    pub drop: Ready,
    pub measure: Ready,
    pub item_code: u32,
    pub count: u32,
    pub room: Option<u32>,
    pub holds: Vec<Hold>,
    pub need_item: bool,
    pub hints: Vec<u32>,
    pub since: Option<String>,
    pub role: Option<StationRole>,
    pub drop_mode: Option<DropMode>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReadySnapshot {
    pub at: String,
    pub targets: Vec<TargetReady>,
}

/// 스테이션 신호(GRM `OPCUA.STATION[n]`, `issue::station_live` 한 줄에서).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StationSig {
    pub live: bool,
    pub stale: bool,
    pub why: Option<String>,
    pub cvok: bool,
    pub req: bool,
    pub item_exist: bool,
    pub cvno: bool,
    pub comp: bool,
    pub od: f32,
}

impl From<&crate::issue::station_live::StationLive> for StationSig {
    fn from(l: &crate::issue::station_live::StationLive) -> StationSig {
        StationSig { live: l.live, stale: l.stale, why: l.why.clone(), cvok: l.pi.cvok, req: l.pi.req, item_exist: l.pi.item_exist, cvno: l.po.cvno, comp: l.po.comp, od: l.od }
    }
}

/// 품목 규격 중 적재 판단에 쓰는 것.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemFacts {
    /// 품목 최대 단수(0 = 제한 없음) — 화물의 정보다(스테이션이 아니라).
    pub stack_max: u32,
    pub height: f32,
    pub compression: f32,
    pub od: f32,
    /// 내경(mm) — 로봇 규칙(품목 내경 전담).
    pub inner_dia: f32,
}

/// `n` 개 스택의 높이 — 단마다 눌린 높이(`Height − Compression·위에 얹힌 수`)의 합.
pub fn stack_height(f: &ItemFacts, n: u32) -> f32 {
    (0..n).map(|k| crate::stock::pressed_height(f.height, f.compression, n - 1 - k)).sum()
}

/// 스택 스테이션에 `add_item` 하나를 더 얹을 수 있나 — 같은 품목만, 품목 StackMax, 스테이션 최대 높이.
pub fn stack_room(items: &HashMap<u32, ItemFacts>, cur_item: u32, cur_count: u32, add_item: u32, max_height_mm: f32) -> Result<(), String> {
    if cur_count > 0 && cur_item != 0 && add_item != 0 && cur_item != add_item {
        return Err(format!("다른 품목 {cur_item} 이 쌓여 있음"));
    }
    let code = if add_item != 0 { add_item } else { cur_item };
    let Some(f) = items.get(&code) else {
        return if max_height_mm > 0.0 && cur_count > 0 { Err(format!("품목 {code} 규격 모름 — 스택 높이를 셀 수 없음")) } else { Ok(()) };
    };
    let n = cur_count + 1;
    if f.stack_max > 0 && n > f.stack_max {
        return Err(format!("품목 StackMax {} 도달 (지금 {cur_count})", f.stack_max));
    }
    if max_height_mm > 0.0 {
        let h = stack_height(f, n);
        if h > max_height_mm {
            return Err(format!("얹으면 높이 {h:.0} mm > 최대 {max_height_mm:.0} mm"));
        }
    }
    Ok(())
}

/// 스택 스테이션의 지금 스택 — GRM 이 비었다고 하면(`ItemExist 0`) 콘솔 재고가 아직 남아 있어도 빈 것으로 본다
/// (컨베이어가 실어 간 뒤 트래킹이 코드를 옮기기 전).
pub fn physical_stack(sig: &StationSig, item_code: u32, count: u32) -> (u32, u32) {
    if sig.live && !sig.item_exist { (0, 0) } else { (item_code, count) }
}

/// 트래킹 OD 에 맞는 품목(±15 mm) — 입고 품목을 모를 때 사람이 고를 후보.
pub fn od_hints(items: &HashMap<u32, ItemFacts>, od: f32) -> Vec<u32> {
    if od <= 0.0 {
        return Vec::new();
    }
    let mut v: Vec<(u32, f32)> = items.iter().filter(|(_, f)| f.od > 0.0 && (f.od - od).abs() <= 15.0).map(|(c, f)| (*c, (f.od - od).abs())).collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    v.into_iter().take(5).map(|(c, _)| c).collect()
}

pub struct Inputs<'a> {
    /// 등록된 스테이션 (id, Use).
    pub stations: &'a [(u16, bool)],
    pub sig: &'a BTreeMap<u16, StationSig>,
    pub profiles: &'a BTreeMap<u16, StationProfile>,
    pub cells: &'a [CellView],
    /// 스테이션 미래 재고 (품목, 개수).
    pub station_stock: &'a BTreeMap<u16, (u32, u32)>,
    pub items: &'a HashMap<u32, ItemFacts>,
    pub holds: &'a BTreeMap<u16, Vec<Hold>>,
    /// (스테이션, "pick" | "drop") → 신호가 준비된 시각.
    pub since: &'a HashMap<(u16, &'static str), String>,
}

/// 신호로 본 준비(예약 · 역할 빼고) — 대기 시간(`since`)은 이 값으로 잰다.
pub fn signal_ready(s: &StationSig, op: &str, mode: DropMode) -> Result<(), String> {
    if !s.live {
        return Err(s.why.clone().unwrap_or_else(|| "GRM 에서 못 읽음".into()));
    }
    if s.stale {
        return Err("GRM 값 오래됨".into());
    }
    if !s.cvok {
        return Err("CVOK 0".into());
    }
    if !s.req {
        return Err("Req 0".into());
    }
    match op {
        "drop" if mode == DropMode::Single && s.item_exist => return Err("ItemExist 1 — 하나씩 스테이션이 비지 않음".into()),
        "pick" | "measure" if !s.item_exist => return Err("ItemExist 0 — 화물 없음".into()),
        _ => {}
    }
    if s.cvno || s.comp {
        return Err(format!("앞 작업 인계 중 (CVNO {} · Comp {})", s.cvno as u8, s.comp as u8));
    }
    Ok(())
}

fn station_row(inp: &Inputs, id: u16, use_: bool) -> TargetReady {
    let p = inp.profiles.get(&id);
    let role = p.map(|p| p.role);
    let mode = p.map(|p| p.drop_mode).unwrap_or_default();
    let sig = inp.sig.get(&id).cloned().unwrap_or(StationSig { why: Some("GRM 에서 못 읽음".into()), ..Default::default() });
    let (item_code, count) = inp.station_stock.get(&id).copied().unwrap_or((0, 0));
    let holds = inp.holds.get(&id).cloned().unwrap_or_default();
    let any_hold = holds.first().map(|h| format!("예약: {}", h.label()));
    let gate = |op: &str, does: bool| -> Ready {
        if role == Some(StationRole::Off) {
            return Ready::no("역할 끔");
        }
        if !does {
            return Ready::no(format!("역할 {} — {} 안 함", role.map(|r| r.as_str()).unwrap_or("-"), op.to_uppercase()));
        }
        if !use_ {
            return Ready::no("Use 꺼짐");
        }
        // 스테이션은 한 번에 한 작업 — 인터록 인계가 한 Task 단위다.
        if let Some(h) = any_hold.clone() {
            return Ready::no(h);
        }
        if let Err(why) = signal_ready(&sig, op, mode) {
            return Ready::no(why);
        }
        Ready::yes()
    };
    let pick = gate("pick", role.is_none_or(StationRole::picks));
    let mut drop = gate("drop", role.is_none_or(StationRole::drops));
    if drop.ok {
        drop = match mode {
            DropMode::Single => Ready::yes(),
            DropMode::Stack => {
                let (ci, cn) = physical_stack(&sig, item_code, count);
                match stack_room(inp.items, ci, cn, ci, p.map(|p| p.max_height_mm).unwrap_or(0.0)) {
                    Ok(()) => Ready::note(format!("스택 {cn} 단")),
                    Err(why) => Ready::no(why),
                }
            }
            DropMode::Pallet => Ready::note("팔렛 슬롯은 작성 때 확인"),
        };
    }
    let measure = gate("measure", true).with(signal_expr(&sig, "measure", mode));
    let pick = pick.with(signal_expr(&sig, "pick", mode));
    let drop = drop.with(signal_expr(&sig, "drop", mode));
    let need_item = sig.item_exist && item_code == 0;
    let since = [("pick", pick.ok), ("drop", drop.ok)].iter().filter(|(_, ok)| *ok).filter_map(|(op, _)| inp.since.get(&(id, *op)).cloned()).min();
    TargetReady {
        kind: "station".into(),
        id,
        pick,
        drop,
        measure,
        item_code,
        count,
        room: None,
        hints: if need_item { od_hints(inp.items, sig.od) } else { Vec::new() },
        need_item,
        holds,
        since,
        role,
        drop_mode: p.map(|p| p.drop_mode),
    }
}

fn cell_row(inp: &Inputs, c: &CellView) -> TargetReady {
    let holds = inp.holds.get(&c.id).cloned().unwrap_or_default();
    let pick = if !c.use_ {
        Ready::no("Use 꺼짐")
    } else if c.count == 0 {
        Ready::no("재고 없음")
    } else if c.item == 0 {
        Ready::no("품목을 모름")
    } else {
        Ready::yes()
    };
    let drop = if !c.use_ {
        Ready::no("Use 꺼짐")
    } else {
        match c.room {
            Some(0) => Ready::no("남은 칸 0 (품목 StackMax)"),
            Some(r) => Ready::note(format!("남은 칸 {r}")),
            None => Ready::yes(),
        }
    };
    TargetReady { kind: "cell".into(), id: c.id, pick, drop, measure: Ready::no("셀 측정은 규칙으로"), item_code: c.item, count: c.count, room: c.room, holds, ..Default::default() }
}

pub fn compute(inp: &Inputs) -> Vec<TargetReady> {
    let mut v: Vec<TargetReady> = inp.stations.iter().map(|(id, u)| station_row(inp, *id, *u)).collect();
    v.extend(inp.cells.iter().map(|c| cell_row(inp, c)));
    v
}

/// 신호 준비 시각 갱신 — 켜진 것은 처음 켜진 시각을 지키고, 꺼지면 지운다.
pub fn update_since(since: &mut HashMap<(u16, &'static str), String>, inp_sig: &BTreeMap<u16, StationSig>, profiles: &BTreeMap<u16, StationProfile>, now: &str) {
    for (id, s) in inp_sig {
        let mode = profiles.get(id).map(|p| p.drop_mode).unwrap_or_default();
        for op in ["pick", "drop"] {
            if signal_ready(s, op, mode).is_ok() {
                since.entry((*id, op)).or_insert_with(|| now.to_string());
            } else {
                since.remove(&(*id, op));
            }
        }
    }
}

// ── 변화만 내보내는 허브(mux `ready`) ────────────────────────────────

struct Hub {
    tx: broadcast::Sender<ReadySnapshot>,
    last: Mutex<ReadySnapshot>,
}

static HUB: LazyLock<Hub> = LazyLock::new(|| Hub { tx: broadcast::channel(16).0, last: Mutex::new(ReadySnapshot::default()) });

/// 대상 값이 바뀐 때만 보낸다(`at` 은 비교에서 뺀다).
pub fn publish(s: ReadySnapshot) -> bool {
    let mut last = HUB.last.lock().unwrap_or_else(PoisonError::into_inner);
    if last.targets == s.targets {
        last.at = s.at;
        return false;
    }
    *last = s.clone();
    let _ = HUB.tx.send(s);
    true
}

pub fn snapshot() -> ReadySnapshot {
    HUB.last.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn subscribe() -> broadcast::Receiver<ReadySnapshot> {
    HUB.tx.subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 조건식을 뺀 판정(사유 비교용).
    fn bare(r: Ready) -> Ready {
        Ready { expr: vec![], ..r }
    }

    #[test]
    fn signal_expression_marks_each_term() {
        let s = StationSig { live: true, cvok: true, req: false, item_exist: true, cvno: false, comp: true, ..Default::default() };
        let show = |v: Vec<Cond>| v.iter().map(|c| format!("{}{}{}", c.name, if c.neg { " 0" } else { "" }, if c.ok { "" } else { "✗" })).collect::<Vec<_>>().join(" & ");
        assert_eq!(show(signal_expr(&s, "pick", DropMode::Single)), "CVOK & Req✗ & ItemExist & CVNO 0 & Comp 0✗");
        assert_eq!(show(signal_expr(&s, "drop", DropMode::Single)), "CVOK & Req✗ & ItemExist 0✗ & CVNO 0 & Comp 0✗");
        assert_eq!(show(signal_expr(&s, "drop", DropMode::Stack)), "CVOK & Req✗ & CVNO 0 & Comp 0✗");
        assert!(signal_expr(&StationSig::default(), "pick", DropMode::Single).is_empty(), "GRM 을 못 읽으면 비운다");
    }

    fn sig(cvok: bool, req: bool, item: bool) -> StationSig {
        StationSig { live: true, cvok, req, item_exist: item, ..Default::default() }
    }
    fn prof(id: u16, role: StationRole, mode: DropMode, h: f32) -> StationProfile {
        StationProfile { station_id: id, role, drop_mode: mode, max_height_mm: h, ..Default::default() }
    }

    struct W {
        stations: Vec<(u16, bool)>,
        sig: BTreeMap<u16, StationSig>,
        profiles: BTreeMap<u16, StationProfile>,
        cells: Vec<CellView>,
        stock: BTreeMap<u16, (u32, u32)>,
        items: HashMap<u32, ItemFacts>,
        holds: BTreeMap<u16, Vec<Hold>>,
        since: HashMap<(u16, &'static str), String>,
    }
    impl W {
        fn new() -> W {
            let mut items = HashMap::new();
            items.insert(2011, ItemFacts { stack_max: 4, height: 200.0, compression: 10.0, od: 650.0, inner_dia: 0.0 });
            items.insert(2013, ItemFacts { stack_max: 0, height: 250.0, compression: 0.0, od: 700.0, inner_dia: 0.0 });
            W { stations: vec![], sig: BTreeMap::new(), profiles: BTreeMap::new(), cells: vec![], stock: BTreeMap::new(), items, holds: BTreeMap::new(), since: HashMap::new() }
        }
        fn run(&self) -> Vec<TargetReady> {
            compute(&Inputs {
                stations: &self.stations,
                sig: &self.sig,
                profiles: &self.profiles,
                cells: &self.cells,
                station_stock: &self.stock,
                items: &self.items,
                holds: &self.holds,
                since: &self.since,
            })
        }
        fn st(&self, id: u16) -> TargetReady {
            self.run().into_iter().find(|t| t.kind == "station" && t.id == id).unwrap()
        }
    }

    #[test]
    fn station_pick_needs_req_cvok_item_and_finished_handover() {
        let mut w = W::new();
        w.stations = vec![(2101, true)];
        w.profiles.insert(2101, prof(2101, StationRole::Pick, DropMode::Single, 0.0));
        w.sig.insert(2101, sig(true, false, true));
        assert_eq!(bare(w.st(2101).pick), Ready::no("Req 0"));
        w.sig.insert(2101, sig(true, true, false));
        assert!(!w.st(2101).pick.ok, "no tire");
        w.sig.insert(2101, StationSig { comp: true, ..sig(true, true, true) });
        assert!(w.st(2101).pick.why.unwrap().contains("인계"));
        w.sig.insert(2101, sig(true, true, true));
        let t = w.st(2101);
        assert!(t.pick.ok);
        assert!(!t.drop.ok, "pick-only station");
        assert!(t.need_item, "console does not know the tire");
        w.stock.insert(2101, (2011, 1));
        assert!(!w.st(2101).need_item);
        // 예약이 있으면 안 된다(어느 경로의 Task 든).
        w.holds.insert(2101, vec![Hold { robot: 2, robot_name: "GR2".into(), op: "PICK".into(), state: "running".into(), order: Some("TO-1".into()), task: None }]);
        assert!(w.st(2101).pick.why.unwrap().contains("GR2 PICK"));
        // 신선하지 않으면 안 된다.
        w.holds.clear();
        w.sig.insert(2101, StationSig { stale: true, ..sig(true, true, true) });
        assert_eq!(bare(w.st(2101).pick), Ready::no("GRM 값 오래됨"));
    }

    #[test]
    fn drop_modes_single_stack_pallet() {
        let mut w = W::new();
        w.stations = vec![(2103, true), (2104, true), (2105, true)];
        w.profiles.insert(2103, prof(2103, StationRole::Drop, DropMode::Single, 0.0));
        w.profiles.insert(2104, prof(2104, StationRole::Drop, DropMode::Stack, 600.0));
        w.profiles.insert(2105, prof(2105, StationRole::Drop, DropMode::Pallet, 0.0));
        for id in [2103, 2104, 2105] {
            w.sig.insert(id, sig(true, true, true));
        }
        assert!(!w.st(2103).drop.ok, "single needs an empty station");
        // 스택: 200 mm, 눌림 10 → 2 단 390, 3 단 570, 4 단 740 > 600
        w.stock.insert(2104, (2011, 2));
        assert!(w.st(2104).drop.ok);
        w.stock.insert(2104, (2011, 3));
        assert!(w.st(2104).drop.why.unwrap().contains("높이"));
        // GRM 이 비었다고 하면(실어 간 뒤) 콘솔 재고가 남아 있어도 빈 스택으로 본다.
        w.sig.insert(2104, sig(true, true, false));
        assert_eq!(bare(w.st(2104).drop), Ready::note("스택 0 단"));
        assert!(w.st(2105).drop.ok);
        // Req 가 없으면 방식과 무관하게 안 된다.
        w.sig.insert(2105, sig(true, false, false));
        assert_eq!(bare(w.st(2105).drop), Ready::no("Req 0"));
    }

    #[test]
    fn stack_height_and_item_stack_max() {
        let w = W::new();
        let f = w.items[&2011];
        assert_eq!(stack_height(&f, 1), 200.0);
        assert_eq!(stack_height(&f, 3), 180.0 + 190.0 + 200.0);
        assert_eq!(stack_room(&w.items, 2011, 3, 2011, 0.0), Ok(()), "4th is still within StackMax 4");
        assert!(stack_room(&w.items, 2011, 4, 2011, 0.0).unwrap_err().contains("StackMax"));
        assert!(stack_room(&w.items, 2011, 1, 2013, 0.0).unwrap_err().contains("다른 품목"));
        assert_eq!(stack_room(&w.items, 0, 0, 9999, 500.0), Ok(()), "empty station takes an unknown item");
        assert!(stack_room(&w.items, 9999, 1, 9999, 500.0).is_err(), "cannot count the height of an unknown item");
    }

    #[test]
    fn roles_and_unprofiled_stations() {
        let mut w = W::new();
        w.stations = vec![(2101, true), (2102, true), (2106, false)];
        w.profiles.insert(2102, prof(2102, StationRole::Off, DropMode::Single, 0.0));
        for id in [2101, 2102, 2106] {
            w.sig.insert(id, sig(true, true, true));
        }
        let free = w.st(2101);
        assert!(free.pick.ok && free.role.is_none(), "no profile: signals only");
        assert_eq!(bare(w.st(2102).pick), Ready::no("역할 끔"));
        assert_eq!(bare(w.st(2106).pick), Ready::no("Use 꺼짐"));
    }

    #[test]
    fn cells_use_future_stock() {
        let mut w = W::new();
        w.cells = vec![
            CellView { id: 401, use_: true, item: 2011, count: 2, room: Some(0), ..Default::default() },
            CellView { id: 402, use_: true, ..Default::default() },
            CellView { id: 403, use_: false, item: 2011, count: 1, room: Some(3), ..Default::default() },
        ];
        let r = w.run();
        let c = |id: u16| r.iter().find(|t| t.id == id).unwrap().clone();
        assert!(c(401).pick.ok && !c(401).drop.ok);
        assert!(!c(402).pick.ok && c(402).drop.ok);
        assert_eq!(c(403).pick, Ready::no("Use 꺼짐"));
    }

    #[test]
    fn since_tracks_the_first_ready_moment_and_hints_match_od() {
        let mut since = HashMap::new();
        let mut s = BTreeMap::new();
        s.insert(2101, sig(true, true, true));
        let p = BTreeMap::new();
        update_since(&mut since, &s, &p, "t1");
        update_since(&mut since, &s, &p, "t2");
        assert_eq!(since.get(&(2101, "pick")).map(String::as_str), Some("t1"));
        s.insert(2101, sig(true, false, true));
        update_since(&mut since, &s, &p, "t3");
        assert!(since.is_empty());
        let w = W::new();
        assert_eq!(od_hints(&w.items, 655.0), vec![2011]);
        assert!(od_hints(&w.items, 0.0).is_empty());
    }
}
