//! `/api/cells/bulk` 병합 계획 — 어떤 셀을 쓰고, 지우고, 건너뛸지 순수 함수로 정한다.
//!
//! 레이아웃 생성기는 원래 "그 구역을 지우고 새로 깐다" 하나뿐이었다. 현장에서는 이미 깔린 셀은 그대로 두고
//! **겹치지 않는 자리만 더 까는** 쪽이 더 잦다 — 그래서 모드를 셋으로 나눈다(기본은 더하기).
//!
//! | 모드 | 지움 | id 중복 | 겹침 |
//! |---|---|---|---|
//! | `append`(기본) | 없음 | 건너뜀(`auto_id` 면 빈 id 로 옮김) | 건너뜀 |
//! | `replace_section(n)` | 그 구역에서 생성분이 이어받지 않는 id | 덮어씀(경고) | 씀(경고) |
//! | `overwrite` | 없음 | 덮어씀(경고) | 씀(경고) |
//!
//! 판정은 **서버가 한다**. 화면도 같은 규칙으로 미리 보지만(`lib/task/layoutGen.ts`), 여러 창이 동시에
//! 고칠 수 있으니 실제 결과는 이 계획이 진실이다.
//!
//! 겹침 판정 지름: 요청에 `diameter` 가 있으면 그 값, 없으면 **생성 셀의 `max(length, width)`**.
//! 셀은 원이지만 슬롯은 length×width 로 저장되고 레이아웃 생성기는 둘 다 지름으로 채운다 — 둘이 다르면
//! 큰 쪽을 쓰는 편이 안전하다(작은 쪽을 쓰면 실제로 닿는 배치를 통과시킨다).
use std::collections::{HashMap, HashSet};

use gr_proto::{CELL_ID_MAX, CellInfo};
use serde_json::{Value as Json, json};

/// 중심 거리 허용 오차(mm) — 좌표를 0.1 mm 로 반올림해 저장하므로 딱 붙은 배치가 겹침으로 잡히지 않게 둔다.
pub const OVERLAP_TOLERANCE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Append,
    ReplaceSection(u16),
    Overwrite,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Append => "append",
            Mode::ReplaceSection(_) => "replace_section",
            Mode::Overwrite => "overwrite",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Added,
    Updated,
    Unchanged,
    Skipped,
    Remapped,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Added => "added",
            Outcome::Updated => "updated",
            Outcome::Unchanged => "unchanged",
            Outcome::Skipped => "skipped",
            Outcome::Remapped => "remapped",
        }
    }
}

/// 요청 줄 하나의 결과. `id` 는 **요청에 온 id**, `new_id` 는 `auto_id` 로 옮긴 id.
#[derive(Clone, Debug)]
pub struct RowPlan {
    pub id: u16,
    pub new_id: Option<u16>,
    pub outcome: Outcome,
    pub reason: Option<String>,
    /// 실제로 쓸 셀(건너뛰거나 그대로면 `None`).
    pub write: Option<CellInfo>,
}

