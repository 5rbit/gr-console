//! 화물 규격 확장 — 콘솔이 소유하는 품목 부가 규격(`tire_codes.spec_json`, migration `0006_console_v2`).
//!
//! PLC 와이어(`LGR_Stock_Item`: Code/Count/InnerDiameter/OuterDiameter/LowerBidHeight/UpperBidHeight/Height/
//! DeflectionFactor)는 바꾸지 않는다. 여기 값은 콘솔의 Z 규칙·재고 한도·표시에만 쓴다.
//!
//! # 정본은 스택 크기별 **절대** 프로파일이다
//!
//! GR PLC 의 `measureSKU` 는 스택 하나를 통째로 재서 **셀 바닥 기준 절대** 비드를 준다. 그 값을 잰 스택
//! 크기별로 그대로 보관한다([`BeadProfile`]) — 같은 크기의 스택을 집을 때 환산도 합산도 없이 바로 쓰려고.
//! 크기가 다른 프로파일은 나란히 산다(n = 3 · 5 · 8 …). 같은 크기를 다시 재면 그 프로파일이 통째로 바뀐다
//! (운전자가 손으로 고친 행만 남는다 — `registry::beads`).
//!
//! # 하중(`Above`) 곡선은 파생값이다
//!
//! 잰 적 없는 크기를 위해서는 여전히 하중 곡선이 필요하다. 타이어가 얼마나 눌리는지는 **그 위에 몇 개가
//! 얹혀 있는가**(`Above`)로 정해지므로 — 5 개 스택의 3 단(위 2 개)과 8 개 스택의 3 단(위 5 개)은 다른 값이다 —
//! 프로파일에서 `above = n − level` 을 키로 잡아 **타이어 자기 바닥 기준** 값으로 옮긴 곡선([`AboveRow`])을
//! 그때그때 만든다([`curve_of`], 저장하지 않는다).
//!
//! # 집는 Z 를 푸는 차례
//!
//! 잡는 타이어가 **설** 스택 크기는 `size = level + above` (PICK 은 `n`, DROP 은 `n + 1`).
//! 1. `size` 를 통째로 잰 프로파일에 그 단이 있으면 잰 **절대** 상부 비드를 그대로 쓴다 —
//!    `Z = floor + AbsUpperBead(size, level) − PickBeadOffset`. 환산도 아래 타이어 합산도 없다.
//! 2. 없으면 파생 곡선으로 — 정확한 점 → 이웃 점 사이 보간(`interpolated`). 잡는 타이어 **아래** 타이어들은
//!    각자 자기 `Above`(= `n − j`)의 `pressed_height` 로 쌓인다.
//! 3. 잰 게 하나도 없으면 그립점은 `mid`(Height/2)로 내려가고, 아래 타이어 높이만 스칼라 `Compression`
//!    모형으로 짓는다(`computed`).
//!
//! 자세한 식과 PLC `p973`(Pick_BeadZOffset) 과의 관계는 `docs/item-spec-z.md`.

use gr_proto::StockItem;
use serde::{Deserialize, Serialize};

/// `stack_max` 상한(콘솔 검증).
pub const STACK_MAX_LIMIT: u8 = 20;
/// `PickBeadOffset` 기본값(mm) — 상부 비드에서 이만큼 아래를 잡는다.
pub const DEFAULT_PICK_BEAD_OFFSET: f32 = 30.0;
/// 눌림을 먹인 뒤에도 남겨 두는 최소 단 높이·비드 높이(mm).
pub const MIN_PITCH: f32 = 1.0;
/// 단 번호·`Above` 상한.
pub const LEVEL_MAX: u8 = 20;
/// MEASLOG SKU 항목의 `Data` 는 7단까지 비드 쌍을 싣는다(`Data[2k-1]`, `Data[2k]`).
pub const SKU_LOG_LEVELS: usize = 7;
/// 하중이 커지는데 값이 커지는 것을 경고하는 문턱(mm) — 0.1 mm 반올림 잡음으로 경고가 뜨지 않게.
pub const MONOTONIC_TOL: f32 = 0.5;
/// 측정 `TotalHeight` 와 단별 pitch 합이 이만큼(mm) 넘게 어긋나면 측정을 의심한다.
pub const TOTAL_HEIGHT_TOL: f32 = 5.0;

/// 곡선 한 점의 출처.
pub const SOURCE_MEASURED: &str = "measured";
pub const SOURCE_MANUAL: &str = "manual";
pub const SOURCE_COMPUTED: &str = "computed";

/// 하중(`above`) 하나에 대한 비드 곡선의 한 점. 비드는 **그 타이어 바닥 기준**이다.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AboveRow {
    /// 이 행이 말하는 하중 — 그 타이어 **위에 얹힌 개수**.
    pub above: u8,
    /// 타이어 바닥에서 하부 비드까지(mm).
    pub lower_bead: Option<f32>,
    /// 타이어 바닥에서 상부 비드까지(mm) — 그립 기준.
    pub upper_bead: Option<f32>,
    /// 그 하중에서 타이어 하나가 차지하는 높이(mm).
    pub pressed_height: Option<f32>,
    /// `measured` | `manual` | `computed`.
    pub source: String,
    /// 이 점을 채운 SKU 표본(PLC 이름, 비면 없음).
    pub sample_plc: String,
    pub sample_seq: u32,
    pub updated_at: String,
}

impl AboveRow {
    pub fn is_manual(&self) -> bool {
        self.source == SOURCE_MANUAL
    }
    pub fn is_measured(&self) -> bool {
        self.source == SOURCE_MEASURED
    }
    pub fn sample(&self) -> Option<SampleRef> {
        (self.sample_seq != 0).then(|| SampleRef { plc: self.sample_plc.clone(), seq: self.sample_seq })
    }
    fn empty(&self) -> bool {
        self.lower_bead.is_none() && self.upper_bead.is_none() && self.pressed_height.is_none()
    }
}

/// 잰 스택 한 단의 **셀 바닥 기준 절대** 측정값 — PLC 가 준 그대로 보관한다(정본).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileRow {
    /// 1-based 단 번호(맨 아래가 1).
    pub level: u8,
    /// 셀 바닥에서 하부 비드까지(mm).
    pub lower_bead: Option<f32>,
    /// 셀 바닥에서 상부 비드까지(mm) — 이 단을 집을 때의 그립 기준.
    pub upper_bead: Option<f32>,
    /// 셀 바닥에서 이 단 타이어 윗면까지(mm). 맨 윗단은 표본의 `TotalHeight`.
    pub stack_height: Option<f32>,
    /// `measured` | `manual`(운전자가 고친 행 — 다시 재도 덮지 않는다).
    pub source: String,
}

impl ProfileRow {
    pub fn is_manual(&self) -> bool {
        self.source == SOURCE_MANUAL
    }
    /// 빈 행(값이 하나도 없다).
    pub fn empty(&self) -> bool {
        self.lower_bead.is_none() && self.upper_bead.is_none() && self.stack_height.is_none()
    }
    fn values(&self) -> LevelValues {
        LevelValues { lower_bead: self.lower_bead, upper_bead: self.upper_bead, stack_height: self.stack_height }
    }
}

/// **정본** — 스택 크기 `count` 를 통째로 잰 결과. 같은 크기를 다시 재면 이 프로파일이 통째로 바뀐다
/// (운전자가 `manual` 로 표시한 행만 남는다). 크기가 다른 프로파일은 나란히 산다(n = 3 · 5 · 8 …).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeadProfile {
    /// 잰 스택의 단수.
    pub count: u8,
    /// 1 단부터의 절대 측정값.
    pub rows: Vec<ProfileRow>,
    /// 표본의 `TotalHeight`(mm).
    pub total_height: Option<f32>,
    /// 표본의 `EachHeight`(mm).
    pub each_height: Option<f32>,
    pub sample_plc: String,
    pub sample_seq: u32,
    /// 이 프로파일을 채운 때(정렬·표시).
    pub at: String,
}

