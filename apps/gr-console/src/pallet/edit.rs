//! 패턴 편집 — 순수 검증 · 변경 · 사양서 비교 · 가져오기 병합(sqlite 없음). 저장은 `store.rs` 가 한 트랜잭션으로 한다.
//!
//! 검증 규칙(400):
//! - Flow id `[A-Z0-9_]{2,32}`(앞뒤 공백을 떼고 대문자로 맞춘 뒤), name 1..=80 자, note ≤ 500 자
//! - 슬롯 1..=20 개, Seq = 1..n 순열, DragDir 0..=8, Slot 이름 중복 없음, |u| ≤ 20
//! - OdMin < OdMax (0..=3000), 한 흐름 안 OD 범위가 겹치지 않음(틈은 경고 — 그 외경은 "no pattern for OD")
//! - 두 슬롯이 같은 자리(MinDistance < 0.01)면 오류. MinDistance ≠ 1 은 저장하고 경고만 한다 — 생성기가
//!   `(OuterDiameter + Gap) / MinDistance` 로 맞춘다(`mod.rs`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::{DragKind, Flow, OD_BOUNDARY_TOL, SOURCE_CUSTOM, SOURCE_IMPORT, SOURCE_SPEC, ScreenAxes, Spec, SpecPattern, SpecSlot, norm_min_distance, seed};
use super::{Library, r1};
use crate::error::ApiError;

pub const FLOWS_MAX: usize = 100;
pub const PATTERNS_MAX: usize = 40;
pub const SLOTS_MAX: usize = 20;
pub const NAME_MAX: usize = 80;
pub const NOTE_MAX: usize = 500;
pub const SLOT_NAME_MAX: usize = 16;
pub const U_MAX: f32 = 20.0;
pub const OD_LIMIT: f32 = 3000.0;
/// 이보다 가까운 두 슬롯은 같은 자리로 본다(정규화 단위).
pub const COINCIDENT: f32 = 0.01;
/// MinDistance 가 1 에서 이만큼 넘게 벗어나면 배율 경고.
pub const NORM_EPS: f32 = 1e-3;
/// 비교에서 같은 좌표로 보는 폭(정규화 단위).
const U_EPS: f32 = 5e-5;
const OD_EPS: f32 = 1e-3;

fn r4(v: f32) -> f32 {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0 } else { r }
}

// ── 검증 ────────────────────────────────────────────────────────────────────

pub fn normalize_flow_id(raw: &str) -> Result<String, String> {
    let id = raw.trim().to_ascii_uppercase();
    let ok = (2..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if ok { Ok(id) } else { Err(format!("Flow id \"{}\" 는 [A-Z0-9_] 2..32 자여야 합니다", raw.trim())) }
}

fn kind_word(k: DragKind) -> &'static str {
    match k {
        DragKind::In => "입고",
        DragKind::Out => "출하",
    }
}

/// 이름·메모 앞뒤 공백을 뗀다.
pub fn tidy_pattern(p: &mut SpecPattern) {
    p.note = p.note.trim().to_string();
    for s in &mut p.slots {
        s.slot = s.slot.trim().to_string();
    }
}

fn tidy_flow(f: &mut Flow) {
    f.name = f.name.trim().to_string();
    f.note = f.note.trim().to_string();
    for p in &mut f.patterns {
        tidy_pattern(p);
    }
    f.patterns.sort_by_key(|p| p.pattern);
}

/// 패턴 한 개 — 오류를 모두 모아 `Err`, 경고는 `Ok`.
pub fn validate_pattern(kind: DragKind, p: &SpecPattern) -> Result<Vec<String>, Vec<String>> {
    let tag = format!("Pattern {}", p.pattern);
    let mut errs = Vec::new();
    let mut warns = Vec::new();
    if p.pattern == 0 {
        errs.push(format!("{tag}: Pattern 번호는 1 이상이어야 합니다"));
    }
    if !p.od_min.is_finite() || !p.od_max.is_finite() || p.od_min < 0.0 || p.od_max > OD_LIMIT {
        errs.push(format!("{tag}: OdMin {} / OdMax {} 는 0..={OD_LIMIT} mm 여야 합니다", p.od_min, p.od_max));
    } else if p.od_min >= p.od_max {
        errs.push(format!("{tag}: OdMin {} 은 OdMax {} 보다 작아야 합니다", p.od_min, p.od_max));
    }
    let n = p.slots.len();
    if !(1..=SLOTS_MAX).contains(&n) {
        errs.push(format!("{tag}: 슬롯 수 {n} — 1..={SLOTS_MAX} 개여야 합니다"));
    }
    let mut names = BTreeSet::new();
    for s in &p.slots {
        let nm = s.slot.as_str();
        if nm.is_empty() || nm.chars().count() > SLOT_NAME_MAX {
            errs.push(format!("{tag}: Slot 이름 \"{nm}\" 은 1..={SLOT_NAME_MAX} 자여야 합니다"));
        } else if !names.insert(nm) {
            errs.push(format!("{tag}: Slot {nm} 이 중복입니다"));
        }
        if s.drag_dir > 8 {
            errs.push(format!("{tag}: {nm} DragDir {} — 0..8 이어야 합니다", s.drag_dir));
        }
        if !s.u.iter().all(|v| v.is_finite() && v.abs() <= U_MAX) {
            errs.push(format!("{tag}: {nm} OffsetX/OffsetY(정규화) 는 |u| ≤ {U_MAX} 여야 합니다"));
        }
    }
    let mut seqs: Vec<u32> = p.slots.iter().map(|s| u32::from(s.seq)).collect();
    seqs.sort_unstable();
    if n > 0 && seqs != (1..=n as u32).collect::<Vec<_>>() {
        errs.push(format!("{tag}: Seq 는 1..{n} 을 한 번씩 써야 합니다 (받은 값 {})", seqs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",")));
    }
    if p.note.chars().count() > NOTE_MAX {
        errs.push(format!("{tag}: note 는 {NOTE_MAX} 자 이하"));
    }
    if let Some(d) = norm_min_distance(&p.slots)
        && p.slots.iter().all(|s| s.u.iter().all(|v| v.is_finite()))
    {
        if d < COINCIDENT {
            errs.push(format!("{tag}: 두 슬롯이 같은 자리입니다 (MinDistance {})", r4(d)));
        } else if (d - 1.0).abs() > NORM_EPS {
            warns.push(format!("{tag}: MinDistance {} ≠ 1 — 생성기가 오프셋을 ×{} 해서 최소 중심거리를 OuterDiameter + Gap 으로 맞춥니다", r4(d), r4(1.0 / d)));
        }
    }
    if errs.is_empty() {
        let (at, word) = match kind {
            DragKind::In => (1u8, "첫"),
            DragKind::Out => (n as u8, "마지막"),
        };
        if let Some(s) = p.slots.iter().find(|s| s.seq == at)
            && s.drag_dir != 0
        {
            warns.push(format!("{tag}: {} 흐름인데 Seq {at}({}) 에 DragDir {} — 사양서는 {word} 작업을 드래그 없이 둡니다", kind_word(kind), s.slot, s.drag_dir));
        }
    }
    if errs.is_empty() { Ok(warns) } else { Err(errs) }
}

