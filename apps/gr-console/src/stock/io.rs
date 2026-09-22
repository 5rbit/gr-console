//! 재고 파일 입출력 — `Stock` 시트(CellId · ItemCode · Count · Note). 셀과 스테이션(컨베이어 화물 코드) 모두.
//!
//! - 내보내기: `GET /api/stock/export.xlsx` (재고만), 통합 `GET /api/registry/export.xlsx` 에도 같은 시트가 붙는다.
//! - 가져오기: `POST /api/stock/import-file?dry_run=1&mode=merge|replace`
//!   - merge(기본): 파일에 있는 자리만 바꾼다. `Count 0` = 그 자리 비움.
//!   - replace: 파일에 없는 자리는 비운다(파일 = 재고 전체). 오류 난 줄의 자리는 지우지 않는다.
//!   - 통합 레지스트리 가져오기는 `Stock` 시트가 있을 때만 merge 로 같이 적용한다.
//! - 줄마다 결과 하나: added · updated(비움 포함) · unchanged · skipped(= errors), replace 는 removed 도.
//! - 검증: 등록된 셀·스테이션인가, 파일 안 중복, Count > 0 이면 등록 품목인가, 품목 StackMax 를 넘는가.
use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::StockEntry;
use crate::error::ApiError;
use crate::registry::xlsx::{RowError, StockExportRow, StockRow, StockSheet};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Merge,
    Replace,
}