impl BeadProfile {
    pub fn at_level(&self, level: u32) -> Option<&ProfileRow> {
        self.rows.iter().find(|r| r.level as u32 == level && !r.empty())
    }
    /// 이 단을 집을 때 쓰는 **절대** 상부 비드(mm, 셀 바닥 기준).
    pub fn abs_upper_bead(&self, level: u32) -> Option<f32> {
        self.at_level(level).and_then(|r| r.upper_bead)
    }
    pub fn sample(&self) -> Option<SampleRef> {
        (self.sample_seq != 0).then(|| SampleRef { plc: self.sample_plc.clone(), seq: self.sample_seq })
    }
    /// 1 단부터의 절대값(`stack_profile` 입력 모양).
    pub fn level_values(&self) -> Vec<LevelValues> {
        (1..=self.count as u32).map(|k| self.at_level(k).map(ProfileRow::values).unwrap_or_default()).collect()
    }
    fn normalized(mut self) -> Self {
        self.rows.sort_by_key(|r| r.level);
        self.rows.dedup_by_key(|r| r.level);
        self.rows.retain(|r| !r.empty() && r.level >= 1 && r.level as u32 <= self.count as u32);
        for r in self.rows.iter_mut() {
            if r.source.trim() != SOURCE_MANUAL {
                r.source = SOURCE_MEASURED.into();
            }
        }
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemSpec {
    /// 셀 최대 단수(0 = 제한 없음) — 재고 한도·검증.
    pub stack_max: u8,
    /// 팔레트당 최대 개수(0 = 제한 없음) — 표시·관리용.
    pub pallet_max: u8,
    pub weight_kg: Option<f32>,
    /// **정본** — 잰 스택 크기별 절대 프로파일. 하중(`Above`) 곡선은 여기서 파생된다([`curve_of`]).
    pub profiles: Vec<BeadProfile>,
    /// 비드 값의 출처 메모(예: "SKU 측정 GR2 seq 1234 (2026-09-18 10:11)").
    pub bead_source: String,
    /// `pick_bead` 그립에서 상부 비드 아래로 내려잡는 양(mm). `None` = 기본 30.
    pub pick_bead_offset: Option<f32>,
    /// 잰 적 없는 하중을 위한 **대체** 눌림양(mm/개) — 위에 한 개 얹힐 때마다 줄어드는 높이·비드.
    /// `None` = 0(보정 없음). 프로파일을 그대로 쓰는 갈래에서는 쓰지 않는다.
    pub compression: Option<f32>,
    /// 눌림양의 출처 메모.
    pub compression_source: String,
    /// SKU 측정이 들어오면 프로파일·눌림양을 자동으로 갱신한다. `None` = 켬(기본).
    pub auto_apply_measured: Option<bool>,
}

impl ItemSpec {
    /// 스택 크기 `count` 를 통째로 잰 프로파일.
    pub fn profile(&self, count: u32) -> Option<&BeadProfile> {
        self.profiles.iter().find(|p| p.count as u32 == count && !p.rows.is_empty())
    }
    pub fn profile_mut(&mut self, count: u32) -> Option<&mut BeadProfile> {
        self.profiles.iter_mut().find(|p| p.count as u32 == count)
    }
    /// 잰 스택 크기 목록(오름차순).
    pub fn measured_counts(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.profiles.iter().filter(|p| !p.rows.is_empty()).map(|p| p.count as u32).collect();
        v.sort_unstable();
        v
    }
    /// 실제로 쓰는 `PickBeadOffset`(비었거나 이상한 값이면 기본 30 mm).
    pub fn pick_bead_offset(&self) -> f32 {
        self.pick_bead_offset.filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(DEFAULT_PICK_BEAD_OFFSET)
    }
    /// SKU 측정 자동 반영 스위치(비면 켬).
    pub fn auto_apply_measured(&self) -> bool {
        self.auto_apply_measured.unwrap_or(true)
    }
    /// 잰 적 없는 하중에 쓰는 대체 눌림양(비었거나 이상한 값이면 0 = 보정 없음).
    pub fn compression(&self) -> f32 {
        self.compression.filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0)
    }
    /// 단별 보기·제안이 "가득 쌓은 스택"으로 삼는 개수(StackMax, 없으면 잰 가장 큰 스택).
    pub fn reference_stack(&self) -> u32 {
        if self.stack_max > 0 { self.stack_max as u32 } else { view_count(self) }
    }
    /// 정렬·중복 제거.
    pub fn normalized(mut self) -> Self {
        self.profiles = self.profiles.into_iter().map(BeadProfile::normalized).filter(|p| p.count > 0 && !p.rows.is_empty()).collect();
        self.profiles.sort_by_key(|p| p.count);
        self.profiles.dedup_by_key(|p| p.count);
        self.bead_source = self.bead_source.trim().to_string();
        self.compression_source = self.compression_source.trim().to_string();
        self
    }
    /// `count` 개가 `stack_max` 를 넘는가(0 = 제한 없음).
    pub fn exceeds(&self, count: u32) -> bool {
        self.stack_max > 0 && count > self.stack_max as u32
    }
    /// 프로파일이 덮는 하중 목록(오름차순) — 파생 곡선이 값을 가지는 `Above`.
    pub fn measured_aboves(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .profiles
            .iter()
            .flat_map(|p| p.rows.iter().filter(|r| !r.empty()).map(move |r| (p.count as u32).saturating_sub(r.level as u32)))
            .filter(|a| self.stack_max == 0 || *a < self.stack_max as u32)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

// ── 검증 ────────────────────────────────────────────────────────────────────

fn finite_nonneg(label: &str, v: Option<f32>) -> Result<(), String> {
    match v {
        Some(x) if !x.is_finite() || x < 0.0 => Err(format!("{label} = {x} must be a finite number >= 0")),
        _ => Ok(()),
    }
}

/// 엄격한 모순만 거부한다(400). 단이 올라가는데 값이 내려가는 등은 `spec_warnings`.
pub fn validate_spec(s: &ItemSpec) -> Result<(), String> {
    if s.stack_max > STACK_MAX_LIMIT {
        return Err(format!("stack_max {} must be 0..={STACK_MAX_LIMIT}", s.stack_max));
    }
    finite_nonneg("weight_kg", s.weight_kg)?;
    finite_nonneg("pick_bead_offset", s.pick_bead_offset)?;
    finite_nonneg("compression", s.compression)?;
    let mut counts = std::collections::HashSet::new();
    for p in &s.profiles {
        if !(1..=LEVEL_MAX).contains(&p.count) {
            return Err(format!("stack {} must be 1..={LEVEL_MAX}", p.count));
        }
        if !counts.insert(p.count) {
            return Err(format!("stack {} is duplicated", p.count));
        }
        finite_nonneg(&format!("stack {} total_height", p.count), p.total_height)?;
        finite_nonneg(&format!("stack {} each_height", p.count), p.each_height)?;
        let mut seen = std::collections::HashSet::new();
        for r in &p.rows {
            let at = format!("stack {} level {}", p.count, r.level);
            if r.level == 0 || r.level > p.count {
                return Err(format!("{at}: level must be 1..={}", p.count));
            }
            if !seen.insert(r.level) {
                return Err(format!("{at} is duplicated"));
            }
            finite_nonneg(&format!("{at} lower_bead"), r.lower_bead)?;
            finite_nonneg(&format!("{at} upper_bead"), r.upper_bead)?;
            finite_nonneg(&format!("{at} stack_height"), r.stack_height)?;
            if let (Some(lo), Some(up)) = (r.lower_bead, r.upper_bead)
                && up <= lo
            {
                return Err(format!("{at}: upper_bead {up} must be > lower_bead {lo}"));
            }
            if !matches!(r.source.as_str(), "" | SOURCE_MEASURED | SOURCE_MANUAL) {
                return Err(format!("{at}: source {} must be measured | manual", r.source));
            }
        }
    }
    Ok(())
}

/// `validate_spec` + 품목 높이에 기대는 규칙(`Height` 를 아는 곳에서만 쓸 수 있다).
pub fn validate_spec_for(s: &ItemSpec, item: &StockItem) -> Result<(), String> {
    validate_spec(s)?;
    let h = eff_height(item);
    if h <= 0.0 {
        return Ok(());
    }
    if let Some(v) = s.pick_bead_offset.filter(|v| *v > h) {
        return Err(format!("pick_bead_offset {v} must be <= Height {h}"));
    }
    if let Some(v) = s.compression.filter(|v| *v >= h) {
        return Err(format!("compression {v} must be < Height {h}"));
    }
    for p in &s.profiles {
        // 절대값이라 위로 갈수록 커진다 — 한 단이 Height × 1.5 보다 더 올라가면 측정이 아니다.
        for r in &p.rows {
            let ceiling = h * 1.5 * r.level as f32;
            if let Some(v) = [r.upper_bead, r.stack_height].into_iter().flatten().find(|v| *v > ceiling) {
                return Err(format!("stack {} level {}: {v} must be <= Height {h} × 1.5 × {}", p.count, r.level, r.level));
            }
        }
    }
    Ok(())
}

/// 모순은 아니지만 의심스러운 값 — 단이 올라가는데 절대 비드가 내려가는 곳, 하중이 커지는데 파생 곡선의
/// 값이 커지는 곳, 높이에 견준 눌림·그립 값, 그리고 잰 `TotalHeight` 와 단별 pitch 합이 어긋나는 곳.
pub fn spec_warnings(s: &ItemSpec, item: Option<&StockItem>) -> Vec<String> {
    let mut out = Vec::new();
    // 정본(절대 프로파일)은 품목 치수를 몰라도 본다 — 위 단의 비드가 아래 단보다 낮을 수 없다.
    type PPick = fn(&ProfileRow) -> Option<f32>;
    let pfields: [(&str, PPick); 3] = [("LowerBead", |r| r.lower_bead), ("UpperBead", |r| r.upper_bead), ("StackHeight", |r| r.stack_height)];
    for p in &s.profiles {
        let mut rows: Vec<&ProfileRow> = p.rows.iter().collect();
        rows.sort_by_key(|r| r.level);
        for (label, get) in pfields {
            let mut prev: Option<(u8, f32)> = None;
            for r in &rows {
                let Some(v) = get(r) else { continue };
                if let Some((pl, pv)) = prev
                    && v < pv
                {
                    out.push(format!("{label}: 스택 {} 의 {}단 {} 이 {pl}단 {pv} 보다 낮습니다", p.count, r.level, round1(v)));
                }
                prev = Some((r.level, v));
            }
        }
    }
    let Some(it) = item else { return out };
    let h = eff_height(it);
    // 파생 곡선: 위가 무거운데 값이 커지면 의심스럽다.
    let curve = curve_of(it, Some(s));
    type Pick = fn(&AboveRow) -> Option<f32>;
    let fields: [(&str, Pick); 3] = [("LowerBead", |r| r.lower_bead), ("UpperBead", |r| r.upper_bead), ("PressedHeight", |r| r.pressed_height)];
    for (label, get) in fields {
        let mut prev: Option<(u8, f32)> = None;
        for r in &curve {
            let Some(v) = get(r) else { continue };
            if let Some((pa, pv)) = prev
                && v > pv + MONOTONIC_TOL
            {
                out.push(format!("{label}: Above {} {} > Above {pa} {} — 위가 무거운데 값이 커집니다", r.above, round1(v), round1(pv)));
            }
            prev = Some((r.above, v));
        }
    }
    // PLC 가 따로 준 두 값 — 스택 하나를 통째로 잰 결과라면 TotalHeight = EachHeight × Count 여야 한다.
    for p in &s.profiles {
        if let (Some(t), Some(e)) = (p.total_height, p.each_height)
            && (t - e * p.count as f32).abs() > TOTAL_HEIGHT_TOL
        {
            out.push(format!(
                "스택 {}: 측정 TotalHeight {t} 와 EachHeight {e} × {} = {} 이 {:.1} mm 어긋납니다 — 측정을 확인하세요",
                p.count,
                p.count,
                round1(e * p.count as f32),
                (t - e * p.count as f32).abs()
            ));
        }
        // 맨 윗단이 들고 있는 적재 높이도 같은 값을 말해야 한다.
        if let (Some(t), Some(top)) = (p.total_height, p.at_level(p.count as u32).and_then(|r| r.stack_height))
            && (t - top).abs() > TOTAL_HEIGHT_TOL
        {
            out.push(format!("스택 {}: TotalHeight {t} 와 맨 윗단 StackHeight {top} 이 {:.1} mm 어긋납니다", p.count, (t - top).abs()));
        }
    }
    if h > 0.0 {
        if let Some(v) = s.pick_bead_offset.filter(|v| *v > h / 2.0) {
            out.push(format!("PickBeadOffset {v} > Height/2 {} — 그립점이 타이어 아래쪽입니다", h / 2.0));
        }
        let ref_above = s.reference_stack().saturating_sub(1);
        let c = s.compression();
        if c > 0.0 && ref_above > 0 && h - c * (ref_above as f32) < MIN_PITCH {
            out.push(format!("Compression {c} × 위 {ref_above}개가 Height {h} 를 다 먹습니다 — 단 높이를 {MIN_PITCH} 로 제한합니다"));
        }
        if c > 0.0 && it.upper_bid_height > 0.0 && ref_above > 0 && it.upper_bid_height - c * (ref_above as f32) - s.pick_bead_offset() < 0.0 {
            out.push(format!(
                "맨 아래 단(위 {ref_above}개)에서 UpperBidHeight {} − Compression {c}×{ref_above} − PickBeadOffset {} < 0 — 그립점을 0 으로 제한합니다",
                it.upper_bid_height,
                s.pick_bead_offset()
            ));
        }
    }
    out
}

// ── 값과 계산 ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelValues {
    pub lower_bead: Option<f32>,
    pub upper_bead: Option<f32>,
    pub stack_height: Option<f32>,
}

/// 소수 한 자리 반올림 — 측정에서 온 값은 다 이 자리로 맞춘다.
pub fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// 단 높이(계산 기준). `DeflectionFactor` 는 적용하지 않는다(모듈 설명).
pub fn eff_height(item: &StockItem) -> f32 {
    item.height.max(0.0)
}

/// `k` 단(1-based)의 **공칭** 계산값(눌림 없음): 하단 = (k−1)·H + LowerBidHeight, 상단 = (k−1)·H +
/// UpperBidHeight, 적재 = k·H.
pub fn computed_level(item: &StockItem, k: u32) -> LevelValues {
    let h = eff_height(item);
    let below = k.saturating_sub(1) as f32 * h;
    LevelValues { lower_bead: Some(below + item.lower_bid_height), upper_bead: Some(below + item.upper_bid_height), stack_height: Some(k as f32 * h) }
}

/// 보기 행 수: max(stack_max, 잰 가장 큰 스택, 1).
pub fn view_count(s: &ItemSpec) -> u32 {
    let deepest = s.profiles.iter().filter(|p| !p.rows.is_empty()).map(|p| p.count as u32).max().unwrap_or(0);
    (s.stack_max as u32).max(deepest).max(1).min(LEVEL_MAX as u32)
}

/// MEASLOG SKU 항목(`Data[0..19]`)의 단별 측정값: `Data[2k-1]` 하단, `Data[2k]` 상단(k=1..7), `Data[17]` 단수,
/// `Data[19]` 전체 높이(맨 윗단 행에만 싣는다). 값은 셀 바닥 기준 절대값이다. 둘 다 0 인 단은 측정 없음.
pub fn measured_from_sku(data: &[f32]) -> Vec<LevelValues> {
    let at = |i: usize| data.get(i).copied().filter(|v| v.is_finite() && *v > 0.0);
    let count = data.get(17).copied().filter(|v| v.is_finite() && *v > 0.0).map(|v| v.round() as usize).unwrap_or(SKU_LOG_LEVELS);
    let n = count.min(SKU_LOG_LEVELS);
    (1..=n).map(|k| LevelValues { lower_bead: at(2 * k - 1), upper_bead: at(2 * k), stack_height: if k == count { at(19) } else { None } }).collect()
}

// ── 측정 스택 프로파일 ──────────────────────────────────────────────────────

/// 잰 스택의 한 단 — 절대 측정값에서 그 타이어 바닥·자기 기준 비드·차지한 높이를 되짚은 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfilePoint {
    pub level: u32,
    /// 이 단 타이어 위에 있던 개수 — 곡선의 키.
    pub above: u32,
    /// 셀 바닥에서 이 타이어 바닥까지.
    pub bottom: Option<f32>,
    /// 이 타이어가 차지한 높이(mm).
    pub pressed_height: Option<f32>,
    /// 타이어 바닥 기준 하부 비드.
    pub lower_bead: Option<f32>,
    /// 타이어 바닥 기준 상부 비드.
    pub upper_bead: Option<f32>,
    /// 이 하중에서 먹은 총 눌림(mm) = Height − PressedHeight.
    pub compression_at: Option<f32>,
}

/// **한 번의 measureSKU = 스택 하나의 눌림 프로파일 전체.** 단별 상부 비드 차이가 그 단 타이어가 실제로
/// 차지한 높이(pitch)이고, 맨 아래 바닥을 0 으로 잡으면 단마다 타이어 바닥이 어디였는지가 풀린다.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StackProfile {
    /// 잰 스택의 단수.
    pub count: u32,
    pub points: Vec<ProfilePoint>,
    /// 구한 pitch 의 합(mm) — `TotalHeight` 와 맞는지 보는 값.
    pub pitch_sum: Option<f32>,
    /// 표본의 `TotalHeight`.
    pub total_height: Option<f32>,
    pub warnings: Vec<String>,
}