/// 한 흐름 안에서 OD 범위가 겹치는 쌍.
pub fn od_overlaps(ps: &[SpecPattern]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in ps.iter().enumerate() {
        for b in &ps[i + 1..] {
            if a.od_min <= b.od_max && b.od_min <= a.od_max {
                out.push(format!("OD 범위가 겹칩니다: Pattern {} ({}~{}) 와 Pattern {} ({}~{})", a.pattern, a.od_min, a.od_max, b.pattern, b.od_min, b.od_max));
            }
        }
    }
    out
}

/// 이어지지 않는 OD 구간(경계 틈 > 1 mm) — 그 외경은 계획·작업 명령이 거부한다.
pub fn od_gaps(ps: &[SpecPattern]) -> Vec<String> {
    let mut v: Vec<&SpecPattern> = ps.iter().filter(|p| p.od_min < p.od_max).collect();
    v.sort_by(|a, b| a.od_min.total_cmp(&b.od_min));
    v.windows(2)
        .filter(|w| w[1].od_min - w[0].od_max > OD_BOUNDARY_TOL)
        .map(|w| format!("no pattern for OD {}~{} (Pattern {} 과 Pattern {} 사이) — 이 외경은 계획·작업 명령이 거부합니다", w[0].od_max, w[1].od_min, w[0].pattern, w[1].pattern))
        .collect()
}

/// 흐름 전체 — 오류를 모두 모아 `Err`, 경고는 `Ok`.
pub fn validate_flow(f: &Flow) -> Result<Vec<String>, Vec<String>> {
    let mut errs = Vec::new();
    let mut warns = Vec::new();
    match normalize_flow_id(&f.id) {
        Ok(id) if id == f.id => {}
        Ok(id) => errs.push(format!("Flow id {} 는 {id} 로 적어야 합니다", f.id)),
        Err(e) => errs.push(e),
    }
    if f.name.is_empty() || f.name.chars().count() > NAME_MAX {
        errs.push(format!("Flow {}: name 은 1..={NAME_MAX} 자여야 합니다", f.id));
    }
    if f.note.chars().count() > NOTE_MAX {
        errs.push(format!("Flow {}: note 는 {NOTE_MAX} 자 이하", f.id));
    }
    if !matches!(f.screen_axes.right.as_str(), "X+" | "X-") || !matches!(f.screen_axes.down.as_str(), "Y+" | "Y-") {
        errs.push(format!("Flow {}: screen_axes 는 right X+|X-, down Y+|Y- 여야 합니다", f.id));
    }
    if f.patterns.len() > PATTERNS_MAX {
        errs.push(format!("Flow {}: Pattern 은 {PATTERNS_MAX} 개 이하", f.id));
    }
    let mut nums = BTreeSet::new();
    for p in &f.patterns {
        if !nums.insert(p.pattern) {
            errs.push(format!("Flow {}: Pattern {} 이 중복입니다", f.id, p.pattern));
        }
        match validate_pattern(f.drag_kind, p) {
            Ok(w) => warns.extend(w),
            Err(e) => errs.extend(e),
        }
    }
    errs.extend(od_overlaps(&f.patterns));
    warns.extend(od_gaps(&f.patterns));
    if f.patterns.is_empty() {
        warns.push(format!("Flow {} 에 Pattern 이 없습니다 — 계획·작업 명령에 쓸 수 없습니다", f.id));
    }
    if errs.is_empty() { Ok(warns) } else { Err(errs) }
}

fn bad(errs: Vec<String>) -> ApiError {
    ApiError::BadRequest(errs.join(" · "))
}

// ── 비교 ────────────────────────────────────────────────────────────────────

fn same_slots(a: &[SpecSlot], b: &[SpecSlot]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| y.slot == x.slot && y.seq == x.seq && y.drag_dir == x.drag_dir && (y.u[0] - x.u[0]).abs() <= U_EPS && (y.u[1] - x.u[1]).abs() <= U_EPS))
}