impl RowPlan {
    fn view(&self) -> Json {
        let mut v = json!({ "id": self.id, "outcome": self.outcome.name() });
        if let Some(r) = &self.reason {
            v["reason"] = json!(r);
        }
        if let Some(n) = self.new_id {
            v["new_id"] = json!(n);
        }
        v
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    /// id 가 겹치면 그 구역의 빈 id 로 옮긴다(건너뛰는 대신). 겹침 검사는 그대로 걸린다.
    pub auto_id: bool,
    /// 겹침 판정 지름(mm). 없으면 생성 셀의 `max(length, width)`.
    pub diameter: Option<f32>,
    pub tolerance: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { mode: Mode::Append, auto_id: false, diameter: None, tolerance: OVERLAP_TOLERANCE }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// 요청 순서 그대로.
    pub rows: Vec<RowPlan>,
    /// 구역 교체에서 지울 id.
    pub deletes: Vec<u16>,
    /// 막지 않은 충돌(덮어쓰기·구역 교체) — 응답 `warnings` 로 나간다.
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn writes(&self) -> impl Iterator<Item = &CellInfo> {
        self.rows.iter().filter_map(|r| r.write.as_ref())
    }
    pub fn count(&self, o: Outcome) -> usize {
        self.rows.iter().filter(|r| r.outcome == o).count()
    }
    pub fn rows_view(&self) -> Vec<Json> {
        self.rows.iter().map(RowPlan::view).collect()
    }
}

/// 겹침 판정에 쓸 지름.
fn diameter_of(c: &CellInfo, o: &Options) -> f32 {
    o.diameter.filter(|d| *d > 0.0).unwrap_or_else(|| c.length.max(c.width))
}

/// 남는 셀 중 중심이 가장 가까우면서 지름 안으로 들어온 것.
fn overlap<'a>(c: &CellInfo, placed: &'a [CellInfo], o: &Options) -> Option<(&'a CellInfo, f32)> {
    let limit = diameter_of(c, o) - o.tolerance;
    if limit <= 0.0 || limit.is_nan() {
        return None;
    }
    placed
        .iter()
        .filter(|k| k.id != c.id)
        .map(|k| (k, ((k.position[0] - c.position[0]).powi(2) + (k.position[1] - c.position[1]).powi(2)).sqrt()))
        .filter(|(_, d)| d.is_finite() && *d < limit)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// 그 구역 뒤의 첫 빈 id — 이미 깔린 블록 다음으로 이어 붙인다(중간 구멍을 채우면 번호가 뒤엉킨다).
fn next_free_id(section: u16, want: u16, placed: &[CellInfo], used: &HashSet<u16>) -> Option<u16> {
    let sec_max = placed.iter().filter(|k| k.section == section).map(|k| k.id).max().unwrap_or(0);
    let mut cand = sec_max.max(want).saturating_add(1);
    while cand <= CELL_ID_MAX {
        if !used.contains(&cand) {
            return Some(cand);
        }
        cand += 1;
    }
    None
}

/// 병합 계획. `rows` 는 검증을 마친 요청 줄, `existing` 은 지금 레지스트리의 셀 전부.
pub fn plan(rows: &[CellInfo], existing: &[CellInfo], o: &Options) -> Plan {
    let gen_ids: HashSet<u16> = rows.iter().map(|c| c.id).collect();
    let by_id: HashMap<u16, &CellInfo> = existing.iter().map(|c| (c.id, c)).collect();
    // 구역 교체: 그 구역의 기존 셀은 (생성분이 같은 id 로 이어받는 것만 빼고) 지운다 — 충돌 검사에서도 빠진다.
    let (deletes, wiped): (Vec<u16>, HashSet<u16>) = match o.mode {
        Mode::ReplaceSection(sec) => {
            (existing.iter().filter(|c| c.section == sec && !gen_ids.contains(&c.id)).map(|c| c.id).collect(), existing.iter().filter(|c| c.section == sec).map(|c| c.id).collect())
        }
        _ => (vec![], HashSet::new()),
    };

    // 겹침·id 검사 대상 = 남는 기존 셀 + 이번 배치에서 이미 받아들인 셀(배치 안끼리도 겹치면 안 된다).
    let mut placed: Vec<CellInfo> = existing.iter().filter(|c| !wiped.contains(&c.id)).cloned().collect();
    let mut used: HashSet<u16> = existing.iter().map(|c| c.id).collect();
    let mut out = Plan { deletes, ..Plan::default() };

    for r in rows {
        let mut c = r.clone();
        // 값까지 똑같으면 쓰지 않는다 — PLC 에서 읽어 온 깨끗한 행을 괜히 dirty 로 만들지 않기 위해서다.
        if by_id.get(&c.id).is_some_and(|ex| *ex == &c) {
            out.rows.push(RowPlan { id: r.id, new_id: None, outcome: Outcome::Unchanged, reason: None, write: None });
            continue;
        }
        let mut new_id = None;
        if let Some(ex) = placed.iter().find(|k| k.id == c.id).cloned() {
            let clash = format!("기존 셀 #{} (구간 {}) 와 id 중복", ex.id, ex.section);
            match o.mode {
                Mode::Append if o.auto_id => match next_free_id(c.section, c.id, &placed, &used) {
                    Some(free) => {
                        new_id = Some(free);
                        c.id = free;
                    }
                    None => {
                        out.rows.push(RowPlan { id: r.id, new_id: None, outcome: Outcome::Skipped, reason: Some(format!("{clash} · 구간 {} 에 빈 id 가 없습니다", c.section)), write: None });
                        continue;
                    }
                },
                Mode::Append => {
                    out.rows.push(RowPlan { id: r.id, new_id: None, outcome: Outcome::Skipped, reason: Some(clash), write: None });
                    continue;
                }
                // 덮어쓰기·구역 교체는 id 가 이긴다 — 막지 않고 알리기만 한다.
                _ => out.warnings.push(format!("셀 #{} — {clash} · 덮어씁니다", r.id)),
            }
        }
        if let Some((hit, d)) = overlap(&c, &placed, o) {
            let why = format!("기존 셀 #{} 와 겹침 (중심 거리 {:.0} < 지름 {:.0})", hit.id, d, diameter_of(&c, o));
            if o.mode == Mode::Append {
                out.rows.push(RowPlan { id: r.id, new_id, outcome: Outcome::Skipped, reason: Some(why), write: None });
                continue;
            }
            out.warnings.push(format!("셀 #{} — {why}", c.id));
        }
        // 지금 레지스트리에 그 id 가 있으면 `updated` — 구역 교체로 지워질 행을 같은 id 로 다시 까는 것도 고치는 일이다.
        let outcome = if new_id.is_some() {
            Outcome::Remapped
        } else if used.contains(&c.id) {
            Outcome::Updated
        } else {
            Outcome::Added
        };
        used.insert(c.id);
        placed.retain(|k| k.id != c.id);
        placed.push(c.clone());
        out.rows.push(RowPlan { id: r.id, new_id, outcome, reason: None, write: Some(c) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(id: u16, section: u16, x: f32, y: f32) -> CellInfo {
        CellInfo { use_: true, blend_use: false, id, section, row: 1, col: 1, length: 800.0, width: 800.0, position: [x, y, 100.0] }
    }
    fn opts(mode: Mode) -> Options {
        Options { mode, ..Options::default() }
    }
    fn outcomes(p: &Plan) -> Vec<(u16, &'static str, Option<u16>)> {
        p.rows.iter().map(|r| (r.id, r.outcome.name(), r.new_id)).collect()
    }

    #[test]
    fn append_adds_only_non_conflicting_rows() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0)];
        // #101 은 id 중복, #102 는 기존 셀과 겹침(중심 거리 100), #103 은 멀리 떨어져 있다.
        let rows = vec![cell(101, 1, 5000.0, 5000.0), cell(102, 1, 1100.0, 1000.0), cell(103, 1, 9000.0, 9000.0)];
        let p = plan(&rows, &existing, &opts(Mode::Append));
        assert_eq!(outcomes(&p), vec![(101, "skipped", None), (102, "skipped", None), (103, "added", None)]);
        assert!(p.deletes.is_empty());
        assert_eq!(p.writes().count(), 1);
        assert!(p.rows[0].reason.as_deref().unwrap().contains("id 중복"));
        let why = p.rows[1].reason.as_deref().unwrap();
        assert!(why.contains("#101") && why.contains("100"), "{why}");
        assert_eq!(p.count(Outcome::Skipped), 2);
    }

    #[test]
    fn identical_row_is_unchanged_and_never_written() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0)];
        let p = plan(&existing.clone(), &existing, &opts(Mode::Append));
        assert_eq!(outcomes(&p), vec![(101, "unchanged", None)]);
        assert_eq!(p.writes().count(), 0);
        // 덮어쓰기에서도 같은 값이면 쓰지 않는다.
        let p = plan(&existing.clone(), &existing, &opts(Mode::Overwrite));
        assert_eq!(p.count(Outcome::Unchanged), 1);
    }

    #[test]
    fn auto_id_remaps_to_the_next_free_id_of_the_section() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0), cell(102, 1, 2000.0, 1000.0)];
        let rows = vec![cell(101, 1, 9000.0, 9000.0)];
        let o = Options { auto_id: true, ..opts(Mode::Append) };
        let p = plan(&rows, &existing, &o);
        assert_eq!(outcomes(&p), vec![(101, "remapped", Some(103))]);
        assert_eq!(p.writes().next().unwrap().id, 103);
        // 옮긴 뒤에도 겹침 검사는 그대로 — 자리가 겹치면 건너뛴다.
        let p = plan(&[cell(101, 1, 2050.0, 1000.0)], &existing, &o);
        assert_eq!(p.rows[0].outcome, Outcome::Skipped);
        assert!(p.rows[0].reason.as_deref().unwrap().contains("겹침"));
    }

    #[test]
    fn replace_section_wipes_only_that_section() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0), cell(102, 1, 2000.0, 1000.0), cell(201, 2, 1000.0, 5000.0)];
        let rows = vec![cell(101, 1, 1500.0, 1500.0)];
        let p = plan(&rows, &existing, &opts(Mode::ReplaceSection(1)));
        assert_eq!(p.deletes, vec![102]); // 101 은 생성분이 이어받는다
        assert_eq!(outcomes(&p), vec![(101, "updated", None)]);
        assert_eq!(p.writes().count(), 1);
        // 구역 2 는 건드리지 않고, 그 구역과 겹치면 경고만.
        let p = plan(&[cell(301, 1, 1000.0, 5000.0)], &existing, &opts(Mode::ReplaceSection(1)));
        assert_eq!(p.deletes, vec![101, 102]);
        assert_eq!(p.rows[0].outcome, Outcome::Added);
        assert!(p.warnings[0].contains("겹침"), "{:?}", p.warnings);
    }

    #[test]
    fn overwrite_upserts_conflicting_ids_and_deletes_nothing() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0), cell(102, 1, 2000.0, 1000.0)];
        let rows = vec![cell(101, 1, 1100.0, 1000.0), cell(103, 1, 2050.0, 1000.0)];
        let p = plan(&rows, &existing, &opts(Mode::Overwrite));
        assert_eq!(outcomes(&p), vec![(101, "updated", None), (103, "added", None)]);
        assert!(p.deletes.is_empty());
        assert_eq!(p.writes().count(), 2);
        // 막지 않은 충돌은 경고로 나간다 — #101 은 id 중복, #103 은 #102 와 겹침(자기 자신과는 겹치지 않는다).
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
        assert!(p.warnings[0].contains("id 중복") && p.warnings[1].contains("#103"));
    }

    #[test]
    fn overlap_uses_the_tolerance_and_the_cell_slot_when_no_diameter_is_given() {
        let existing = vec![cell(101, 1, 1000.0, 1000.0)];
        // 중심 거리 799.6 — 지름 800, 오차 0.5 면 한계 799.5 라 통과한다.
        let p = plan(&[cell(102, 1, 1799.6, 1000.0)], &existing, &opts(Mode::Append));
        assert_eq!(p.rows[0].outcome, Outcome::Added);
        // 같은 배치라도 지름을 크게 주면 걸린다.
        let o = Options { diameter: Some(1000.0), ..opts(Mode::Append) };
        assert_eq!(plan(&[cell(102, 1, 1799.6, 1000.0)], &existing, &o).rows[0].outcome, Outcome::Skipped);
        // 오차를 키우면 통과한다.
        let o = Options { diameter: Some(1000.0), tolerance: 300.0, ..opts(Mode::Append) };
        assert_eq!(plan(&[cell(102, 1, 1799.6, 1000.0)], &existing, &o).rows[0].outcome, Outcome::Added);
    }

    #[test]
    fn a_batch_row_also_blocks_a_later_row_that_lands_on_it() {
        let p = plan(&[cell(101, 1, 1000.0, 1000.0), cell(102, 1, 1100.0, 1000.0)], &[], &opts(Mode::Append));
        assert_eq!(outcomes(&p), vec![(101, "added", None), (102, "skipped", None)]);
    }
}