fn pos(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// 측정 스택의 프로파일. `measured` 는 1 단부터(`measured_from_sku`, 셀 바닥 기준 절대값), `count` 는 잰
/// 스택의 단수라 `k` 단 위에는 `count − k` 개가 있었다.
///
/// `pitch(k)` = `LowerBead(k+1) − LowerBead(k)` — 하부 비드는 타이어 바닥 바로 위라 눌림에 거의 안 움직여
/// 타이어 바닥이 어디였는지를 가장 곧게 말해 준다(없으면 상부 비드 차이로). 맨 아래 타이어 바닥을 0 으로
/// 두고 쌓아 올려 단마다 `bottom(k)` 을 얻고, 거기서 자기 기준 비드를 뺀다.
///
/// 맨 윗단은 위에 아무것도 없어 비드 차이가 없다 — `TotalHeight` 에서 아래 단들의 합을 뺀 값을 쓰고, 그게
/// 말이 안 되면 `EachHeight` 로 떨어진다. 측정 검사는 PLC 가 따로 준 두 값(`TotalHeight` 와
/// `EachHeight × TotalCount`)이 서로 맞는지로 한다.
pub fn stack_profile(item: &StockItem, measured: &[LevelValues], count: u32, each_height: Option<f32>, total_height: Option<f32>) -> StackProfile {
    let h = eff_height(item);
    let n = (count as usize).min(measured.len()).min(LEVEL_MAX as usize);
    let mut out = StackProfile { count, total_height: pos(total_height), ..Default::default() };
    if n == 0 || h <= 0.0 {
        return out;
    }
    let at = |k: usize| measured.get(k - 1).cloned().unwrap_or_default();
    let ok = |v: Option<f32>| v.filter(|x: &f32| x.is_finite() && *x > 0.0 && *x <= h * 2.0).map(round1);
    // 아래 n−1 단은 이웃한 비드 차이로 자기 높이를 안다.
    let mut pitches: Vec<Option<f32>> = (1..n)
        .map(|k| {
            let (a, b) = (at(k), at(k + 1));
            ok(pos(b.lower_bead).zip(pos(a.lower_bead)).map(|(hi, lo)| hi - lo).or_else(|| pos(b.upper_bead).zip(pos(a.upper_bead)).map(|(hi, lo)| hi - lo)))
        })
        .collect();
    let below_sum: f32 = pitches.iter().flatten().sum();
    let top = if pitches.iter().all(Option::is_some) { out.total_height.map(|t| t - below_sum).and_then(|v| ok(Some(v))) } else { None };
    pitches.push(top.or_else(|| ok(each_height)));
    let mut bottom = 0.0f32;
    for k in 1..=n {
        let pitch = pitches[k - 1];
        let m = at(k);
        let rel = |v: Option<f32>| pos(v).map(|x| round1(x - bottom)).filter(|x| *x >= 0.0);
        out.points.push(ProfilePoint {
            level: k as u32,
            above: count.saturating_sub(k as u32),
            bottom: Some(round1(bottom)),
            pressed_height: pitch,
            lower_bead: rel(m.lower_bead),
            upper_bead: rel(m.upper_bead),
            compression_at: pitch.map(|p| round1((h - p).max(0.0))),
        });
        bottom += pitch.unwrap_or(0.0);
    }
    if pitches.iter().all(Option::is_some) {
        out.pitch_sum = Some(round1(bottom));
    }
    // PLC 가 준 두 값이 서로 맞는가 — 스택 하나를 통째로 잰 결과라면 TotalHeight = EachHeight × TotalCount.
    if let (Some(t), Some(e)) = (out.total_height, pos(each_height))
        && (t - e * count as f32).abs() > TOTAL_HEIGHT_TOL
    {
        out.warnings.push(format!("TotalHeight {t} 와 EachHeight {e} × TotalCount {count} = {} 이 {:.1} mm 어긋납니다 — 측정을 확인하세요", round1(e * count as f32), (t - e * count as f32).abs()));
    }
    out
}

/// 스택 하나를 통째로 잰 결과(셀 바닥 기준 절대값) — [`put_profile`] 에 넘기는 모양.
#[derive(Clone, Copy, Debug)]
pub struct MeasuredStack<'a> {
    /// 잰 스택의 단수.
    pub count: u32,
    /// 1 단부터의 절대 측정값.
    pub levels: &'a [LevelValues],
    pub each_height: Option<f32>,
    pub total_height: Option<f32>,
    pub sample: Option<&'a SampleRef>,
    /// 반영한 때(정렬·표시).
    pub at: &'a str,
}

/// 잰 스택 하나(**절대값** 그대로)를 정본 프로파일에 넣는다 — 같은 크기의 옛 프로파일을 대신한다.
/// 운전자가 손으로 고친 행(`manual`)은 그대로 두고 `skipped` 로 알린다.
pub fn put_profile(spec: &mut ItemSpec, m: &MeasuredStack, respect_manual: bool) -> FillOutcome {
    let mut out = FillOutcome::default();
    let count = m.count;
    if count == 0 || count > LEVEL_MAX as u32 {
        return out;
    }
    let kept: Vec<ProfileRow> = match spec.profile(count) {
        Some(p) if respect_manual => p.rows.iter().filter(|r| r.is_manual()).cloned().collect(),
        _ => Vec::new(),
    };
    let ok = |v: Option<f32>| v.filter(|x: &f32| x.is_finite() && *x > 0.0).map(round1);
    let mut rows: Vec<ProfileRow> = Vec::new();
    for k in 1..=count {
        if let Some(kept) = kept.iter().find(|r| r.level as u32 == k) {
            rows.push(kept.clone());
            out.skipped.push(k);
            continue;
        }
        let v = m.levels.get(k as usize - 1).cloned().unwrap_or_default();
        let row = ProfileRow { level: k as u8, lower_bead: ok(v.lower_bead), upper_bead: ok(v.upper_bead), stack_height: ok(v.stack_height), source: SOURCE_MEASURED.into() };
        if row.empty() {
            continue;
        }
        rows.push(row);
        out.levels.push(k);
    }
    if rows.is_empty() {
        return out;
    }
    rows.sort_by_key(|r| r.level);
    let next = BeadProfile {
        count: count as u8,
        rows,
        total_height: ok(m.total_height),
        each_height: ok(m.each_height),
        sample_plc: m.sample.map(|s| s.plc.clone()).unwrap_or_default(),
        sample_seq: m.sample.map(|s| s.seq).unwrap_or(0),
        at: m.at.to_string(),
    };
    match spec.profile_mut(count) {
        Some(p) => *p = next,
        None => spec.profiles.push(next),
    }
    spec.profiles.sort_by_key(|p| p.count);
    out
}