impl Mode {
    pub fn parse(s: Option<&str>) -> Mode {
        if s.is_some_and(|v| v.eq_ignore_ascii_case("replace")) { Mode::Replace } else { Mode::Merge }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct StockCounts {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub removed: usize,
    pub errors: Vec<RowError>,
}

/// 한 자리에 할 일.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Set(StockRow),
    Remove(u16),
}

/// 순수 계획 — 저장은 하지 않는다. `known` = 등록된 셀·스테이션인가, `item` = 품목 코드 → (등록됨, StackMax).
pub fn plan(file: &StockSheet, current: &[StockEntry], mode: Mode, known: &dyn Fn(u16) -> bool, item: &dyn Fn(u32) -> Option<u32>) -> (StockCounts, Vec<Op>) {
    let mut c = StockCounts { errors: file.errors.clone(), ..Default::default() };
    let mut ops = Vec::new();
    let cur: HashMap<u16, &StockEntry> = current.iter().filter(|e| e.count > 0).map(|e| (e.cell_id, e)).collect();
    // 파일이 언급한 자리(오류 줄 포함) — replace 에서 지우지 않는다.
    let mut touched: HashSet<u16> = HashSet::new();
    let mut seen: HashMap<u16, usize> = HashMap::new();
    let err = |c: &mut StockCounts, row: usize, m: String| c.errors.push(RowError { sheet: file.sheet.clone(), row, message: m });
    for (row, r) in &file.rows {
        touched.insert(r.cell_id);
        if let Some(first) = seen.insert(r.cell_id, *row) {
            err(&mut c, *row, format!("CellId {} 가 {first} 줄과 겹친다 — 한 자리는 한 줄", r.cell_id));
            continue;
        }
        if !known(r.cell_id) {
            err(&mut c, *row, format!("CellId {} 는 등록된 셀·스테이션이 아니다", r.cell_id));
            continue;
        }
        if r.count > 0 {
            match item(r.item_code) {
                None => {
                    err(&mut c, *row, format!("ItemCode {} 는 등록된 품목이 아니다", r.item_code));
                    continue;
                }
                Some(max) if max > 0 && r.count > max => {
                    err(&mut c, *row, format!("Count {} 가 품목 {} 의 StackMax {max} 를 넘는다", r.count, r.item_code));
                    continue;
                }
                Some(_) => {}
            }
        }
        match (cur.get(&r.cell_id), r.count) {
            (None, 0) => c.unchanged += 1,
            (Some(_), 0) => {
                c.updated += 1;
                ops.push(Op::Remove(r.cell_id));
            }
            (Some(e), _) if e.item_code == r.item_code && e.count == r.count && e.note == r.note => c.unchanged += 1,
            (Some(_), _) => {
                c.updated += 1;
                ops.push(Op::Set(r.clone()));
            }
            (None, _) => {
                c.added += 1;
                ops.push(Op::Set(r.clone()));
            }
        }
    }
    if mode == Mode::Replace && file.errors.iter().all(|e| e.row != 1) {
        let mut gone: Vec<u16> = cur.keys().filter(|id| !touched.contains(id)).copied().collect();
        gone.sort_unstable();
        c.removed = gone.len();
        ops.extend(gone.into_iter().map(Op::Remove));
    }
    c.skipped = c.errors.len();
    (c, ops)
}

/// 계획을 세우고 `dry_run` 이 아니면 적용한다(재고 이벤트가 나가 화면이 바로 바뀐다).
pub fn apply(st: &AppState, file: &StockSheet, mode: Mode, dry_run: bool) -> Result<StockCounts, ApiError> {
    let current = st.stock.list()?;
    let cells: HashSet<u16> = st.registry.cells()?.into_iter().map(|e| e.cell.id).chain(st.registry.stations()?.into_iter().map(|e| e.id)).collect();
    let items: HashMap<u32, u32> = st.registry.items()?.into_iter().map(|i| (i.code, i.spec.stack_max as u32)).collect();
    let (counts, ops) = plan(file, &current, mode, &|id| cells.contains(&id), &|code| items.get(&code).copied());
    if !dry_run {
        for op in &ops {
            match op {
                Op::Set(r) => {
                    st.stock.set(r.cell_id, r.item_code, r.count, &r.note, "file import")?;
                }
                Op::Remove(id) => {
                    st.stock.remove(*id)?;
                }
            }
        }
    }
    Ok(counts)
}

/// 내보낼 줄 — 품목 이름을 붙여 읽기 쉽게.
pub fn export_rows(st: &AppState) -> Result<Vec<StockExportRow>, ApiError> {
    let names: HashMap<u32, String> = st.registry.items()?.into_iter().map(|i| (i.code, i.name)).collect();
    Ok(st
        .stock
        .list()?
        .into_iter()
        .filter(|e| e.count > 0)
        .map(|e| StockExportRow { cell_id: e.cell_id, item_code: e.item_code, item_name: names.get(&e.item_code).cloned().unwrap_or_default(), count: e.count, note: e.note, updated_at: e.updated_at })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u16, code: u32, n: u32) -> StockEntry {
        StockEntry { cell_id: id, item_code: code, count: n, note: String::new(), updated_at: String::new() }
    }
    fn row(id: u16, code: u32, n: u32) -> StockRow {
        StockRow { cell_id: id, item_code: code, count: n, note: String::new() }
    }
    fn sheet(rows: Vec<StockRow>) -> StockSheet {
        StockSheet { sheet: "Stock".into(), rows: rows.into_iter().enumerate().map(|(i, r)| (i + 2, r)).collect(), errors: vec![] }
    }
    const KNOWN: fn(u16) -> bool = |id| id == 101 || id == 102 || id == 103 || id == 2101;
    const ITEM: fn(u32) -> Option<u32> = |c| match c {
        1001 => Some(5),
        1002 => Some(0),
        _ => None,
    };

    #[test]
    fn merge_classifies_each_row() {
        let cur = [entry(101, 1001, 3), entry(102, 1001, 2)];
        let f = sheet(vec![row(101, 1001, 3), row(102, 1001, 4), row(103, 1002, 1), row(2101, 1001, 1)]);
        let (c, ops) = plan(&f, &cur, Mode::Merge, &KNOWN, &ITEM);
        assert_eq!((c.added, c.updated, c.unchanged, c.removed, c.skipped), (2, 1, 1, 0, 0));
        assert_eq!(ops, vec![Op::Set(row(102, 1001, 4)), Op::Set(row(103, 1002, 1)), Op::Set(row(2101, 1001, 1))]);
    }

    #[test]
    fn count_zero_empties_and_replace_removes_the_rest() {
        let cur = [entry(101, 1001, 3), entry(102, 1001, 2), entry(103, 1002, 1)];
        let f = sheet(vec![row(101, 0, 0)]);
        let (c, ops) = plan(&f, &cur, Mode::Merge, &KNOWN, &ITEM);
        assert_eq!((c.updated, c.removed), (1, 0));
        assert_eq!(ops, vec![Op::Remove(101)]);
        let (c, ops) = plan(&f, &cur, Mode::Replace, &KNOWN, &ITEM);
        assert_eq!((c.updated, c.removed), (1, 2));
        assert_eq!(ops, vec![Op::Remove(101), Op::Remove(102), Op::Remove(103)]);
    }

    #[test]
    fn bad_rows_are_skipped_and_their_places_kept_on_replace() {
        let cur = [entry(101, 1001, 3), entry(102, 1001, 2)];
        let f = sheet(vec![
            row(999, 1001, 1), // 등록 안 된 자리
            row(101, 7777, 1), // 등록 안 된 품목 → 101 은 replace 에서도 남는다
            row(103, 1001, 9), // StackMax 5 초과
            row(103, 1001, 1), // 중복
        ]);
        let (c, ops) = plan(&f, &cur, Mode::Replace, &KNOWN, &ITEM);
        assert_eq!(c.skipped, 4);
        assert_eq!(c.errors.len(), 4);
        assert_eq!(ops, vec![Op::Remove(102)], "only the untouched place goes");
    }

    #[test]
    fn a_broken_header_never_wipes_stock() {
        let cur = [entry(101, 1001, 3)];
        let f = StockSheet { sheet: "Stock".into(), rows: vec![], errors: vec![RowError { sheet: "Stock".into(), row: 1, message: "no CellId".into() }] };
        let (c, ops) = plan(&f, &cur, Mode::Replace, &KNOWN, &ITEM);
        assert!(ops.is_empty());
        assert_eq!((c.removed, c.skipped), (0, 1));
    }
}