/// 편집 내용(번호·OD·슬롯·note)이 같은가 — updated_at·사양서 메타는 보지 않는다.
pub fn same_content(a: &SpecPattern, b: &SpecPattern) -> bool {
    a.pattern == b.pattern && (a.od_min - b.od_min).abs() <= OD_EPS && (a.od_max - b.od_max).abs() <= OD_EPS && a.note == b.note && same_slots(&a.slots, &b.slots)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub spec: Json,
    pub current: Json,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SlotDiff {
    pub slot: String,
    /// same | changed | added | removed
    pub status: &'static str,
    pub fields: Vec<FieldChange>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PatternDiff {
    pub pattern: u8,
    /// same | changed | added | removed
    pub status: &'static str,
    pub fields: Vec<FieldChange>,
    pub slots: Vec<SlotDiff>,
    pub spec: Option<SpecPattern>,
    pub current: Option<SpecPattern>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FlowDiff {
    pub id: String,
    pub based_on: Option<String>,
    /// spec(사양서와 같음) | modified | custom(사양서 기준 없음)
    pub status: &'static str,
    /// name·note 만 바뀐 것은 수정으로 치지 않는다.
    pub modified: bool,
    pub fields: Vec<FieldChange>,
    pub patterns: Vec<PatternDiff>,
}

fn change(field: &str, spec: Json, current: Json) -> FieldChange {
    FieldChange { field: field.into(), spec, current }
}

/// 직렬화 → JSON 값. `serde_json::to_value`/`json!` 은 f32 를 f64 로 넓혀 `0.5033000111579895` 처럼 보이게 하므로
/// 문자열(f32 최단 표기)을 거쳐 읽는다 — 화면·비교·내보내기에 사양서와 같은 숫자가 나온다.
pub fn clean_json<T: Serialize + ?Sized>(v: &T) -> Json {
    serde_json::to_string(v).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn diff_pattern(cur: Option<&SpecPattern>, base: Option<&SpecPattern>) -> PatternDiff {
    let pattern = cur.or(base).map(|p| p.pattern).unwrap_or(0);
    let (status, fields, slots) = match (cur, base) {
        (Some(c), None) => ("added", vec![], c.slots.iter().map(|s| SlotDiff { slot: s.slot.clone(), status: "added", fields: vec![] }).collect()),
        (None, Some(b)) => ("removed", vec![], b.slots.iter().map(|s| SlotDiff { slot: s.slot.clone(), status: "removed", fields: vec![] }).collect()),
        (Some(c), Some(b)) => {
            let mut fields = Vec::new();
            if (c.od_min - b.od_min).abs() > OD_EPS {
                fields.push(change("od_min", clean_json(&b.od_min), clean_json(&c.od_min)));
            }
            if (c.od_max - b.od_max).abs() > OD_EPS {
                fields.push(change("od_max", clean_json(&b.od_max), clean_json(&c.od_max)));
            }
            if c.note != b.note {
                fields.push(change("note", json!(b.note), json!(c.note)));
            }
            let mut slots = Vec::new();
            for bs in &b.slots {
                match c.slots.iter().find(|s| s.slot == bs.slot) {
                    None => slots.push(SlotDiff { slot: bs.slot.clone(), status: "removed", fields: vec![] }),
                    Some(cs) => {
                        let mut f = Vec::new();
                        if cs.seq != bs.seq {
                            f.push(change("seq", json!(bs.seq), json!(cs.seq)));
                        }
                        if (cs.u[0] - bs.u[0]).abs() > U_EPS || (cs.u[1] - bs.u[1]).abs() > U_EPS {
                            f.push(change("u", clean_json(&bs.u), clean_json(&cs.u)));
                        }
                        if cs.drag_dir != bs.drag_dir {
                            f.push(change("drag_dir", json!(bs.drag_dir), json!(cs.drag_dir)));
                        }
                        slots.push(SlotDiff { slot: bs.slot.clone(), status: if f.is_empty() { "same" } else { "changed" }, fields: f });
                    }
                }
            }
            for cs in c.slots.iter().filter(|s| !b.slots.iter().any(|x| x.slot == s.slot)) {
                slots.push(SlotDiff { slot: cs.slot.clone(), status: "added", fields: vec![] });
            }
            let same = fields.is_empty() && slots.iter().all(|s| s.status == "same");
            (if same { "same" } else { "changed" }, fields, slots)
        }
        (None, None) => ("same", vec![], vec![]),
    };
    PatternDiff { pattern, status, fields, slots, spec: base.cloned(), current: cur.cloned() }
}

/// 사양서 기준(`based_on` → 내장 흐름)과 비교.
pub fn diff_flow(cur: &Flow) -> FlowDiff {
    let base = cur.based_on.as_deref().and_then(|b| seed().flow(b));
    diff_against(cur, base)
}

pub fn diff_against(cur: &Flow, base: Option<&Flow>) -> FlowDiff {
    let Some(base) = base else {
        return FlowDiff { id: cur.id.clone(), based_on: cur.based_on.clone(), status: "custom", modified: false, fields: vec![], patterns: vec![] };
    };
    let mut fields = Vec::new();
    if cur.name != base.name {
        fields.push(change("name", json!(base.name), json!(cur.name)));
    }
    if cur.drag_kind != base.drag_kind {
        fields.push(change("drag_kind", json!(base.drag_kind), json!(cur.drag_kind)));
    }
    let nums: BTreeSet<u8> = cur.patterns.iter().chain(&base.patterns).map(|p| p.pattern).collect();
    let patterns: Vec<PatternDiff> = nums.into_iter().map(|n| diff_pattern(cur.pattern(n), base.pattern(n))).collect();
    // name·note 는 이름표라 수정으로 치지 않는다(복사한 흐름은 이름이 늘 다르다) — 생성에 쓰는 값만 본다.
    let modified = fields.iter().any(|f| f.field != "name")
        || patterns.iter().any(|p| match p.status {
            "same" => false,
            "changed" => p.fields.iter().any(|f| f.field != "note") || p.slots.iter().any(|s| s.status != "same"),
            _ => true,
        });
    FlowDiff { id: cur.id.clone(), based_on: cur.based_on.clone(), status: if modified { "modified" } else { "spec" }, modified, fields, patterns }
}

// ── 변경 ────────────────────────────────────────────────────────────────────

/// 저장소 한 트랜잭션 안에서 읽은 상태.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub lib: Library,
    /// 흐름 id(대문자) → 그 흐름을 쓰는 품목 코드.
    pub usage: BTreeMap<String, Vec<u32>>,
}

impl Snapshot {
    pub fn used_by(&self, id: &str) -> Vec<u32> {
        self.usage.get(&id.trim().to_ascii_uppercase()).cloned().unwrap_or_default()
    }
    fn find(&self, id: &str) -> Result<&Flow, ApiError> {
        self.lib.flow(id).ok_or_else(|| ApiError::NotFound(format!("Flow {id} 가 없습니다 (있는 것: {})", self.lib.ids())))
    }
}

/// 저장소에 쓸 변경 — `rename` 먼저, 그다음 `delete`, `write`(흐름 행 + 그 흐름의 패턴 전체 교체).
#[derive(Clone, Debug, Default)]
pub struct Change {
    pub rename: Option<(String, String)>,
    pub delete: Vec<String>,
    pub write: Vec<Flow>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CreateFlow {
    pub id: String,
    pub name: Option<String>,
    pub drag_kind: Option<DragKind>,
    pub note: Option<String>,
    /// 이 흐름의 패턴·사양서 기준을 그대로 복사한다.
    pub copy_from: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FlowPatch {
    /// 다르면 이름 바꾸기 — 이 흐름을 쓰는 프로파일도 같이 바뀐다.
    pub id: Option<String>,
    pub name: Option<String>,
    pub drag_kind: Option<DragKind>,
    pub note: Option<String>,
    pub screen_axes: Option<ScreenAxes>,
    pub reference: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PatternBody {
    /// 경로의 번호와 다르면 번호 바꾸기.
    #[serde(default)]
    pub pattern: Option<u8>,
    pub od_min: f32,
    pub od_max: f32,
    pub slots: Vec<SpecSlot>,
    #[serde(default)]
    pub note: String,
}

fn stamp(p: &mut SpecPattern, now: &str) {
    p.updated_at = now.to_string();
}

pub fn create_flow(snap: &Snapshot, req: &CreateFlow, now: &str) -> Result<(Flow, Vec<String>), ApiError> {
    let id = normalize_flow_id(&req.id).map_err(ApiError::BadRequest)?;
    if snap.lib.flow(&id).is_some() {
        return Err(ApiError::Conflict(format!("Flow {id} 가 이미 있습니다")));
    }
    if snap.lib.flows.len() >= FLOWS_MAX {
        return Err(ApiError::BadRequest(format!("흐름은 {FLOWS_MAX} 개까지입니다")));
    }
    let mut f = match &req.copy_from {
        Some(src) => {
            let s = snap.find(src)?;
            let mut patterns = s.patterns.clone();
            patterns.iter_mut().for_each(|p| stamp(p, now));
            Flow {
                id: id.clone(),
                name: req.name.clone().unwrap_or_else(|| format!("{} (copy of {})", s.name, s.id)),
                drag_kind: req.drag_kind.unwrap_or(s.drag_kind),
                patterns,
                source: SOURCE_CUSTOM.into(),
                note: req.note.clone().unwrap_or_default(),
                updated_at: now.into(),
                ..s.clone()
            }
        }
        None => Flow {
            id: id.clone(),
            name: req.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.clone()),
            source_slide: 0,
            also_slides: vec![],
            drag_kind: req.drag_kind.ok_or_else(|| ApiError::BadRequest("drag_kind(in|out) 가 필요합니다 — 또는 copy_from 으로 복사".into()))?,
            reference: false,
            robot: String::new(),
            zone: String::new(),
            screen_axes: ScreenAxes::default(),
            notes: vec![],
            patterns: vec![],
            source: SOURCE_CUSTOM.into(),
            based_on: None,
            note: req.note.clone().unwrap_or_default(),
            updated_at: now.into(),
        },
    };
    tidy_flow(&mut f);
    let warns = validate_flow(&f).map_err(bad)?;
    Ok((f, warns))
}

pub fn update_flow(snap: &Snapshot, id: &str, patch: &FlowPatch, now: &str) -> Result<(Flow, Change, Vec<String>), ApiError> {
    let cur = snap.find(id)?;
    let mut f = cur.clone();
    let mut ch = Change::default();
    if let Some(v) = &patch.name {
        f.name = v.clone();
    }
    if let Some(v) = patch.drag_kind {
        f.drag_kind = v;
    }
    if let Some(v) = &patch.note {
        f.note = v.clone();
    }
    if let Some(v) = &patch.screen_axes {
        f.screen_axes = v.clone();
    }
    if let Some(v) = patch.reference {
        f.reference = v;
    }
    if let Some(raw) = &patch.id {
        let new = normalize_flow_id(raw).map_err(ApiError::BadRequest)?;
        if new != cur.id {
            if snap.lib.flow(&new).is_some() {
                return Err(ApiError::Conflict(format!("Flow {new} 가 이미 있습니다")));
            }
            ch.rename = Some((cur.id.clone(), new.clone()));
            f.id = new;
        }
    }
    tidy_flow(&mut f);
    if f != *cur {
        f.updated_at = now.into();
    }
    let warns = validate_flow(&f).map_err(bad)?;
    ch.write.push(f.clone());
    Ok((f, ch, warns))
}

pub fn delete_flow(snap: &Snapshot, id: &str) -> Result<Change, ApiError> {
    let cur = snap.find(id)?;
    let used = snap.used_by(&cur.id);
    if !used.is_empty() {
        return Err(ApiError::Conflict(format!(
            "Flow {} 는 품목 {} 의 팔렛 패턴이 씁니다 — 먼저 그 품목의 Flow 를 바꾸거나 지우세요",
            cur.id,
            used.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(Change { delete: vec![cur.id.clone()], ..Default::default() })
}

pub fn upsert_pattern(snap: &Snapshot, id: &str, no: u8, body: &PatternBody, now: &str) -> Result<(Flow, SpecPattern, Vec<String>), ApiError> {
    let cur = snap.find(id)?;
    let target = body.pattern.unwrap_or(no);
    if target != no && cur.pattern(target).is_some() {
        return Err(ApiError::Conflict(format!("Flow {} 에 Pattern {target} 이 이미 있습니다", cur.id)));
    }
    let existing = cur.pattern(no);
    let mut p = SpecPattern {
        pattern: target,
        od_min: body.od_min,
        od_max: body.od_max,
        order_text: existing.map(|e| e.order_text.clone()).unwrap_or_default(),
        drawn: existing.map(|e| e.drawn.clone()).unwrap_or_default(),
        slots: body.slots.clone(),
        notes: existing.map(|e| e.notes.clone()).unwrap_or_default(),
        note: body.note.clone(),
        updated_at: now.into(),
    };
    tidy_pattern(&mut p);
    let mut f = cur.clone();
    match existing {
        Some(e) if same_content(e, &p) => p.updated_at = e.updated_at.clone(),
        _ => f.updated_at = now.into(),
    }
    f.patterns.retain(|x| x.pattern != no && x.pattern != target);
    f.patterns.push(p.clone());
    tidy_flow(&mut f);
    let warns = validate_flow(&f).map_err(bad)?;
    Ok((f, p, warns))
}

pub fn delete_pattern(snap: &Snapshot, id: &str, no: u8) -> Result<Flow, ApiError> {
    let cur = snap.find(id)?;
    if cur.pattern(no).is_none() {
        return Err(ApiError::NotFound(format!("Flow {} 에 Pattern {no} 이 없습니다", cur.id)));
    }
    let mut f = cur.clone();
    f.patterns.retain(|p| p.pattern != no);
    Ok(f)
}

/// 사양서 기준으로 되돌린다 — `pattern` 이 있으면 그 패턴만, 없으면 이름·드래그 종류·메타·패턴 전체(운전자 note 는 둔다).
pub fn reset_flow(snap: &Snapshot, id: &str, pattern: Option<u8>, now: &str) -> Result<Flow, ApiError> {
    let cur = snap.find(id)?;
    let base = cur.based_on.as_deref().and_then(|b| seed().flow(b)).ok_or_else(|| ApiError::BadRequest(format!("Flow {} 는 사양서 흐름에서 오지 않아 되돌릴 기준이 없습니다", cur.id)))?;
    let mut f = cur.clone();
    match pattern {
        None => {
            let mut patterns = base.patterns.clone();
            patterns.iter_mut().for_each(|p| stamp(p, now));
            f = Flow {
                id: cur.id.clone(),
                patterns,
                source: if cur.id == base.id { SOURCE_SPEC.into() } else { cur.source.clone() },
                based_on: Some(base.id.clone()),
                note: cur.note.clone(),
                updated_at: now.into(),
                ..base.clone()
            };
        }
        Some(no) => {
            let mut p = base.pattern(no).cloned().ok_or_else(|| ApiError::NotFound(format!("사양서 Flow {} 에 Pattern {no} 이 없습니다", base.id)))?;
            stamp(&mut p, now);
            f.patterns.retain(|x| x.pattern != no);
            f.patterns.push(p);
            f.updated_at = now.into();
        }
    }
    tidy_flow(&mut f);
    validate_flow(&f).map_err(bad)?;
    Ok(f)
}

/// 삭제한 패턴의 흐름 — 시각을 찍어 검증까지.
pub fn finish_delete_pattern(mut f: Flow, now: &str) -> Result<Flow, ApiError> {
    f.updated_at = now.into();
    validate_flow(&f).map_err(bad)?;
    Ok(f)
}

// ── 내보내기 · 가져오기 ─────────────────────────────────────────────────────

/// 저장소 → 사양 문서(`spec_r4.json` 과 같은 모양).
pub fn to_doc(lib: &Library, at: &str) -> Spec {
    let s = seed();
    Spec {
        version: format!("store {at}"),
        source: format!("gr-console pallet store (seed {})", s.version),
        generator: "gr-console GET /api/pallet/export.json".into(),
        pallet_size_mm: s.pallet_size_mm,
        normalization: format!("{} Edited patterns keep u as stored; the generator scales offsets by (OuterDiameter + Gap) / MinDistance(u).", s.normalization),
        size_classes: s.size_classes.clone(),
        size_class_notes: s.size_class_notes.clone(),
        flows: lib.flows.clone(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportPatternRow {
    pub pattern: u8,
    /// added | changed | unchanged
    pub action: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportFlowRow {
    pub id: String,
    /// created | updated | unchanged | invalid
    pub action: &'static str,
    pub patterns: Vec<ImportPatternRow>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub ok: bool,
    pub errors: Vec<String>,
    pub flows: Vec<ImportFlowRow>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

impl ImportReport {
    pub fn all_errors(&self) -> Vec<String> {
        self.errors.iter().cloned().chain(self.flows.iter().flat_map(|f| f.errors.iter().map(move |e| format!("{}: {e}", f.id)))).collect()
    }
}

/// 흐름 id + 패턴 번호로 병합 — 문서에 없는 흐름·패턴은 그대로 둔다. 쓸 흐름과 보고서를 돌려준다.
pub fn merge_import(lib: &Library, doc: &Spec, now: &str) -> (Vec<Flow>, ImportReport) {
    let mut errors = Vec::new();
    if doc.flows.is_empty() {
        errors.push("flows 가 비었습니다".into());
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    let mut writes = Vec::new();
    for df in &doc.flows {
        let id = match normalize_flow_id(&df.id) {
            Ok(id) => id,
            Err(e) => {
                rows.push(ImportFlowRow { id: df.id.clone(), action: "invalid", patterns: vec![], warnings: vec![], errors: vec![e] });
                continue;
            }
        };
        if !seen.insert(id.clone()) {
            errors.push(format!("Flow {id} 가 문서에 두 번 있습니다"));
            continue;
        }
        let existing = lib.flow(&id);
        let mut row_errors = Vec::new();
        let mut f = match existing {
            Some(e) => {
                let mut f = e.clone();
                if !df.name.trim().is_empty() {
                    f.name = df.name.clone();
                }
                f.drag_kind = df.drag_kind;
                if !df.note.trim().is_empty() {
                    f.note = df.note.clone();
                }
                if !df.robot.is_empty() {
                    f.robot = df.robot.clone();
                }
                if !df.zone.is_empty() {
                    f.zone = df.zone.clone();
                }
                if !df.notes.is_empty() {
                    f.notes = df.notes.clone();
                }
                if df.screen_axes != ScreenAxes::default() {
                    f.screen_axes = df.screen_axes.clone();
                }
                f
            }
            None => {
                let based_on = df.based_on.as_deref().and_then(|b| seed().flow(b)).or_else(|| seed().flow(&id)).map(|s| s.id.clone());
                Flow {
                    id: id.clone(),
                    name: if df.name.trim().is_empty() { id.clone() } else { df.name.clone() },
                    patterns: vec![],
                    source: SOURCE_IMPORT.into(),
                    based_on,
                    updated_at: now.into(),
                    ..df.clone()
                }
            }
        };
        let mut prow = Vec::new();
        let mut nums = BTreeSet::new();
        for dp in &df.patterns {
            if !nums.insert(dp.pattern) {
                row_errors.push(format!("Pattern {} 이 문서에 두 번 있습니다", dp.pattern));
                continue;
            }
            let mut p = dp.clone();
            tidy_pattern(&mut p);
            let action = match f.pattern(p.pattern) {
                Some(ep) if same_content(ep, &p) => "unchanged",
                Some(ep) => {
                    if p.order_text.is_empty() {
                        p.order_text = ep.order_text.clone();
                    }
                    if p.notes.is_empty() {
                        p.notes = ep.notes.clone();
                    }
                    if p.drawn == Default::default() {
                        p.drawn = ep.drawn.clone();
                    }
                    "changed"
                }
                None => "added",
            };
            if action != "unchanged" {
                p.updated_at = now.into();
                f.patterns.retain(|x| x.pattern != p.pattern);
                f.patterns.push(p.clone());
            }
            prow.push(ImportPatternRow { pattern: p.pattern, action });
        }
        tidy_flow(&mut f);
        let action = match existing {
            None => "created",
            Some(e) => {
                let meta_same = Flow { patterns: vec![], updated_at: String::new(), ..e.clone() } == Flow { patterns: vec![], updated_at: String::new(), ..f.clone() };
                if meta_same && prow.iter().all(|p| p.action == "unchanged") { "unchanged" } else { "updated" }
            }
        };
        if action == "updated" {
            f.updated_at = now.into();
        }
        let warnings = match validate_flow(&f) {
            Ok(w) => w,
            Err(e) => {
                row_errors.extend(e);
                vec![]
            }
        };
        if row_errors.is_empty() && action != "unchanged" {
            writes.push(f);
        }
        rows.push(ImportFlowRow { id, action: if row_errors.is_empty() { action } else { "invalid" }, patterns: prow, warnings, errors: row_errors });
    }
    let ok = errors.is_empty() && rows.iter().all(|r| r.errors.is_empty());
    let count = |a: &str| rows.iter().filter(|r| r.action == a).count();
    let report = ImportReport { dry_run: true, ok, errors, created: count("created"), updated: count("updated"), unchanged: count("unchanged"), flows: rows };
    (if ok { writes } else { vec![] }, report)
}

// ── 화면용 보기 ─────────────────────────────────────────────────────────────

/// `GET /api/pallet/flows` 한 줄 — 흐름 + 패턴별 MinDistance·비교 상태 + 사용 품목 + 경고.
pub fn flow_view(f: &Flow, used_by: &[u32]) -> Json {
    let d = diff_flow(f);
    let warnings = validate_flow(f).unwrap_or_else(|e| e);
    let mut v = clean_json(f);
    let patterns: Vec<Json> = f
        .patterns
        .iter()
        .map(|p| {
            let mut pv = clean_json(p);
            pv["min_distance"] = clean_json(&norm_min_distance(&p.slots).map(r4));
            pv["status"] = json!(d.patterns.iter().find(|x| x.pattern == p.pattern).map(|x| x.status).unwrap_or("custom"));
            pv
        })
        .collect();
    v["patterns"] = json!(patterns);
    v["status"] = json!(d.status);
    v["modified"] = json!(d.modified);
    v["removed_spec_patterns"] = json!(d.patterns.iter().filter(|p| p.status == "removed").map(|p| p.pattern).collect::<Vec<_>>());
    v["used_by"] = json!(used_by);
    v["warnings"] = json!(warnings);
    v["max_od"] = clean_json(&r1(f.max_od()));
    v
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn views_and_diffs_keep_short_float_notation() {
        let lib = Library::from_seed(seed());
        let hp = lib.flow("HP_IN").unwrap();
        let text = serde_json::to_string(&flow_view(hp, &[])).unwrap();
        assert!(!text.contains("0.5033000"), "no f32→f64 widening noise in flow view");
        let p4 = hp.pattern(4).unwrap();
        let v = flow_view(hp, &[]);
        let row = v["patterns"].as_array().unwrap().iter().find(|p| p["pattern"] == json!(4)).unwrap();
        assert_eq!(row["slots"][0]["u"], clean_json(&p4.slots[0].u));
        assert_eq!(row["slots"][0]["u"].to_string(), serde_json::to_string(&p4.slots[0].u).unwrap());
        let mut edited = hp.clone();
        edited.patterns.iter_mut().find(|p| p.pattern == 4).unwrap().slots[3].u[0] += 0.1;
        let d = serde_json::to_string(&diff_flow(&edited)).unwrap();
        assert!(d.contains("0.6033") && !d.contains("0.603299"), "{d}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-09-15T12:00:00+09:00";

    fn snap() -> Snapshot {
        Snapshot { lib: Library::from_seed(seed()), usage: BTreeMap::new() }
    }

    fn body_of(p: &SpecPattern) -> PatternBody {
        PatternBody { pattern: None, od_min: p.od_min, od_max: p.od_max, slots: p.slots.clone(), note: p.note.clone() }
    }

    fn msg(e: ApiError) -> String {
        e.to_string()
    }

    #[test]
    fn seed_flows_validate_clean() {
        for f in &Library::from_seed(seed()).flows {
            let w = validate_flow(f).unwrap_or_else(|e| panic!("{}: {e:?}", f.id));
            assert!(w.iter().all(|x| !x.contains("no pattern for OD") && !x.contains("MinDistance")), "{}: {w:?}", f.id);
            let d = diff_flow(f);
            assert_eq!((d.status, d.modified), ("spec", false), "{}", f.id);
        }
    }

    #[test]
    fn flow_ids() {
        assert_eq!(normalize_flow_id(" hp_in_site ").unwrap(), "HP_IN_SITE");
        for bad in ["A", "HP IN", "HP-IN", "한글", &"A".repeat(33)] {
            assert!(normalize_flow_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pattern_validation_errors() {
        let s = snap();
        let p3 = s.lib.flow("HP_IN").unwrap().pattern(3).unwrap().clone();
        let upsert = |b: PatternBody| upsert_pattern(&s, "HP_IN", 3, &b, NOW).map(|_| ()).map_err(msg);
        let mut b = body_of(&p3);
        b.slots[1].seq = 1;
        assert!(upsert(b).unwrap_err().contains("Seq 는 1..3"));
        let mut b = body_of(&p3);
        b.slots[1].drag_dir = 9;
        assert!(upsert(b).unwrap_err().contains("DragDir 9"));
        let mut b = body_of(&p3);
        b.slots[2].slot = "C#1".into();
        assert!(upsert(b).unwrap_err().contains("중복"));
        let mut b = body_of(&p3);
        b.slots = (1..=21).map(|i| SpecSlot { slot: format!("C#{i}"), seq: i as u8, u: [i as f32 * 1.1, 0.0], drag_dir: 0 }).collect();
        assert!(upsert(b).unwrap_err().contains("슬롯 수 21"));
        let mut b = body_of(&p3);
        b.slots.clear();
        assert!(upsert(b).unwrap_err().contains("슬롯 수 0"));
        let mut b = body_of(&p3);
        (b.od_min, b.od_max) = (800.0, 790.0);
        assert!(upsert(b).unwrap_err().contains("OdMin"));
        let mut b = body_of(&p3);
        b.slots[1].u = b.slots[0].u;
        assert!(upsert(b).unwrap_err().contains("같은 자리"));
        // P3 range reaching into P4 (663..755)
        let mut b = body_of(&p3);
        b.od_min = 750.0;
        let e = upsert(b).unwrap_err();
        assert!(e.contains("겹칩니다") && e.contains("Pattern 4"), "{e}");
        // renumber onto an existing pattern
        let b = PatternBody { pattern: Some(4), ..body_of(&p3) };
        assert!(matches!(upsert_pattern(&s, "HP_IN", 3, &b, NOW), Err(ApiError::Conflict(_))));
        // unknown flow
        assert!(matches!(upsert_pattern(&s, "NOPE", 3, &body_of(&p3), NOW), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn pattern_warnings_gaps_and_scale() {
        let s = snap();
        let p4 = s.lib.flow("HP_IN").unwrap().pattern(4).unwrap().clone();
        let mut b = body_of(&p4);
        b.od_max = 700.0; // 701..755 gap before P3
        for sl in &mut b.slots {
            sl.u = [sl.u[0] * 0.9, sl.u[1] * 0.9];
        }
        let first = b.slots.iter_mut().find(|x| x.seq == 1).unwrap();
        first.drag_dir = 2;
        let (f, p, w) = upsert_pattern(&s, "hp_in", 4, &b, NOW).unwrap();
        assert_eq!((f.id.as_str(), p.updated_at.as_str(), f.updated_at.as_str()), ("HP_IN", NOW, NOW));
        assert!(w.iter().any(|x| x.contains("no pattern for OD 700~756")), "{w:?}");
        assert!(w.iter().any(|x| x.contains("MinDistance 0.9")), "{w:?}");
        assert!(w.iter().any(|x| x.contains("첫 작업을 드래그 없이")), "{w:?}");
        assert!(f.pattern_for_od(720.0).is_err());
        // unchanged upsert keeps the pattern timestamp
        let mut s2 = snap();
        s2.lib.flows[0].patterns[0].updated_at = "old".into();
        let p2 = s2.lib.flows[0].patterns[0].clone();
        let (_, p, _) = upsert_pattern(&s2, &s2.lib.flows[0].id.clone(), p2.pattern, &body_of(&p2), NOW).unwrap();
        assert_eq!(p.updated_at, "old");
    }

    #[test]
    fn create_copy_rename_delete() {
        let mut s = snap();
        assert!(matches!(create_flow(&s, &CreateFlow { id: "hp_in".into(), copy_from: Some("OP_IN".into()), ..Default::default() }, NOW), Err(ApiError::Conflict(_))));
        assert!(matches!(create_flow(&s, &CreateFlow { id: "X".into(), drag_kind: Some(DragKind::In), ..Default::default() }, NOW), Err(ApiError::BadRequest(_))));
        assert!(create_flow(&s, &CreateFlow { id: "NEW_FLOW".into(), ..Default::default() }, NOW).unwrap_err().to_string().contains("drag_kind"));
        let (c, _) = create_flow(&s, &CreateFlow { id: "hp_in_site".into(), copy_from: Some("HP_IN".into()), ..Default::default() }, NOW).unwrap();
        assert_eq!((c.id.as_str(), c.source.as_str(), c.based_on.as_deref(), c.patterns.len()), ("HP_IN_SITE", SOURCE_CUSTOM, Some("HP_IN"), 7));
        assert_eq!(diff_flow(&c).status, "spec");
        let (e, _) = create_flow(&s, &CreateFlow { id: "EMPTY".into(), drag_kind: Some(DragKind::Out), ..Default::default() }, NOW).unwrap();
        assert_eq!((e.name.as_str(), diff_flow(&e).status, e.screen_axes.clone()), ("EMPTY", "custom", ScreenAxes::default()));
        s.lib.flows.push(c);
        s.usage.insert("HP_IN_SITE".into(), vec![2021, 2022]);
        let err = delete_flow(&s, "hp_in_site").unwrap_err();
        assert!(matches!(err, ApiError::Conflict(_)) && err.to_string().contains("2021, 2022"), "{err}");
        assert_eq!(delete_flow(&s, "OP_IN").unwrap().delete, vec!["OP_IN".to_string()]);
        let (f, ch, _) = update_flow(&s, "HP_IN_SITE", &FlowPatch { id: Some("site_a".into()), name: Some(" Site A ".into()), ..Default::default() }, NOW).unwrap();
        assert_eq!((f.id.as_str(), f.name.as_str(), ch.rename.clone()), ("SITE_A", "Site A", Some(("HP_IN_SITE".to_string(), "SITE_A".to_string()))));
        assert!(matches!(update_flow(&s, "HP_IN_SITE", &FlowPatch { id: Some("OP_IN".into()), ..Default::default() }, NOW), Err(ApiError::Conflict(_))));
        assert!(update_flow(&s, "HP_IN", &FlowPatch { screen_axes: Some(ScreenAxes { right: "Y+".into(), down: "Y-".into() }), ..Default::default() }, NOW).is_err());
    }

    #[test]
    fn diff_and_reset() {
        let mut s = snap();
        let p3 = s.lib.flow("HP_IN").unwrap().pattern(3).unwrap().clone();
        let mut b = body_of(&p3);
        b.slots.iter_mut().find(|x| x.slot == "C#2").unwrap().drag_dir = 5;
        b.note = "현장 확인".into();
        let (f, _, _) = upsert_pattern(&s, "HP_IN", 3, &b, NOW).unwrap();
        let d = diff_flow(&f);
        assert_eq!((d.status, d.modified), ("modified", true));
        let pd = d.patterns.iter().find(|p| p.pattern == 3).unwrap();
        assert_eq!(pd.status, "changed");
        assert_eq!(pd.fields.iter().map(|x| x.field.as_str()).collect::<Vec<_>>(), vec!["note"]);
        let c2 = pd.slots.iter().find(|x| x.slot == "C#2").unwrap();
        assert_eq!((c2.status, c2.fields[0].field.as_str(), c2.fields[0].spec.clone(), c2.fields[0].current.clone()), ("changed", "drag_dir", json!(3), json!(5)));
        assert!(d.patterns.iter().filter(|p| p.pattern != 3).all(|p| p.status == "same"));
        // note only is not "modified"
        let mut only_note = s.lib.flow("HP_IN").unwrap().clone();
        only_note.patterns[0].note = "memo".into();
        assert_eq!(diff_flow(&only_note).status, "spec");
        // removed pattern
        let i = s.lib.flows.iter().position(|x| x.id == "HP_IN").unwrap();
        s.lib.flows[i] = f;
        s.lib.flows[i].patterns.retain(|p| p.pattern != 9);
        let d = diff_flow(&s.lib.flows[i]);
        assert_eq!(d.patterns.iter().find(|p| p.pattern == 9).unwrap().status, "removed");
        // reset one pattern → P3 back, P9 still removed
        let r = reset_flow(&s, "HP_IN", Some(3), NOW).unwrap();
        let d = diff_flow(&r);
        assert_eq!(d.patterns.iter().find(|p| p.pattern == 3).unwrap().status, "same");
        assert_eq!(d.status, "modified");
        // whole flow
        s.lib.flows[i] = r;
        s.lib.flows[i].note = "keep".into();
        let r = reset_flow(&s, "HP_IN", None, NOW).unwrap();
        assert_eq!((diff_flow(&r).status, r.patterns.len(), r.note.as_str(), r.source.as_str()), ("spec", 7, "keep", SOURCE_SPEC));
        assert!(r.patterns.iter().all(|p| p.updated_at == NOW));
        assert!(matches!(reset_flow(&s, "HP_IN", Some(7), NOW), Err(ApiError::NotFound(_))));
        let (e, _) = create_flow(&s, &CreateFlow { id: "EMPTY".into(), drag_kind: Some(DragKind::In), ..Default::default() }, NOW).unwrap();
        s.lib.flows.push(e);
        assert!(matches!(reset_flow(&s, "EMPTY", None, NOW), Err(ApiError::BadRequest(_))));
    }

    #[test]
    fn import_merge_and_errors() {
        let s = snap();
        let mut doc = to_doc(&s.lib, NOW);
        // unchanged round trip
        let (w, r) = merge_import(&s.lib, &doc, NOW);
        assert!(r.ok && w.is_empty(), "{:?}", r.all_errors());
        assert_eq!((r.created, r.updated, r.unchanged), (0, 0, s.lib.flows.len()));
        // change OP_EXT_IN P3 order, add a new flow with one pattern, keep only 2 flows in the doc (others untouched)
        doc.flows.retain(|f| f.id == "OP_EXT_IN" || f.id == "HP_IN");
        let ext = doc.flows.iter_mut().find(|f| f.id == "OP_EXT_IN").unwrap();
        ext.patterns.retain(|p| p.pattern == 3);
        let p3 = &mut ext.patterns[0];
        let (a, b) = (p3.slots.iter().position(|x| x.seq == 2).unwrap(), p3.slots.iter().position(|x| x.seq == 3).unwrap());
        p3.slots[a].seq = 3;
        p3.slots[b].seq = 2;
        let mut site = doc.flows.iter().find(|f| f.id == "HP_IN").unwrap().clone();
        site.id = "site_b".into();
        site.based_on = Some("HP_IN".into());
        site.patterns.truncate(1);
        doc.flows.push(site);
        let (w, r) = merge_import(&s.lib, &doc, NOW);
        assert!(r.ok, "{:?}", r.all_errors());
        assert_eq!((r.created, r.updated, r.unchanged), (1, 1, 1));
        let ext_row = r.flows.iter().find(|f| f.id == "OP_EXT_IN").unwrap();
        assert_eq!(ext_row.patterns, vec![ImportPatternRow { pattern: 3, action: "changed" }]);
        let ext_new = w.iter().find(|f| f.id == "OP_EXT_IN").unwrap();
        assert_eq!(ext_new.patterns.len(), s.lib.flow("OP_EXT_IN").unwrap().patterns.len(), "patterns not in the doc are kept");
        assert_eq!(ext_new.pattern(3).unwrap().updated_at, NOW);
        let site = w.iter().find(|f| f.id == "SITE_B").unwrap();
        assert_eq!((site.source.as_str(), site.based_on.as_deref()), (SOURCE_IMPORT, Some("HP_IN")));
        // overlap error → nothing to write
        let mut bad_doc = doc.clone();
        bad_doc.flows[0].patterns.iter_mut().find(|p| p.pattern == 3).unwrap().od_min = 700.0;
        bad_doc.flows[0].id = "HP_IN".into();
        let (w, r) = merge_import(&s.lib, &bad_doc, NOW);
        assert!(!r.ok && w.is_empty());
        assert!(r.all_errors().iter().any(|e| e.contains("겹칩니다")), "{:?}", r.all_errors());
        // bad id + duplicate flow
        let mut bad_doc = doc.clone();
        bad_doc.flows.push(bad_doc.flows[0].clone());
        bad_doc.flows[1].id = "bad id".into();
        let (_, r) = merge_import(&s.lib, &bad_doc, NOW);
        assert!(!r.ok && r.all_errors().len() >= 2, "{:?}", r.all_errors());
        let (_, r) = merge_import(&s.lib, &Spec { flows: vec![], ..doc.clone() }, NOW);
        assert!(!r.ok);
        // spec_r4.json itself imports cleanly (unchanged)
        let raw: Spec = serde_json::from_str(include_str!("spec_r4.json")).unwrap();
        let (_, r) = merge_import(&s.lib, &raw, NOW);
        assert!(r.ok && r.unchanged == raw.flows.len(), "{:?}", r);
    }

    #[test]
    fn view_has_min_distance_status_usage() {
        let s = snap();
        let v = flow_view(s.lib.flow("OP_OUT").unwrap(), &[2101]);
        assert_eq!(v["status"], json!("spec"));
        assert_eq!(v["used_by"], json!([2101]));
        assert_eq!(v["patterns"][0]["status"], json!("same"));
        assert!((v["patterns"][0]["min_distance"].as_f64().unwrap() - 1.0).abs() < 1e-3);
        assert_eq!(v["max_od"], json!(937.0));
    }
}