/// 어느 SKU 측정 표본이 그 점을 채웠는지.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SampleRef {
    pub plc: String,
    pub seq: u32,
}

/// [`put_profile`] 결과 — 새로 넣은 단과 `manual` 이라 건드리지 않은 단.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FillOutcome {
    /// 새 측정으로 채운 단 번호.
    pub levels: Vec<u32>,
    /// `manual` 이라 덮지 않은 단 번호.
    pub skipped: Vec<u32>,
}

/// 클라이언트가 보낸 프로파일에서 **운전자가 손으로 고친 행**을 표시한다 — 저장된 값과 다르면 `source` 를
/// `manual` 로 올린다. 자동 SKU 반영이 그 행을 덮지 않게 하는 장치이고, 값을 그대로 둔 채 `source` 를
/// `measured` 로 보내면 다시 자동 반영 대상이 된다.
pub fn mark_manual_edits(next: &mut ItemSpec, prev: &ItemSpec) {
    for p in next.profiles.iter_mut() {
        let old = prev.profiles.iter().find(|o| o.count == p.count);
        for row in p.rows.iter_mut() {
            let was = old.and_then(|o| o.rows.iter().find(|r| r.level == row.level));
            let changed = match was {
                Some(o) => (o.lower_bead, o.upper_bead, o.stack_height) != (row.lower_bead, row.upper_bead, row.stack_height),
                None => old.is_some(),
            };
            if changed {
                row.source = SOURCE_MANUAL.into();
            }
        }
    }
}

// ── 스칼라 눌림 되짚기(대체값) ──────────────────────────────────────────────

/// 측정에서 되짚은 **대체** 눌림양 제안(mm/개). 곡선에 점이 없는 하중에만 쓰인다.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CompressionSuggestion {
    /// 채택값 — `from_beads` 가 있으면 그것, 없으면 `from_each_height`.
    pub value: Option<f32>,
    /// `beads` | `each_height`
    pub method: Option<&'static str>,
    pub from_beads: Option<f32>,
    pub from_each_height: Option<f32>,
    pub samples: usize,
    pub measured_count: u32,
}

/// `k` 단 타이어의 상부 비드까지 쌓인 "눌린 개수 합" — 아래 `j<k` 타이어들이 받는 무게 + 자기 위 개수.
fn press_weight(count: u32, k: u32) -> f32 {
    let below: u32 = (1..k).map(|j| count.saturating_sub(j)).sum();
    (below + count.saturating_sub(k)) as f32
}

/// 측정값에서 스칼라 눌림양을 되짚는다(대체값). `measured` 는 1단부터 절대값, `count` 는 잰 스택의 단수.
/// 측정 UpperBead(k) = (k−1)·H + UpperBid − C·press_weight(count, k) 를 원점을 지나는 최소제곱으로 푼다.
pub fn suggest_compression(item: &StockItem, measured: &[LevelValues], count: u32, each_height: Option<f32>) -> CompressionSuggestion {
    let h = eff_height(item);
    let mut num = 0.0f32;
    let mut den = 0.0f32;
    let mut samples = 0usize;
    if h > 0.0 && item.upper_bid_height > 0.0 {
        for (i, m) in measured.iter().enumerate() {
            let k = i as u32 + 1;
            let (Some(v), w) = (m.upper_bead, press_weight(count, k)) else { continue };
            if w <= 0.0 || !v.is_finite() {
                continue;
            }
            let d = ((k - 1) as f32 * h + item.upper_bid_height) - v;
            num += w * d;
            den += w * w;
            samples += 1;
        }
    }
    let from_beads = (den > 0.0 && samples > 0).then(|| round1((num / den).max(0.0)));
    let from_each_height = each_height.filter(|v| v.is_finite() && *v > 0.0 && h > 0.0 && count >= 2).map(|e| round1((2.0 * (h - e) / (count - 1) as f32).max(0.0)));
    let (value, method) = match (from_beads, from_each_height) {
        (Some(v), _) => (Some(v), Some("beads")),
        (None, Some(v)) => (Some(v), Some("each_height")),
        _ => (None, None),
    };
    CompressionSuggestion { value, method, from_beads, from_each_height, samples, measured_count: count }
}

// ── 곡선 풀이 ───────────────────────────────────────────────────────────────

/// 하중 하나에 대해 실제로 쓰는 값과 그 출처.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveValue {
    pub above: u32,
    /// 타이어 바닥 기준 상부 비드(그립 기준). 품목에 비드가 없으면 `None`.
    pub upper_bead: Option<f32>,
    pub lower_bead: Option<f32>,
    /// 그 하중에서 타이어 하나가 차지하는 높이.
    pub pressed_height: f32,
    /// `measured` | `manual` | `interpolated` | `computed`.
    pub source: &'static str,
    /// **상부 비드만** 의 출처 — `measured` | `manual` | `interpolated` | `computed`.
    /// `computed` 는 곡선에 점이 없어 공칭값(UpperBidHeight − Compression·above)으로 지어냈다는 뜻이고,
    /// 그때 그립은 비드를 쓰지 않고 `mid` 로 내려간다(`grip_point`).
    pub upper_bead_source: &'static str,
    /// 이 값을 준 표본(정확히 맞은 측정 점일 때만).
    pub sample: Option<SampleRef>,
}

/// 하중 `above` 를 두 측정 점 사이에서 선형 보간하거나(있으면), 스칼라 모형으로 만든다.
fn interp(rows: &[&AboveRow], above: u32, get: fn(&AboveRow) -> Option<f32>) -> Option<f32> {
    let lo = rows.iter().filter(|r| (r.above as u32) < above && get(r).is_some()).max_by_key(|r| r.above)?;
    let hi = rows.iter().filter(|r| (r.above as u32) > above && get(r).is_some()).min_by_key(|r| r.above)?;
    let (a, b) = (lo.above as f32, hi.above as f32);
    let (va, vb) = (get(lo)?, get(hi)?);
    (b > a).then(|| round1(va + (vb - va) * (above as f32 - a) / (b - a)))
}

/// 정본 프로파일에서 **파생한** 하중(`Above`) 곡선 — 저장하지 않는다.
///
/// `n` 단 스택을 통째로 잰 프로파일의 `k` 단 타이어는 위에 `n − k` 개를 이고 있었다. 절대 측정값에서 그
/// 타이어 바닥을 되짚어(`stack_profile`) 자기 바닥 기준 비드·단 높이로 옮긴다. 크기가 다른 프로파일은
/// 곡선의 다른 구간을 채우고, 같은 하중이 겹치면 **새 표본**이 이긴다(손으로 고친 행은 지지 않는다).
pub fn curve_of(item: &StockItem, spec: Option<&ItemSpec>) -> Vec<AboveRow> {
    let Some(s) = spec else { return Vec::new() };
    let mut by: std::collections::BTreeMap<u8, AboveRow> = std::collections::BTreeMap::new();
    let mut profs: Vec<&BeadProfile> = s.profiles.iter().filter(|p| p.count > 0 && !p.rows.is_empty()).collect();
    profs.sort_by(|a, b| (a.at.as_str(), a.sample_seq, a.count).cmp(&(b.at.as_str(), b.sample_seq, b.count)));
    for p in profs {
        let prof = stack_profile(item, &p.level_values(), p.count as u32, p.each_height, p.total_height);
        for pt in &prof.points {
            if pt.above > LEVEL_MAX as u32 || (s.stack_max > 0 && pt.above >= s.stack_max as u32) {
                continue;
            }
            let manual = p.at_level(pt.level).is_some_and(ProfileRow::is_manual);
            let made = AboveRow {
                above: pt.above as u8,
                lower_bead: pt.lower_bead,
                upper_bead: pt.upper_bead,
                pressed_height: pt.pressed_height,
                source: if manual { SOURCE_MANUAL.into() } else { SOURCE_MEASURED.into() },
                sample_plc: if manual { String::new() } else { p.sample_plc.clone() },
                sample_seq: if manual { 0 } else { p.sample_seq },
                updated_at: p.at.clone(),
            };
            if made.empty() {
                continue;
            }
            if by.get(&made.above).is_some_and(|prev| prev.is_manual()) && !manual {
                continue;
            }
            by.insert(made.above, made);
        }
    }
    by.into_values().collect()
}

/// 하중 `above` 에서 쓰는 값 — 파생 곡선의 점 → 보간 → 스칼라 모형의 차례.
/// 곡선은 부르는 쪽이 [`curve_of`] 로 한 번 만들어 돌려 쓴다.
pub fn curve_at_in(item: &StockItem, spec: Option<&ItemSpec>, curve: &[AboveRow], above: u32) -> CurveValue {
    let h = eff_height(item);
    let c = spec.map(|s| s.compression()).unwrap_or(0.0);
    let fallback_pressed = crate::stock::pressed_height(h, c, above);
    let fallback_upper = (item.upper_bid_height > 0.0).then_some(item.upper_bid_height - c * above as f32);
    let fallback_lower = (item.lower_bid_height > 0.0).then_some(item.lower_bid_height);
    let rows: Vec<&AboveRow> = curve.iter().collect();
    if let Some(r) = curve.iter().find(|r| r.above as u32 == above)
        && (r.upper_bead.is_some() || r.pressed_height.is_some())
    {
        let source = if r.is_manual() {
            SOURCE_MANUAL
        } else if r.is_measured() {
            SOURCE_MEASURED
        } else {
            SOURCE_COMPUTED
        };
        return CurveValue {
            above,
            upper_bead: r.upper_bead.or(fallback_upper),
            lower_bead: r.lower_bead.or(fallback_lower),
            pressed_height: r.pressed_height.unwrap_or(fallback_pressed),
            source,
            // 점은 있어도 UpperBead 칸이 비어 있으면 비드는 여전히 지어낸 값이다.
            upper_bead_source: if r.upper_bead.is_some() { source } else { interp(&rows, above, |r| r.upper_bead).map_or(SOURCE_COMPUTED, |_| "interpolated") },
            sample: r.sample(),
        };
    }
    let up = interp(&rows, above, |r| r.upper_bead);
    let lo = interp(&rows, above, |r| r.lower_bead);
    let pr = interp(&rows, above, |r| r.pressed_height);
    let source = if up.is_some() || pr.is_some() { "interpolated" } else { SOURCE_COMPUTED };
    CurveValue {
        above,
        upper_bead: up.or(fallback_upper),
        lower_bead: lo.or(fallback_lower),
        pressed_height: pr.unwrap_or(fallback_pressed),
        source,
        upper_bead_source: if up.is_some() { "interpolated" } else { SOURCE_COMPUTED },
        sample: None,
    }
}

// ── 그립·Z ──────────────────────────────────────────────────────────────────

/// 스택 위 그립 Z 계산 결과.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StackZ {
    pub z: f32,
    /// `profile`(잰 절대값 그대로) | `curve`(다른 크기 프로파일에서 환산·보간) | `computed`
    pub z_source: &'static str,
    /// 잡는(PICK: 가져갈 묶음의 맨 아래 / DROP: 놓을 묶음의 맨 아래) 타이어의 단.
    pub level: u32,
    /// 그 아래 깔린 타이어 수.
    pub below: u32,
    /// 잡는 타이어 **위에** 얹힌 타이어 수(하중 키) — PICK: c−1, DROP: 0.
    pub above: u32,
    /// 셀 바닥에서 아래 스택 윗면까지.
    pub base: f32,
    /// 잡는 타이어 바닥에서 그립점까지.
    pub grip: f32,
    /// 실제로 쓴 그립 기준(`pick_bead` 를 시켰어도 **측정된** 비드가 없으면 `mid` 로 내려간다).
    pub grip_ref: &'static str,
    /// 쓴 대체 눌림양(mm/개). 0 이면 보정 없음.
    pub compression: f32,
    /// 그립 기준이 된 상부 비드(잡는 타이어 바닥 기준) — `pick_bead` 를 실제로 쓸 때만.
    pub upper_bead: Option<f32>,
    /// 그 비드가 어디서 왔는가 — `measured` | `manual` | `interpolated` | `computed`.
    pub bead_source: &'static str,
    /// 쓴 PickBeadOffset — `pick_bead` 일 때만.
    pub pick_bead_offset: Option<f32>,
    /// 곡선에서 가져온 값들(`pressed[above=3]=232 measured` 같은 형태).
    pub used: Vec<String>,
    /// 제한(클램프)이 걸린 곳 — compose 경고로 올라간다.
    pub warnings: Vec<String>,
}

/// 그립점(잡는 타이어 바닥 기준) 계산 결과.
#[derive(Clone, Debug, PartialEq)]
pub struct GripPoint {
    pub offset: f32,
    /// `mid` | `pick_bead`
    pub used_ref: &'static str,
    pub upper_bead: Option<f32>,
    pub pick_bead_offset: Option<f32>,
    pub warnings: Vec<String>,
}

/// `mid` | `pick_bead` (모르는 값은 `mid`).
///
/// 옛 `bead`(상부 비드를 그대로 잡기)는 없어졌다 — 저장된 값·가져온 파일에 남아 있으면 `pick_bead`
/// (상부 비드 − PickBeadOffset, 화면 이름 `bead+offset`)로 읽는다. 다시 써 내보내지는 않는다.
pub fn normalize_grip_ref(s: &str) -> &'static str {
    match s.trim().to_ascii_lowercase().replace(['-', '+', ' '], "_").as_str() {
        "bead" | "pick_bead" | "pickbead" | "bead_offset" => "pick_bead",
        _ => "mid",
    }
}

/// 위에 `above` 개가 얹힌 타이어의 그립점.
///
/// `pick_bead` 는 그 하중에서 푼 상부 비드(타이어 바닥 기준) − PickBeadOffset 인데, **그 비드가 실제로
/// 측정(또는 손입력·보간)된 값일 때만** 쓴다. 한 번도 재 본 적 없는 품목(`bead_source == computed`,
/// 공칭 UpperBidHeight 로 지어낸 값)은 그립점을 `mid`(Height/2)로 내린다 — 결정 2026-09-18.
pub fn grip_point(grip_ref: &str, item: &StockItem, spec: Option<&ItemSpec>, above: u32, bead: Option<f32>, bead_source: &str) -> GripPoint {
    let want = normalize_grip_ref(grip_ref);
    let mid = crate::stock::grip_offset("mid", item);
    let mut warnings = Vec::new();
    let measured = bead.filter(|_| bead_source != SOURCE_COMPUTED).map(|v| {
        if v < MIN_PITCH {
            warnings.push(format!("UpperBead {v} (위 {above}개 눌림) 이 너무 낮아 {MIN_PITCH} 로 제한했습니다"));
            MIN_PITCH
        } else {
            v
        }
    });
    let mid_point = |mut warnings: Vec<String>, note: Option<String>| {
        if let Some(n) = note {
            warnings.push(n);
        }
        GripPoint { offset: mid, used_ref: "mid", upper_bead: None, pick_bead_offset: None, warnings }
    };
    if want != "pick_bead" {
        return mid_point(warnings, None);
    }
    let Some(b) = measured else {
        return mid_point(warnings, Some(format!("그립 기준 bead+offset: 측정된 비드가 없어 mid(Height/2 = {mid}) 로 잡습니다")));
    };
    let off = spec.map(|s| s.pick_bead_offset()).unwrap_or(DEFAULT_PICK_BEAD_OFFSET);
    let g = b - off;
    if g < 0.0 {
        warnings.push(format!("PickBeadOffset {off} 가 UpperBead {b} 보다 커서 그립점을 0 으로 제한했습니다"));
    }
    GripPoint { offset: g.max(0.0), used_ref: "pick_bead", upper_bead: Some(b), pick_bead_offset: Some(off), warnings }
}

/// `below` = PICK/MEASURE: n − c, DROP: n. 잡는 타이어가 서는 스택 크기는 `size = level + above`
/// (PICK 은 `n`, DROP 은 `n + 1` — 놓고 나면 한 단 높아진다).
///
/// Z 를 푸는 차례:
/// 1. **그 크기를 통째로 잰 프로파일**에 그 단이 있으면 잰 **절대** 상부 비드를 그대로 쓴다 —
///    `Z = floor + AbsUpperBead(size, level) − PickBeadOffset`. 환산도, 아래 타이어 합산도 없다.
/// 2. 없으면 프로파일에서 파생한 하중(`Above`) 곡선으로 — 아래 타이어는 각자 자기 하중의 `PressedHeight`,
///    잡는 타이어의 비드는 자기 하중의 값(정확한 점 → 보간).
/// 3. 잰 게 하나도 없으면 `mid`(Height/2). 스칼라 `Compression` 모형은 2·3 의 아래 타이어 높이에만 쓴다.
pub fn stack_z_with(tt: gr_proto::TaskType, floor: f32, item: &StockItem, spec: Option<&ItemSpec>, grip_ref: &str, n: u32, c: u32) -> StackZ {
    use gr_proto::TaskType;
    let h = eff_height(item);
    let compression = spec.map(|s| s.compression()).unwrap_or(0.0);
    let below = crate::stock::below_count(tt, n, c);
    let level = below + 1;
    let total = n.max(below);
    let above = total.saturating_sub(level);
    if matches!(tt, TaskType::Pick | TaskType::Measure | TaskType::Drop)
        && normalize_grip_ref(grip_ref) == "pick_bead"
        && let Some(s) = spec
        && let Some(p) = s.profile(level + above)
        && let Some(abs) = p.abs_upper_bead(level)
    {
        // (1) 잰 절대값 그대로 — 이 값에 아래 타이어가 이미 다 들어 있다.
        let off = s.pick_bead_offset();
        let mut warnings = Vec::new();
        let bead = if abs < MIN_PITCH {
            warnings.push(format!("AbsUpperBead {abs} (스택 {} 의 {level}단) 이 너무 낮아 {MIN_PITCH} 로 제한했습니다", p.count));
            MIN_PITCH
        } else {
            abs
        };
        if bead - off < 0.0 {
            warnings.push(format!("PickBeadOffset {off} 가 AbsUpperBead {bead} 보다 커서 그립점을 0 으로 제한했습니다"));
        }
        let manual = p.at_level(level).is_some_and(ProfileRow::is_manual);
        let grip = (bead - off).max(0.0);
        let stamp = p.sample().map(|s| format!(" ({} #{})", s.plc, s.seq)).unwrap_or_default();
        return StackZ {
            z: floor + grip,
            z_source: "profile",
            level,
            below,
            above,
            // 절대값이라 아래 스택을 따로 세지 않는다 — 그립점은 **셀 바닥** 기준이다.
            base: 0.0,
            grip,
            grip_ref: "pick_bead",
            compression,
            upper_bead: Some(bead),
            bead_source: if manual { SOURCE_MANUAL } else { SOURCE_MEASURED },
            pick_bead_offset: Some(off),
            used: vec![format!("grip=pick_bead abs(n={},L{level}) {}{stamp}", p.count, round1(bead))],
            warnings,
        };
    }
    if !matches!(tt, TaskType::Pick | TaskType::Measure | TaskType::Drop) {
        return StackZ {
            z: floor,
            z_source: "computed",
            level,
            below,
            above,
            base: 0.0,
            grip: 0.0,
            grip_ref: "mid",
            compression,
            upper_bead: None,
            bead_source: SOURCE_COMPUTED,
            pick_bead_offset: None,
            used: vec![],
            warnings: vec![],
        };
    }
    let mut used = Vec::new();
    let mut warnings = Vec::new();
    let curve = curve_of(item, spec);
    // 아래 j 단 타이어 위에는 total − j 개가 얹혀 있다 — 각자 자기 하중의 눌린 높이로 쌓는다.
    let mut base = 0.0f32;
    for j in 1..=below {
        let v = curve_at_in(item, spec, &curve, total.saturating_sub(j));
        if v.source != SOURCE_COMPUTED {
            used.push(format!("pressed[above={}]={} {}", v.above, v.pressed_height, v.source));
        }
        base += v.pressed_height;
    }
    base = round1(base);
    if compression > 0.0 && below > 0 && crate::stock::pressed_height(h, compression, total.saturating_sub(1)) <= MIN_PITCH {
        warnings.push(format!("Compression {compression} 이 Height {h} 를 다 먹어 맨 아래 단 높이를 {MIN_PITCH} 로 제한했습니다"));
    }
    let grip_v = curve_at_in(item, spec, &curve, above);
    if grip_v.upper_bead_source != SOURCE_COMPUTED && grip_v.upper_bead.is_some() {
        used.push(format!(
            "upper_bead[above={}]={} {}{}",
            above,
            grip_v.upper_bead.unwrap_or_default(),
            grip_v.upper_bead_source,
            grip_v.sample.as_ref().map(|s| format!("({} #{})", s.plc, s.seq)).unwrap_or_default()
        ));
    }
    let g = grip_point(grip_ref, item, spec, above, grip_v.upper_bead, grip_v.upper_bead_source);
    warnings.extend(g.warnings.iter().cloned());
    let z_source = if used.is_empty() { "computed" } else { "curve" };
    // 어느 갈래로 잡았는지는 audit 에 남긴다 — z_source 는 위에서 이미 정했으니 이 줄이 바꾸지 않는다.
    used.push(if g.used_ref == "mid" && normalize_grip_ref(grip_ref) == "pick_bead" {
        format!("grip=mid (측정 없음, Height/2 = {})", g.offset)
    } else if g.used_ref == "pick_bead" {
        // 이 스택 크기를 잰 적이 없어 다른 크기의 프로파일에서 환산한 값이다.
        format!("grip=pick_bead {} converted({})", g.offset, grip_v.upper_bead_source)
    } else {
        format!("grip={} {}", g.used_ref, g.offset)
    });
    StackZ {
        z: floor + base + g.offset,
        z_source,
        level,
        below,
        above,
        base,
        grip: g.offset,
        grip_ref: g.used_ref,
        compression,
        upper_bead: g.upper_bead,
        bead_source: if g.upper_bead.is_some() { grip_v.upper_bead_source } else { SOURCE_COMPUTED },
        pick_bead_offset: g.pick_bead_offset,
        used,
        warnings,
    }
}

/// 단 `k` 를 `total` 개 스택에서 집을 때의 Z(셀 바닥 기준 0).
/// PICK 으로 `total − k + 1` 개를 집으면 잡는 타이어가 `k` 단이고 위에 `total − k` 개가 남는다.
pub fn pick_z_at(item: &StockItem, spec: &ItemSpec, total: u32, k: u32) -> StackZ {
    let c = total.saturating_sub(k) + 1;
    stack_z_with(gr_proto::TaskType::Pick, 0.0, item, Some(spec), "pick_bead", total.max(k), c)
}

// ── 보기(미리보기 스택 크기 기준) ───────────────────────────────────────────

/// 미리보기 스택 `n` 에서 본 한 단.
#[derive(Clone, Debug, Serialize)]
pub struct LevelView {
    /// 미리보기 스택에서의 단 번호(파생값) — `n − above`.
    pub level: u32,
    /// 이 행의 **키**: 위에 얹힌 개수.
    pub above: u32,
    /// 곡선에 저장된 그대로(점이 없으면 `None`).
    pub configured: Option<AboveRow>,
    /// 실제로 쓰는 값(측정 → 보간 → 계산).
    pub effective: CurveValue,
    /// 그 하중에서 먹은 총 눌림(mm) = Height − PressedHeight.
    pub compression_at: Option<f32>,
    /// 미리보기 스택에서 이 타이어 바닥이 서는 높이(셀 바닥 기준).
    pub bottom: f32,
    /// 셀 바닥 기준 절대 비드(미리보기 스택에서) — 화면 표시·측정 대조용.
    pub abs_lower_bead: Option<f32>,
    pub abs_upper_bead: Option<f32>,
    /// 눌림 없는 공칭 절대값.
    pub computed: LevelValues,
    /// 같은 크기의 스택을 잰 최신 표본의 그 단 값(절대). 크기가 다르면 `None`.
    pub measured: Option<LevelValues>,
    /// measured − abs (둘 다 있을 때).
    pub deviation: Option<LevelValues>,
    /// 이 단을 집을 때 **실제로 쓸** 그립 Z(셀 바닥 기준). 높이를 모르면 `None`.
    pub pick_z: Option<f32>,
    /// 그 Z 가 쓴 기준 — `pick_bead`(측정된 비드 − PickBeadOffset) | `mid`(측정 없음 → Height/2).
    pub pick_z_ref: &'static str,
    /// `measured` | `manual` | `interpolated` | `computed`.
    pub source: &'static str,
    /// 이 점을 채운 표본.
    pub sample: Option<SampleRef>,
    /// 이 크기의 스택을 **통째로 잰 프로파일**의 값인가(아니면 다른 크기에서 환산·보간한 값 — 화면은 흐리게).
    pub from_profile: bool,
}

fn diff(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    Some(a? - b?)
}

/// 미리보기 스택 `preview`(없으면 `reference_stack`) 기준의 단별 보기.
///
/// 그 크기를 **통째로 잰 프로파일**이 있으면 절대 비드·적재 높이를 그 값 그대로 보인다(`from_profile`).
/// 없으면 파생 곡선으로 지은 값이라 화면은 흐리게 그린다.
/// `measured` 는 최신 SKU 표본의 절대값, `measured_count` 는 그 표본의 단수(미리보기와 같을 때만 대조한다).
pub fn level_views(item: &StockItem, spec: &ItemSpec, measured: Option<&[LevelValues]>, preview: Option<u32>, measured_count: u32) -> Vec<LevelView> {
    let n = preview.filter(|v| *v > 0).unwrap_or_else(|| spec.reference_stack().max(view_count(spec))).min(LEVEL_MAX as u32);
    let h = eff_height(item);
    let curve = curve_of(item, Some(spec));
    let prof = spec.profile(n);
    let mut bottoms = Vec::with_capacity(n as usize + 1);
    let mut acc = 0.0f32;
    for k in 1..=n {
        bottoms.push(round1(acc));
        acc += curve_at_in(item, Some(spec), &curve, n - k).pressed_height;
    }
    (1..=n)
        .map(|k| {
            let above = n - k;
            let v = curve_at_in(item, Some(spec), &curve, above);
            let bottom = bottoms[k as usize - 1];
            let stored = prof.and_then(|p| p.at_level(k));
            let abs = |x: Option<f32>| x.map(|y| round1(bottom + y));
            // 잰 크기면 절대값을 그대로, 아니면 파생 곡선을 쌓아 만든 값.
            let abs_lower = stored.and_then(|r| r.lower_bead).or_else(|| abs(v.lower_bead));
            let abs_upper = stored.and_then(|r| r.upper_bead).or_else(|| abs(v.upper_bead));
            let abs_stack = stored.and_then(|r| r.stack_height).unwrap_or_else(|| round1(bottom + v.pressed_height));
            let m = (measured_count == n).then(|| measured.and_then(|rows| rows.get(k as usize - 1)).filter(|m| **m != LevelValues::default()).cloned()).flatten();
            let deviation =
                m.as_ref().map(|m| LevelValues { lower_bead: diff(m.lower_bead, abs_lower), upper_bead: diff(m.upper_bead, abs_upper), stack_height: diff(m.stack_height, Some(abs_stack)) });
            let z = pick_z_at(item, spec, n, k);
            LevelView {
                level: k,
                above,
                configured: curve.iter().find(|r| r.above as u32 == above).cloned(),
                compression_at: (h > 0.0).then(|| round1((h - v.pressed_height).max(0.0))),
                bottom,
                abs_lower_bead: abs_lower,
                abs_upper_bead: abs_upper,
                computed: computed_level(item, k),
                measured: m,
                deviation,
                pick_z: (h > 0.0 || z.grip_ref == "pick_bead").then_some(z.z),
                pick_z_ref: z.grip_ref,
                source: if stored.is_some_and(ProfileRow::is_manual) {
                    SOURCE_MANUAL
                } else if stored.is_some() {
                    SOURCE_MEASURED
                } else {
                    v.source
                },
                sample: stored.map(|_| prof.and_then(BeadProfile::sample)).unwrap_or_else(|| v.sample.clone()),
                from_profile: stored.is_some(),
                effective: v,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gr_proto::TaskType;

    fn item() -> StockItem {
        StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, lower_bid_height: 20.0, upper_bid_height: 220.0, height: 240.0, deflection_factor: 0.3 }
    }
    /// 시험용 — 곡선 하나를 뽑아 그 하중의 값을 본다.
    fn at(it: &StockItem, s: &ItemSpec, above: u32) -> CurveValue {
        curve_at_in(it, Some(s), &curve_of(it, Some(s)), above)
    }
    /// 위 `above` 개를 인 타이어의 실제 모양(비선형: 아래로 갈수록 더 눌린다).
    fn squash(above: u32) -> (f32, f32) {
        let c = [0.0f32, 4.0, 9.0, 15.0, 22.0, 30.0, 39.0, 49.0][above.min(7) as usize];
        (240.0 - c, 220.0 - c * 0.8)
    }
    /// `n` 단 스택을 실제로 재면 나올 절대 비드값(1 단부터).
    fn sample_of(n: u32) -> (Vec<LevelValues>, f32, f32) {
        let mut rows = Vec::new();
        let mut bottom = 0.0f32;
        let mut total = 0.0f32;
        for k in 1..=n {
            let (pressed, upper) = squash(n - k);
            rows.push(LevelValues { lower_bead: Some(bottom + 20.0), upper_bead: Some(bottom + upper), stack_height: None });
            bottom += pressed;
            total += pressed;
        }
        let each = total / n as f32;
        if let Some(last) = rows.last_mut() {
            last.stack_height = Some(total);
        }
        (rows, each, total)
    }
    /// 잰 절대 상부 비드(셀 바닥 기준) — `n` 단 스택의 `k` 단.
    fn abs_upper(n: u32, k: u32) -> f32 {
        let below: f32 = (1..k).map(|j| squash(n - j).0).sum();
        round1(below + squash(n - k).1)
    }
    /// `counts` 크기의 스택을 차례로 잰 규격.
    fn measured(base: ItemSpec, counts: &[u32]) -> ItemSpec {
        let it = item();
        let mut s = base;
        for (i, n) in counts.iter().enumerate() {
            let (m, each, total) = sample_of(*n);
            let sample = SampleRef { plc: "GR2".into(), seq: *n };
            put_profile(&mut s, &MeasuredStack { count: *n, levels: &m, each_height: Some(each), total_height: Some(total), sample: Some(&sample), at: &format!("t{i}") }, true);
        }
        let _ = &it;
        s
    }

    #[test]
    fn validation_rules() {
        let ok = measured(ItemSpec { stack_max: 8, ..Default::default() }, &[5]);
        assert!(validate_spec(&ok).is_ok());
        assert!(validate_spec(&ItemSpec::default()).is_ok());
        let with = |f: &dyn Fn(&mut ItemSpec)| {
            let mut s = ok.clone();
            f(&mut s);
            validate_spec(&s)
        };
        assert!(with(&|s| s.stack_max = 21).unwrap_err().contains("stack_max"));
        assert!(with(&|s| s.weight_kg = Some(-1.0)).unwrap_err().contains("weight_kg"));
        assert!(with(&|s| s.profiles.push(s.profiles[0].clone())).unwrap_err().contains("duplicated"));
        assert!(with(&|s| s.profiles[0].count = 21).unwrap_err().contains("1..=20"));
        assert!(with(&|s| s.profiles[0].rows[0].level = 9).unwrap_err().contains("level must be 1..=5"));
        assert!(with(&|s| s.profiles[0].rows[0].lower_bead = Some(f32::NAN)).unwrap_err().contains("lower_bead"));
        assert!(with(&|s| s.profiles[0].rows[0].upper_bead = Some(1.0)).unwrap_err().contains("must be > lower_bead"));
        assert!(with(&|s| s.profiles[0].rows[0].source = "어디선가".into()).unwrap_err().contains("source"));
        // 단이 올라가는데 절대 비드가 내려가면 경고만 (품목을 몰라도 본다)
        let mut down = ok.clone();
        down.profiles[0].rows[1].upper_bead = Some(1.0);
        down.profiles[0].rows[1].lower_bead = Some(0.5);
        assert!(validate_spec(&down).is_ok());
        let w = spec_warnings(&down, None);
        assert!(w.iter().any(|x| x.contains("UpperBead")), "{w:?}");
        assert!(w.iter().any(|x| x.contains("LowerBead")), "{w:?}");
    }

    /// **핵심** — 같은 단 번호라도 스택 크기가 다르면 잰 절대값이 다르고, 그 값을 환산 없이 그대로 쓴다.
    #[test]
    fn the_same_level_in_different_stacks_uses_its_own_profile() {
        let it = item();
        let s = measured(ItemSpec { stack_max: 8, ..Default::default() }, &[5, 8]);
        assert_eq!(s.measured_counts(), vec![5, 8]);
        // 5 단의 3 단과 8 단의 3 단 — 잰 절대 비드가 다르고 Z 도 다르다
        let z5 = pick_z_at(&it, &s, 5, 3);
        let z8 = pick_z_at(&it, &s, 8, 3);
        assert_eq!((z5.z_source, z8.z_source), ("profile", "profile"));
        assert_eq!(z5.z, abs_upper(5, 3) - 30.0);
        assert_eq!(z8.z, abs_upper(8, 3) - 30.0);
        assert_ne!(z5.z, z8.z, "단 번호만 같으면 Z 가 같아서는 안 된다");
        // 절대값이라 아래 타이어를 따로 합치지 않는다
        assert_eq!((z8.base, z8.grip), (0.0, abs_upper(8, 3) - 30.0));
        assert_eq!((z8.level, z8.above, z8.below), (3, 5, 2));
        assert!(z8.used.iter().any(|u| u == &format!("grip=pick_bead abs(n=8,L3) {} (GR2 #8)", abs_upper(8, 3))), "{:?}", z8.used);
        // floor 는 그대로 더해진다
        let on_floor = stack_z_with(TaskType::Pick, 1500.0, &it, Some(&s), "pick_bead", 8, 6);
        assert_eq!(on_floor.z, 1500.0 + abs_upper(8, 3) - 30.0);
        // DROP 은 놓고 나면 한 단 높아진다 — 5 단 프로파일의 5 단을 쓴다
        let drop = stack_z_with(TaskType::Drop, 0.0, &it, Some(&s), "pick_bead", 4, 1);
        assert_eq!((drop.level, drop.z_source, drop.z), (5, "profile", abs_upper(5, 5) - 30.0));
        // 하중 곡선은 프로파일에서 파생된다 — 같은 하중이면 두 표본이 같은 값을 준다
        for a in 0..5 {
            assert_eq!(at(&it, &s, a).upper_bead, Some(round1(squash(a).1)), "above {a}");
            assert_eq!(at(&it, &s, a).pressed_height, round1(squash(a).0), "above {a}");
        }
        assert_eq!(s.measured_aboves(), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    /// 잰 적 없는 크기 → 파생 곡선(환산·보간), 잰 게 하나도 없으면 → mid.
    #[test]
    fn an_unmeasured_size_falls_back_to_the_curve_then_to_mid() {
        let it = item();
        let s = measured(ItemSpec { stack_max: 8, ..Default::default() }, &[5, 8]);
        // 6 단은 잰 적이 없다 — 곡선으로 환산한다
        let z = pick_z_at(&it, &s, 6, 2);
        assert_eq!(z.z_source, "curve");
        let base: f32 = round1(squash(5).0);
        assert_eq!(z.base, base, "아래 1 개는 자기 하중(위 5 개)의 눌린 높이");
        assert_eq!(z.z, base + round1(squash(4).1) - 30.0);
        assert!(z.used.iter().any(|u| u.contains("converted")), "{:?}", z.used);
        assert_eq!(z.grip_ref, "pick_bead");
        // 한 번도 안 잰 품목은 mid
        let never = ItemSpec { stack_max: 5, compression: Some(6.0), ..Default::default() };
        let r = stack_z_with(TaskType::Pick, 0.0, &it, Some(&never), "pick_bead", 5, 1);
        assert_eq!((r.z_source, r.grip_ref, r.grip), ("computed", "mid", 120.0));
        assert!(r.used.iter().any(|u| u.contains("grip=mid (측정 없음")), "{:?}", r.used);
        assert!(r.warnings.iter().any(|w| w.contains("측정된 비드가 없어")), "{:?}", r.warnings);
        // 그래도 **아래** 타이어 높이는 스칼라 눌림 모형 그대로다
        assert_eq!(r.base, round1((1..=4).map(|j| crate::stock::pressed_height(240.0, 6.0, 5 - j)).sum::<f32>()));
    }

    /// 같은 크기를 다시 재면 프로파일이 통째로 바뀌지만, 손으로 고친 행은 살아남는다.
    #[test]
    fn manual_rows_survive_a_re_measurement_of_that_size() {
        let it = item();
        let mut s = measured(ItemSpec { stack_max: 5, ..Default::default() }, &[5]);
        // 3 단을 손으로 고친다
        let prev = s.clone();
        s.profile_mut(5).unwrap().rows[2].upper_bead = Some(500.0);
        mark_manual_edits(&mut s, &prev);
        assert!(s.profile(5).unwrap().at_level(3).unwrap().is_manual());
        assert!(s.profile(5).unwrap().at_level(2).unwrap().source == SOURCE_MEASURED);
        // 다시 재도 3 단은 그대로
        let (m, each, total) = sample_of(5);
        let again = SampleRef { plc: "GR2".into(), seq: 99 };
        let out = put_profile(&mut s, &MeasuredStack { count: 5, levels: &m, each_height: Some(each), total_height: Some(total), sample: Some(&again), at: "t2" }, true);
        assert_eq!(out.skipped, vec![3]);
        assert_eq!(s.profile(5).unwrap().at_level(3).unwrap().upper_bead, Some(500.0));
        assert_eq!(s.profile(5).unwrap().at_level(1).unwrap().upper_bead, Some(abs_upper(5, 1)));
        // 그 단의 Z 는 손으로 넣은 값을 쓴다
        let z = pick_z_at(&it, &s, 5, 3);
        assert_eq!((z.z, z.bead_source), (500.0 - 30.0, SOURCE_MANUAL));
        // 파생 곡선에도 manual 로 올라간다
        assert_eq!(at(&it, &s, 2).source, SOURCE_MANUAL);
        // 손 표시를 지우고 다시 넣으면 덮인다
        s.profile_mut(5).unwrap().rows[2].source = SOURCE_MEASURED.into();
        let out = put_profile(&mut s, &MeasuredStack { count: 5, levels: &m, each_height: Some(each), total_height: Some(total), sample: None, at: "t3" }, true);
        assert!(out.skipped.is_empty());
        assert_eq!(s.profile(5).unwrap().at_level(3).unwrap().upper_bead, Some(abs_upper(5, 3)));
    }

    #[test]
    fn exact_interpolated_and_fallback_branches() {
        let it = item();
        // 하중 0 · 2 · 4 만 덮는 곡선을 만들려고 1 · 3 · 5 단 스택을 잰다
        let s = measured(ItemSpec { stack_max: 6, compression: Some(5.0), ..Default::default() }, &[1, 3, 5]);
        assert_eq!(s.measured_aboves(), vec![0, 1, 2, 3, 4]);
        let exact = at(&it, &s, 2);
        assert_eq!((exact.source, exact.upper_bead), (SOURCE_MEASURED, Some(round1(squash(2).1))));
        // 5 는 잰 점 위쪽이라 보간이 안 된다 → 스칼라 모형(220 − 5·5)
        let out = at(&it, &s, 5);
        assert_eq!((out.source, out.upper_bead, out.pressed_height), (SOURCE_COMPUTED, Some(195.0), 215.0));
        // 곡선이 비면(또는 규격 자체가 없으면) 전부 스칼라 — 끄는 스위치는 없다
        let empty = ItemSpec { profiles: vec![], ..s.clone() };
        assert_eq!(at(&it, &empty, 2).source, SOURCE_COMPUTED);
        assert_eq!(at(&it, &empty, 2).upper_bead, Some(210.0));
        assert_eq!(curve_at_in(&it, None, &[], 2).source, SOURCE_COMPUTED);
        // 곡선에 구멍이 있으면 두 점 사이를 선형 보간한다
        let hole = [
            AboveRow { above: 0, upper_bead: Some(210.0), source: SOURCE_MEASURED.into(), ..Default::default() },
            AboveRow { above: 4, upper_bead: Some(180.0), source: SOURCE_MEASURED.into(), ..Default::default() },
        ];
        let mid = curve_at_in(&it, Some(&s), &hole, 2);
        assert_eq!((mid.source, mid.upper_bead), ("interpolated", Some(195.0)));
        assert_eq!(curve_at_in(&it, Some(&s), &hole, 5).source, SOURCE_COMPUTED, "점 바깥은 보간하지 않는다");
    }

    #[test]
    fn stack_z_without_a_profile_is_the_scalar_model() {
        let it = item();
        let empty = ItemSpec::default();
        for (tt, n, c) in [(TaskType::Pick, 3, 1), (TaskType::Drop, 2, 1), (TaskType::Pick, 0, 1)] {
            let grip = crate::stock::grip_offset("mid", &it);
            let plain = 1000.0 + it.height * crate::stock::below_count(tt, n, c) as f32 + grip;
            assert_eq!(stack_z_with(tt, 1000.0, &it, None, "mid", n, c).z, plain);
            let r = stack_z_with(tt, 1000.0, &it, Some(&empty), "mid", n, c);
            assert_eq!((r.z, r.z_source), (plain, "computed"));
        }
        // MOVE 같은 종류는 셀 바닥 그대로
        let r = stack_z_with(TaskType::Move, 1000.0, &it, Some(&empty), "pick_bead", 3, 1);
        assert_eq!((r.z, r.z_source), (1000.0, "computed"));
    }

    /// 그립 기준은 mid | pick_bead 뿐이고, 잰 비드가 없으면 mid 로 내려간다 (결정 2026-09-18).
    #[test]
    fn grip_reference_rules() {
        let it = item(); // H 240 → mid 120, UpperBid 220
        let s = measured(ItemSpec { stack_max: 5, ..Default::default() }, &[5]);
        assert_eq!(stack_z_with(TaskType::Pick, 0.0, &it, Some(&s), "mid", 5, 1).grip, 120.0);
        // PickBeadOffset 을 바꾸면 그대로 따라온다
        let off = ItemSpec { pick_bead_offset: Some(45.0), ..s.clone() };
        assert_eq!(pick_z_at(&it, &off, 5, 5).z, abs_upper(5, 5) - 45.0);
        // 비드가 너무 낮으면 0 으로 제한하고 경고 — 맨 아래 단의 잰 비드는 202.4 뿐이다
        assert!((pick_z_at(&it, &ItemSpec { pick_bead_offset: Some(200.0), ..s.clone() }, 5, 1).z - (abs_upper(5, 1) - 200.0)).abs() < 0.05);
        let hard = ItemSpec { pick_bead_offset: Some(210.0), ..s.clone() };
        let r = pick_z_at(&it, &hard, 5, 1);
        assert_eq!(r.z, 0.0);
        assert!(r.warnings.iter().any(|w| w.contains("PickBeadOffset")), "{:?}", r.warnings);
        // 비드 높이가 없는 품목은 mid
        let no_bead = StockItem { upper_bid_height: 0.0, ..item() };
        let r = stack_z_with(TaskType::Pick, 0.0, &no_bead, Some(&ItemSpec { stack_max: 5, ..Default::default() }), "pick_bead", 1, 1);
        assert_eq!((r.grip_ref, r.grip, r.upper_bead), ("mid", 120.0, None));
        // 옛 `bead`(비드를 그대로 잡기)는 없어졌다 — 읽을 때 `pick_bead` 로 옮겨진다
        assert_eq!(normalize_grip_ref("bead"), "pick_bead");
        assert_eq!(normalize_grip_ref("BEAD"), "pick_bead");
        assert_eq!(normalize_grip_ref("bead+offset"), "pick_bead");
        assert_eq!(normalize_grip_ref("PICK-BEAD"), "pick_bead");
        assert_eq!(normalize_grip_ref("무엇"), "mid");
        assert_eq!(stack_z_with(TaskType::Pick, 0.0, &it, Some(&s), "bead", 5, 5).z, pick_z_at(&it, &s, 5, 1).z);
    }

    #[test]
    fn compression_clamps_and_warns() {
        let it = item();
        let hard = ItemSpec { stack_max: 5, compression: Some(100.0), ..Default::default() };
        let top = stack_z_with(TaskType::Pick, 0.0, &it, Some(&hard), "mid", 5, 1);
        assert_eq!(top.base, MIN_PITCH + MIN_PITCH + 40.0 + 140.0);
        assert!(top.warnings.iter().any(|w| w.contains("Compression")), "{:?}", top.warnings);
        assert!(validate_spec_for(&ItemSpec { compression: Some(240.0), ..Default::default() }, &it).unwrap_err().contains("compression"));
        assert!(validate_spec_for(&ItemSpec { pick_bead_offset: Some(300.0), ..Default::default() }, &it).unwrap_err().contains("pick_bead_offset"));
        let s = ItemSpec { pick_bead_offset: Some(150.0), ..Default::default() };
        assert!(validate_spec_for(&s, &it).is_ok());
        assert!(spec_warnings(&s, Some(&it)).iter().any(|w| w.contains("PickBeadOffset")));
        assert!(spec_warnings(&s, None).is_empty(), "품목을 모르면 높이 경고는 없다");
        assert!(validate_spec(&ItemSpec { compression: Some(-1.0), ..Default::default() }).unwrap_err().contains("compression"));
        // 말이 안 되는 절대값은 거부한다
        let mut silly = measured(ItemSpec { stack_max: 5, ..Default::default() }, &[5]);
        silly.profile_mut(5).unwrap().rows[0].upper_bead = Some(5000.0);
        assert!(validate_spec_for(&silly, &it).unwrap_err().contains("must be <= Height"));
    }

    #[test]
    fn total_height_mismatch_is_flagged() {
        let it = item();
        let (m, each, total) = sample_of(5);
        // PLC 가 준 두 값이 서로 안 맞으면(EachHeight × TotalCount ≠ TotalHeight) 측정을 의심한다
        let bad = stack_profile(&it, &m, 5, Some(each + 20.0), Some(total));
        assert!(bad.warnings.iter().any(|w| w.contains("TotalHeight")), "{:?}", bad.warnings);
        let good = stack_profile(&it, &m, 5, Some(each), Some(total));
        assert!(good.warnings.is_empty(), "{:?}", good.warnings);
        // 맨 윗단 높이는 TotalHeight 에서 아래 단 합을 빼서 되짚는다 — 눌리지 않은 240 이 나와야 한다
        assert_eq!(good.points[4].pressed_height, Some(240.0));
        assert_eq!(good.pitch_sum, Some(round1(total)));
        // 규격에 남은 값으로도 같은 검사를 한다
        let mut s = measured(ItemSpec { stack_max: 5, ..Default::default() }, &[5]);
        assert!(!spec_warnings(&s, Some(&it)).iter().any(|w| w.contains("TotalHeight")), "{:?}", spec_warnings(&s, Some(&it)));
        s.profile_mut(5).unwrap().each_height = Some(each + 20.0);
        assert!(spec_warnings(&s, Some(&it)).iter().any(|w| w.contains("EachHeight")));
        s.profile_mut(5).unwrap().each_height = Some(each);
        s.profile_mut(5).unwrap().total_height = Some(total + 30.0);
        assert!(spec_warnings(&s, Some(&it)).iter().any(|w| w.contains("맨 윗단 StackHeight")));
    }

    #[test]
    fn level_views_follow_the_preview_stack_size() {
        let it = item();
        let s = measured(ItemSpec { stack_max: 8, ..Default::default() }, &[8]);
        let (m8, ..) = sample_of(8);
        let v = level_views(&it, &s, Some(&m8), Some(8), 8);
        assert_eq!(v.len(), 8);
        assert_eq!((v[0].level, v[0].above), (1, 7));
        assert_eq!((v[7].level, v[7].above), (8, 0));
        assert_eq!(v[0].bottom, 0.0);
        // 잰 크기라 절대 비드는 측정 그대로고 편차는 0
        for (i, row) in v.iter().enumerate() {
            assert!(row.from_profile, "level {}", row.level);
            assert_eq!(row.abs_upper_bead, Some(round1(m8[i].upper_bead.unwrap())));
            assert_eq!(row.source, SOURCE_MEASURED);
            assert_eq!(row.sample.as_ref().map(|s| s.seq), Some(8));
            assert_eq!(row.deviation.as_ref().unwrap().upper_bead, Some(0.0));
            assert_eq!(row.pick_z, Some(abs_upper(8, row.level) - 30.0));
            assert_eq!(row.pick_z_ref, "pick_bead");
        }
        // 미리보기를 5 로 줄이면 잰 적 없는 크기라 곡선에서 지어낸다(흐리게)
        let v5 = level_views(&it, &s, Some(&m8), Some(5), 8);
        assert_eq!(v5.len(), 5);
        assert!(v5.iter().all(|r| !r.from_profile));
        assert_ne!(v5[2].abs_upper_bead, v[2].abs_upper_bead);
        assert!(v5[2].measured.is_none(), "스택 크기가 다르면 대조하지 않는다");
        // pick_z 는 compose 경로와 같아야 한다
        assert_eq!(pick_z_at(&it, &s, 8, 3).z, v[2].pick_z.unwrap());
        assert_eq!(v[2].pick_z.unwrap(), stack_z_with(TaskType::Pick, 0.0, &it, Some(&s), "pick_bead", 8, 6).z);
        // 잰 적 없는 품목은 pick_z 도 mid(Height/2) 로 내려간다 — 표가 실제로 쓸 Z 를 보인다
        let no_bead = StockItem { upper_bid_height: 0.0, ..item() };
        let flat = level_views(&no_bead, &ItemSpec { stack_max: 3, ..Default::default() }, None, None, 0);
        assert_eq!((flat[0].pick_z, flat[0].pick_z_ref), (Some(120.0), "mid"));
    }

    #[test]
    fn scalar_compression_is_recovered_as_a_fallback() {
        let it = item(); // H 240, UpperBid 220
        let m = |lo: f32, up: f32| LevelValues { lower_bead: Some(lo), upper_bead: Some(up), stack_height: None };
        let rows = vec![m(28.0, 208.0), m(248.0, 442.0), LevelValues { stack_height: Some(702.0), ..m(482.0, 682.0) }];
        let s = suggest_compression(&it, &rows, 3, Some(234.0));
        assert_eq!((s.value, s.method), (Some(6.0), Some("beads")));
        assert_eq!((s.from_beads, s.from_each_height, s.samples, s.measured_count), (Some(6.0), Some(6.0), 3, 3));
        let s = suggest_compression(&it, &[], 3, Some(234.0));
        assert_eq!((s.value, s.method, s.samples), (Some(6.0), Some("each_height"), 0));
        assert_eq!(suggest_compression(&it, &[], 0, None).value, None);
        assert_eq!(suggest_compression(&it, &[m(20.0, 220.0), m(260.0, 460.0)], 2, Some(240.0)).value, Some(0.0));
        // StackMax 를 넘는 하중은 파생 곡선에서 빠진다(프로파일 자체는 남는다)
        let mut spec = ItemSpec { stack_max: 2, ..Default::default() };
        let out = put_profile(&mut spec, &MeasuredStack { count: 3, levels: &rows, each_height: Some(234.0), total_height: Some(702.0), sample: None, at: "t" }, true);
        assert_eq!(out.levels, vec![1, 2, 3]);
        assert_eq!(spec.measured_aboves(), vec![0, 1]);
        assert!(validate_spec_for(&spec, &it).is_ok());
    }

    #[test]
    fn computed_levels_ignore_deflection() {
        let it = item();
        assert_eq!(computed_level(&it, 1), LevelValues { lower_bead: Some(20.0), upper_bead: Some(220.0), stack_height: Some(240.0) });
        assert_eq!(computed_level(&it, 3), LevelValues { lower_bead: Some(500.0), upper_bead: Some(700.0), stack_height: Some(720.0) });
        assert_eq!(view_count(&ItemSpec::default()), 1);
        assert_eq!(view_count(&ItemSpec { stack_max: 4, ..Default::default() }), 4);
        assert_eq!(view_count(&ItemSpec { stack_max: 15, ..Default::default() }), 15);
        assert_eq!(view_count(&measured(ItemSpec::default(), &[12])), 12);
        let rows = measured_from_sku(&[2.0, 20.0, 219.0, 257.0, 456.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 237.0, 474.0]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].stack_height, Some(474.0));
        assert_eq!(rows[0].stack_height, None);
    }
}
