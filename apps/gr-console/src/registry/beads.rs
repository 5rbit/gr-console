//! SKU 측정(MEASLOG `Kind = 2`) → 화물 규격의 단별 비드 표.
//!
//! GR PLC 가 `measureSKU` 를 끝내면 MEASLOG 항목 하나에 단별 하/상 비드(셀 바닥 기준 mm)와 스택 단수 ·
//! 한 개 높이 · 전체 높이 · 진단 플래그가 실려 온다(`LGR_Const_Measure` 의 `MEAS_SKU_*` 인덱스, 프런트
//! `lib/meas/const.ts` 의 `DATA_LABEL[2]` 와 같은 배치):
//!
//! | idx | 뜻 | idx | 뜻 |
//! |---|---|---|---|
//! | 0 | Status | 15 | DiagFlags |
//! | 1..14 | Stack[1..7].Lower/UpperBead | 16 | LayerOffsetMax |
//! | | | 17 | StackCount(= TotalCount) |
//! | | | 18 | EachHeight |
//! | | | 19 | StackHeight(TotalHeight) |
//!
//! 여기서 하는 일은 셋이다.
//! 1. **표본으로 적재** — `item_bead_samples` 에 품목 코드별로 최근 [`HISTORY_LIMIT`] 개만 남긴다.
//! 2. **검증** — 오류 상태 · 진단 플래그 · 단수 0 · 비단조 · 말도 안 되는 값은 이유와 함께 거부로 적는다
//!    (화면이 "왜 안 바뀌었는지" 를 그대로 보여 준다).
//! 3. **규격 반영** — 품목 스위치 `auto_apply_measured`(기본 켬)가 켜져 있으면 그 **스택 크기의 프로파일**
//!    (`spec::BeadProfile`)을 통째로 새로 쓴다. 값은 PLC 가 준 **셀 바닥 기준 절대값 그대로**라 같은 크기의
//!    스택을 집을 때 환산 없이 바로 쓰인다(`spec::put_profile`). 하중(`Above`) 곡선은 여기서 파생된다.
//!    회귀로 되짚은 스칼라 `Compression` 은 잰 적 없는 크기를 위한 대체값으로 같이 넣는다.
//!    **운전자가 손으로 고친 행(`source = manual`)은 절대 덮지 않고** 건너뛴 단을 경고로 남긴다.

use gr_proto::StockItem;
use serde::Serialize;
use serde_json::{Value as Json, json};

use super::spec::{self, ItemSpec, LevelValues, SampleRef};
use super::{ItemEntry, Registry};
use crate::error::ApiError;
use crate::util::now_str;

/// 품목·PLC 당 남기는 표본 수.
pub const HISTORY_LIMIT: usize = 20;
/// 비드·높이 값의 상한(mm) — 이보다 크면 쓰레기 값으로 본다.
pub const SANE_MAX_MM: f32 = 5000.0;

// SKU `Data` 인덱스 (`LGR_Const_Measure` MEAS_SKU_*)
const I_STATUS: usize = 0;
const I_DIAG_FLAGS: usize = 15;
const I_LAYER_OFFSET: usize = 16;
const I_TOTAL_COUNT: usize = 17;
const I_EACH_HEIGHT: usize = 18;
const I_TOTAL_HEIGHT: usize = 19;

/// MEASLOG SKU 항목 하나에서 뽑아 낸 비드 측정 표본.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BeadSample {
    pub plc: String,
    pub seq: u32,
    pub code: u32,
    /// PLC 가 찍은 측정 시각(`TimeStamp`).
    pub at: String,
    /// MEASLOG 항목 상태(3 이상 = 오류).
    pub status: u16,
    /// `Data[0]` — MeasureSku 자체 상태.
    pub sku_status: f32,
    pub diag_flags: u32,
    pub layer_offset: f32,
    /// 잰 스택의 단수 — 이 표본의 `k` 단 위에는 `total_count − k` 개가 있었다.
    pub total_count: u32,
    pub each_height: f32,
    pub total_height: f32,
    /// 1 단부터의 단별 하/상 비드(셀 바닥 기준).
    pub levels: Vec<LevelValues>,
}

impl BeadSample {
    /// 값이 하나라도 있는 단 수.
    pub fn filled(&self) -> usize {
        self.levels.iter().filter(|m| **m != LevelValues::default()).count()
    }
    /// 화면·메모에 쓰는 도장.
    pub fn stamp(&self) -> String {
        format!("SKU 측정 {} seq {} ({})", self.plc, self.seq, self.at)
    }
    pub fn sample_ref(&self) -> SampleRef {
        SampleRef { plc: self.plc.clone(), seq: self.seq }
    }
}

fn f32_at(data: &[f32], i: usize) -> f32 {
    data.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0)
}

/// MEASLOG 항목 JSON(`Kind = 2`)에서 표본을 만든다. 다른 종류거나 코드가 0 이면 `None`.
pub fn sample_from_entry(plc: &str, entry: &Json) -> Option<BeadSample> {
    if entry["Kind"].as_u64().unwrap_or(0) != gr_proto::MEAS_LOG_KIND_SKU as u64 {
        return None;
    }
    let code = entry["Cmd"]["Item"]["Code"].as_u64().unwrap_or(0) as u32;
    let seq = entry["Seq"].as_u64().unwrap_or(0) as u32;
    if code == 0 || seq == 0 {
        return None;
    }
    let data: Vec<f32> = entry["Data"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    let total_count = f32_at(&data, I_TOTAL_COUNT).max(0.0).round() as u32;
    Some(BeadSample {
        plc: plc.to_string(),
        seq,
        code,
        at: entry["TimeStamp"].as_str().unwrap_or("").to_string(),
        status: entry["Status"].as_u64().unwrap_or(0) as u16,
        sku_status: f32_at(&data, I_STATUS),
        diag_flags: f32_at(&data, I_DIAG_FLAGS).max(0.0).round() as u32,
        layer_offset: f32_at(&data, I_LAYER_OFFSET),
        total_count,
        each_height: f32_at(&data, I_EACH_HEIGHT),
        total_height: f32_at(&data, I_TOTAL_HEIGHT),
        levels: spec::measured_from_sku(&data),
    })
}

fn sane(v: f32) -> bool {
    v.is_finite() && v > 0.0 && v <= SANE_MAX_MM
}

/// 규격에 넣어도 되는 표본인가. `Err` 의 글이 화면에 그대로 "거부 사유" 로 보인다.
pub fn validate_sample(s: &BeadSample) -> Result<(), String> {
    if s.status >= 3 {
        return Err(format!("측정 Status {} (오류)", s.status));
    }
    if s.sku_status >= 3.0 {
        return Err(format!("MeasureSku Status {} (오류)", s.sku_status));
    }
    if s.diag_flags != 0 {
        return Err(format!("DiagFlags 0x{:X} — 스택 정렬 진단 경고", s.diag_flags));
    }
    if s.total_count == 0 {
        return Err("TotalCount 0 — 잰 스택이 없습니다".into());
    }
    if s.total_count as usize > spec::LEVEL_MAX as usize {
        return Err(format!("TotalCount {} 가 최대 단수 {} 를 넘습니다", s.total_count, spec::LEVEL_MAX));
    }
    if s.filled() == 0 {
        return Err("단별 비드 값이 없습니다".into());
    }
    if s.each_height != 0.0 && !sane(s.each_height) {
        return Err(format!("EachHeight {} 가 0..={SANE_MAX_MM} mm 밖입니다", s.each_height));
    }
    if s.total_height != 0.0 && !sane(s.total_height) {
        return Err(format!("TotalHeight {} 가 0..={SANE_MAX_MM} mm 밖입니다", s.total_height));
    }
    let (mut prev_lo, mut prev_up) = (None::<(u32, f32)>, None::<(u32, f32)>);
    for (i, m) in s.levels.iter().enumerate() {
        let k = i as u32 + 1;
        if *m == LevelValues::default() {
            continue;
        }
        for (label, v) in [("LowerBead", m.lower_bead), ("UpperBead", m.upper_bead)] {
            if let Some(v) = v
                && !sane(v)
            {
                return Err(format!("Level {k} {label} {v} 가 0..={SANE_MAX_MM} mm 밖입니다"));
            }
        }
        if let (Some(lo), Some(up)) = (m.lower_bead, m.upper_bead)
            && up <= lo
        {
            return Err(format!("Level {k}: UpperBead {up} ≤ LowerBead {lo}"));
        }
        if let (Some(v), Some((pk, pv))) = (m.lower_bead, prev_lo)
            && v <= pv
        {
            return Err(format!("LowerBead 가 올라가지 않습니다 — Level {k} {v} ≤ Level {pk} {pv}"));
        }
        if let (Some(v), Some((pk, pv))) = (m.upper_bead, prev_up)
            && v <= pv
        {
            return Err(format!("UpperBead 가 올라가지 않습니다 — Level {k} {v} ≤ Level {pk} {pv}"));
        }
        prev_lo = m.lower_bead.map(|v| (k, v)).or(prev_lo);
        prev_up = m.upper_bead.map(|v| (k, v)).or(prev_up);
    }
    Ok(())
}

/// 표본을 규격에 실제로 넣은 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Applied {
    /// 값을 채운 단 번호.
    pub levels: Vec<u32>,
    /// `manual` 이라 건너뛴 단 번호.
    pub skipped: Vec<u32>,
    /// 새로 넣은 스칼라 눌림양(mm/개, 평균·대체값). 못 되짚었거나 이상하면 `None`.
    pub compression: Option<f32>,
    /// `beads` | `each_height`
    pub compression_method: Option<&'static str>,
    /// 이 스택의 단별 눌림 프로파일(주 데이터).
    pub profile: spec::StackProfile,
    pub warnings: Vec<String>,
}

impl Applied {
    pub fn changed(&self) -> bool {
        !self.levels.is_empty() || self.compression.is_some()
    }
}

/// 표본을 규격에 넣는다(저장은 부르는 쪽). 검증은 미리 [`validate_sample`] 로 끝낸 뒤 부른다.
///
/// 한 번의 measureSKU 는 스택 **하나를 통째로** 잰 것이라, `n` 단 표본 하나가 그 크기의 프로파일 전체를
/// 이룬다. 스칼라 `compression` 은 잰 적 없는 크기를 위한 대체값으로만 같이 넣는다.
pub fn apply_sample(s: &mut ItemSpec, item: &StockItem, sample: &BeadSample) -> Applied {
    let mut out = Applied::default();
    let src = sample.sample_ref();
    let each = Some(sample.each_height).filter(|v| sane(*v));
    let total = Some(sample.total_height).filter(|v| sane(*v));
    out.profile = spec::stack_profile(item, &sample.levels, sample.total_count, each, total);
    out.warnings.extend(out.profile.warnings.iter().cloned());
    let stack = spec::MeasuredStack { count: sample.total_count, levels: &sample.levels, each_height: each, total_height: total, sample: Some(&src), at: &now_str() };
    let fill = spec::put_profile(s, &stack, true);
    out.levels = fill.levels;
    out.skipped = fill.skipped;
    if !out.skipped.is_empty() {
        out.warnings.push(format!("손으로 고친 {:?} 단은 그대로 두었습니다 — Source 를 measured 로 되돌리면 덮습니다", out.skipped));
    }
    if !out.levels.is_empty() {
        s.bead_source = sample.stamp();
    }
    let sug = spec::suggest_compression(item, &sample.levels, sample.total_count, each);
    let h = spec::eff_height(item);
    match sug.value {
        Some(v) if v.is_finite() && v >= 0.0 && (h <= 0.0 || v < h) => {
            s.compression = Some(v);
            s.compression_source = format!("{} — {} n={} (단별 CompressionAt 의 대체값)", sample.stamp(), sug.method.unwrap_or("?"), sug.samples);
            out.compression = Some(v);
            out.compression_method = sug.method;
        }
        Some(v) => out.warnings.push(format!("되짚은 Compression {v} 가 Height {h} 보다 커서 쓰지 않았습니다")),
        None => {}
    }
    out
}

/// 저장된 표본 한 줄(화면·API 용).
#[derive(Clone, Debug, Serialize)]
pub struct StoredSample {
    #[serde(flatten)]
    pub sample: BeadSample,
    /// 규격에 반영했는가.
    pub applied: bool,
    /// 거부 사유(반영했으면 어느 단을 넣었는지).
    pub reason: String,
    /// 콘솔이 받은 시각.
    pub recorded_at: String,
}

/// [`ingest`] 한 번의 결과.
#[derive(Clone, Debug, Serialize)]
pub struct Ingested {
    pub code: u32,
    pub plc: String,
    pub seq: u32,
    pub applied: bool,
    pub reason: String,
    pub detail: Option<Applied>,
}

impl Registry {
    /// SKU 표본 하나를 적재하고, 스위치가 켜져 있고 검증을 통과하면 규격까지 갱신한다.
    /// 이미 같은 `(code, plc, seq)` 가 있으면 아무것도 하지 않고 `None`.
    pub fn ingest_bead_sample(&self, sample: &BeadSample) -> Result<Option<Ingested>, ApiError> {
        if self.has_bead_sample(sample.code, &sample.plc, sample.seq)? {
            return Ok(None);
        }
        let entry = self.item(sample.code)?;
        let (applied, reason, detail) = self.decide(sample, entry.as_ref(), false)?;
        self.store_bead_sample(sample, applied, &reason)?;
        Ok(Some(Ingested { code: sample.code, plc: sample.plc.clone(), seq: sample.seq, applied, reason, detail }))
    }

    /// 이미 적재한 표본을 다시(운전자 지시로) 규격에 넣는다. `force` 면 자동 스위치가 꺼져 있어도 넣는다.
    pub fn apply_bead_sample(&self, code: u32, plc: Option<&str>, seq: u32, force: bool) -> Result<Ingested, ApiError> {
        let stored =
            self.bead_sample(code, plc, seq)?.ok_or_else(|| ApiError::NotFound(format!("품목 {code} 의 비드 표본 seq {seq} 가 없습니다{}", plc.map(|p| format!(" ({p})")).unwrap_or_default())))?;
        let entry = self.item(code)?;
        let (applied, reason, detail) = self.decide(&stored.sample, entry.as_ref(), force)?;
        if !applied {
            return Err(ApiError::BadRequest(format!("표본 seq {seq} 는 규격에 넣을 수 없습니다 — {reason}")));
        }
        self.mark_bead_sample(code, &stored.sample.plc, seq, applied, &reason)?;
        Ok(Ingested { code, plc: stored.sample.plc, seq, applied, reason, detail })
    }

    /// 검증 → (스위치 확인) → 규격 저장. 돌려주는 글이 표본 줄의 `reason` 이 된다.
    fn decide(&self, sample: &BeadSample, entry: Option<&ItemEntry>, force: bool) -> Result<(bool, String, Option<Applied>), ApiError> {
        let Some(e) = entry else {
            return Ok((false, format!("품목 {} 가 등록되어 있지 않습니다", sample.code), None));
        };
        if let Err(why) = validate_sample(sample) {
            return Ok((false, why, None));
        }
        if !force && !e.spec.auto_apply_measured() {
            return Ok((false, "AutoApplyMeasured 꺼짐 — 손으로 Apply 하세요".into(), None));
        }
        let mut s = e.spec.clone();
        let detail = apply_sample(&mut s, &e.item, sample);
        if !detail.changed() {
            let why = if detail.skipped.is_empty() { "바꿀 값이 없습니다".to_string() } else { format!("손으로 고친 {:?} 단뿐이라 그대로 두었습니다", detail.skipped) };
            return Ok((false, why, Some(detail)));
        }
        let s = s.normalized();
        if let Err(m) = spec::validate_spec_for(&s, &e.item) {
            return Ok((false, format!("규격 검증 실패: {m}"), Some(detail)));
        }
        self.set_item_spec(e.code, &s)?;
        let mut reason = format!("스택 {} 의 {:?} 단 반영", sample.total_count, detail.levels);
        let prof: Vec<String> = detail.profile.points.iter().map(|p| format!("{}:{}", p.above, p.pressed_height.map(|v| format!("{v}")).unwrap_or_else(|| "-".into()))).collect();
        if !prof.is_empty() {
            reason.push_str(&format!(", PressedHeight [{}]", prof.join(", ")));
        }
        if let Some(v) = detail.compression {
            reason.push_str(&format!(", 대체 Compression {v} mm/개"));
        }
        for w in &detail.warnings {
            reason.push_str(&format!(" · {w}"));
        }
        Ok((true, reason, Some(detail)))
    }

    // ---- 표본 적재 (`item_bead_samples`, migration 0010)
    fn has_bead_sample(&self, code: u32, plc: &str, seq: u32) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.query_row("SELECT COUNT(*) FROM item_bead_samples WHERE code = ?1 AND plc = ?2 AND seq = ?3", (code, plc, seq), |r| r.get::<_, i64>(0)))? > 0)
    }

    fn store_bead_sample(&self, s: &BeadSample, applied: bool, reason: &str) -> Result<(), ApiError> {
        self.db.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO item_bead_samples (code, plc, seq, at, status, diag_flags, total_count, each_height, total_height, layer_offset, sample_json, applied, reason, recorded_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                (
                    s.code,
                    &s.plc,
                    s.seq,
                    &s.at,
                    s.status,
                    s.diag_flags,
                    s.total_count,
                    s.each_height as f64,
                    s.total_height as f64,
                    s.layer_offset as f64,
                    serde_json::to_string(s).unwrap_or_else(|_| "{}".into()),
                    applied as i64,
                    reason,
                    now_str(),
                ),
            )?;
            // 품목·PLC 당 최근 HISTORY_LIMIT 개만 남긴다.
            c.execute(
                "DELETE FROM item_bead_samples WHERE code = ?1 AND plc = ?2 AND seq NOT IN (SELECT seq FROM item_bead_samples WHERE code = ?1 AND plc = ?2 ORDER BY seq DESC LIMIT ?3)",
                (s.code, &s.plc, HISTORY_LIMIT as i64),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    fn mark_bead_sample(&self, code: u32, plc: &str, seq: u32, applied: bool, reason: &str) -> Result<(), ApiError> {
        self.db.with(|c| c.execute("UPDATE item_bead_samples SET applied = ?1, reason = ?2 WHERE code = ?3 AND plc = ?4 AND seq = ?5", (applied as i64, reason, code, plc, seq)))?;
        Ok(())
    }

    /// 최신 순 표본 목록. `plc = None` 이면 모든 로봇.
    pub fn bead_samples(&self, code: u32, plc: Option<&str>, limit: usize) -> Result<Vec<StoredSample>, ApiError> {
        let rows: Vec<(String, i64, String, String)> = self.db.with(|c| {
            let mut st =
                c.prepare("SELECT sample_json, applied, reason, recorded_at FROM item_bead_samples WHERE code = ?1 AND (?2 IS NULL OR plc = ?2) ORDER BY recorded_at DESC, seq DESC LIMIT ?3")?;
            let it = st.query_map((code, plc, limit as i64), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(j, applied, reason, recorded_at)| serde_json::from_str::<BeadSample>(&j).ok().map(|sample| StoredSample { sample, applied: applied != 0, reason, recorded_at }))
            .collect())
    }

    /// 표본 하나(`seq`). `plc = None` 이면 가장 최근에 받은 것.
    pub fn bead_sample(&self, code: u32, plc: Option<&str>, seq: u32) -> Result<Option<StoredSample>, ApiError> {
        let row: Option<(String, i64, String, String)> = self.db.with(|c| {
            let mut st =
                c.prepare("SELECT sample_json, applied, reason, recorded_at FROM item_bead_samples WHERE code = ?1 AND seq = ?2 AND (?3 IS NULL OR plc = ?3) ORDER BY recorded_at DESC LIMIT 1")?;
            let mut it = st.query_map((code, seq, plc), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            it.next().transpose()
        })?;
        Ok(row.and_then(|(j, applied, reason, recorded_at)| serde_json::from_str::<BeadSample>(&j).ok().map(|sample| StoredSample { sample, applied: applied != 0, reason, recorded_at })))
    }
}

/// 표본 목록의 화면용 요약(`GET /api/items/{code}/bead-samples`).
pub fn samples_json(rows: &[StoredSample]) -> Vec<Json> {
    rows.iter()
        .map(|s| {
            json!({ "plc": s.sample.plc, "seq": s.sample.seq, "code": s.sample.code, "at": s.sample.at, "status": s.sample.status,
                "sku_status": s.sample.sku_status, "diag_flags": s.sample.diag_flags, "layer_offset": s.sample.layer_offset,
                "total_count": s.sample.total_count, "each_height": s.sample.each_height, "total_height": s.sample.total_height,
                "levels": s.sample.levels, "filled": s.sample.filled(), "applied": s.applied, "reason": s.reason, "recorded_at": s.recorded_at,
                "valid": validate_sample(&s.sample).is_ok() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn item() -> StockItem {
        StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, lower_bid_height: 20.0, upper_bid_height: 220.0, height: 240.0, deflection_factor: 0.0 }
    }

    /// `each` mm 씩 눌린 3 단 스택의 SKU 항목(데모 월드가 만드는 모양 그대로).
    fn entry(seq: u32, code: u32, count: usize, each: f32, status: u16) -> Json {
        let h = 240.0f32;
        let mut data = [0f32; 20];
        for k in 1..=count.min(7) {
            data[2 * k - 1] = (k - 1) as f32 * each + 20.0;
            data[2 * k] = (k - 1) as f32 * each + h - 20.0;
        }
        data[0] = if status >= 3 { 4.0 } else { 2.0 };
        data[17] = count as f32;
        data[18] = each;
        data[19] = count as f32 * each;
        json!({ "Seq": seq, "Kind": 2, "Status": status, "TimeStamp": "2026-09-18 10:11:12.000", "Data": data.to_vec(), "Cmd": { "Item": { "Code": code } } })
    }

    /// 규격에서 파생한 하중 곡선(정본은 프로파일).
    fn curve(spec: &ItemSpec) -> Vec<spec::AboveRow> {
        spec::curve_of(&item(), Some(spec))
    }
    fn above(spec: &ItemSpec, a: u8) -> spec::AboveRow {
        curve(spec).into_iter().find(|r| r.above == a).unwrap_or_else(|| panic!("above {a} missing"))
    }

    fn reg_with_item(spec: ItemSpec) -> std::sync::Arc<Registry> {
        let reg = Registry::new(Db::open_memory().unwrap());
        reg.upsert_item_full(1001, "225/45R17", &item(), "", Some(&spec)).unwrap();
        reg
    }

    #[test]
    fn sample_reads_every_sku_index() {
        let s = sample_from_entry("GR2", &entry(7, 1001, 3, 237.0, 2)).unwrap();
        assert_eq!((s.plc.as_str(), s.seq, s.code, s.total_count), ("GR2", 7, 1001, 3));
        assert_eq!(s.each_height, 237.0);
        assert_eq!(s.total_height, 711.0);
        assert_eq!(s.filled(), 3);
        assert_eq!(s.levels[1].lower_bead, Some(257.0));
        assert!(validate_sample(&s).is_ok());
        assert!(sample_from_entry("GR2", &json!({ "Seq": 1, "Kind": 1, "Cmd": { "Item": { "Code": 1001 } } })).is_none());
    }

    #[test]
    fn ingest_stores_the_absolute_profile_of_that_stack_size() {
        let reg = reg_with_item(ItemSpec { stack_max: 5, ..Default::default() });
        let s = sample_from_entry("GR2", &entry(11, 1001, 3, 237.0, 2)).unwrap();
        let got = reg.ingest_bead_sample(&s).unwrap().unwrap();
        assert!(got.applied, "{}", got.reason);
        let spec = reg.item(1001).unwrap().unwrap().spec;
        // 정본은 그 크기의 **절대** 프로파일 — PLC 가 준 값 그대로
        assert_eq!(spec.measured_counts(), vec![3]);
        let p = spec.profile(3).unwrap();
        assert_eq!((p.sample_plc.as_str(), p.sample_seq), ("GR2", 11));
        assert_eq!(p.abs_upper_bead(1), Some(220.0));
        assert_eq!(p.abs_upper_bead(3), Some(2.0 * 237.0 + 220.0));
        assert_eq!(p.at_level(3).unwrap().stack_height, Some(711.0));
        assert!(!p.at_level(2).unwrap().is_manual());
        assert_eq!((p.total_height, p.each_height), (Some(711.0), Some(237.0)));
        // 하중 곡선은 여기서 파생된다 — TotalCount 3 이면 Above 0·1·2 세 점
        assert_eq!(spec.measured_aboves(), vec![0, 1, 2]);
        assert_eq!(above(&spec, 2).sample_seq, 11);
        assert_eq!(above(&spec, 2).sample_plc, "GR2");
        assert!(above(&spec, 2).is_measured());
        // 데모 모양은 pitch 237, 타이어 바닥 기준 상부 비드 220 으로 일정하다
        assert_eq!(above(&spec, 0).pressed_height, Some(237.0));
        assert_eq!(above(&spec, 1).upper_bead, Some(220.0));
        assert!(spec.bead_source.contains("seq 11"));
        // 대체 스칼라 Compression 도 같이 들어간다(곡선에 점이 없는 하중용)
        let c = spec.compression.unwrap();
        assert!((0.5..=4.0).contains(&c), "compression {c}");
        assert!(spec.compression_source.contains("seq 11"));
        // 같은 표본을 다시 넣어도 두 번 적재하지 않는다
        assert!(reg.ingest_bead_sample(&s).unwrap().is_none());
    }

    #[test]
    fn bad_samples_are_recorded_with_a_reason() {
        let reg = reg_with_item(ItemSpec::default());
        // 오류 상태
        let bad = sample_from_entry("GR2", &entry(1, 1001, 3, 237.0, 4)).unwrap();
        let got = reg.ingest_bead_sample(&bad).unwrap().unwrap();
        assert!(!got.applied && got.reason.contains("Status"), "{}", got.reason);
        // TotalCount 0
        let mut zero = sample_from_entry("GR2", &entry(2, 1001, 3, 237.0, 2)).unwrap();
        zero.total_count = 0;
        assert!(validate_sample(&zero).unwrap_err().contains("TotalCount"));
        // 비단조 (2 단 상부 비드가 1 단보다 낮다)
        let mut down = sample_from_entry("GR2", &entry(3, 1001, 3, 237.0, 2)).unwrap();
        down.levels[1].upper_bead = Some(10.0);
        assert!(validate_sample(&down).unwrap_err().contains("UpperBead"));
        // upper ≤ lower
        let mut flat = sample_from_entry("GR2", &entry(4, 1001, 3, 237.0, 2)).unwrap();
        flat.levels[0].upper_bead = Some(5.0);
        assert!(validate_sample(&flat).unwrap_err().contains("≤ LowerBead"));
        // 말도 안 되는 값
        let mut huge = sample_from_entry("GR2", &entry(5, 1001, 3, 237.0, 2)).unwrap();
        huge.levels[0].lower_bead = Some(99_999.0);
        assert!(validate_sample(&huge).unwrap_err().contains("밖입니다"));
        // DiagFlags
        let mut diag = sample_from_entry("GR2", &entry(6, 1001, 3, 237.0, 2)).unwrap();
        diag.diag_flags = 0x21;
        assert!(validate_sample(&diag).unwrap_err().contains("DiagFlags"));
        // 규격은 그대로다
        assert!(reg.item(1001).unwrap().unwrap().spec.profiles.is_empty());
    }

    #[test]
    fn manual_rows_survive_and_the_switch_is_honoured() {
        // 3 단 스택의 2 단을 손으로 고쳐 두었다(절대값)
        let manual = spec::BeadProfile {
            count: 3,
            rows: vec![spec::ProfileRow { level: 2, lower_bead: Some(250.0), upper_bead: Some(450.0), stack_height: None, source: spec::SOURCE_MANUAL.into() }],
            ..Default::default()
        };
        let reg = reg_with_item(ItemSpec { stack_max: 5, profiles: vec![manual], ..Default::default() });
        let got = reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(21, 1001, 3, 237.0, 2)).unwrap()).unwrap().unwrap();
        assert!(got.applied);
        assert_eq!(got.detail.as_ref().unwrap().skipped, vec![2]);
        let spec = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!(spec.profile(3).unwrap().abs_upper_bead(2), Some(450.0), "손으로 고친 행을 덮었다");
        assert!(spec.profile(3).unwrap().at_level(2).unwrap().is_manual());
        assert_eq!(spec.profile(3).unwrap().abs_upper_bead(1), Some(220.0));
        assert!(above(&spec, 1).is_manual());
        assert!(got.reason.contains("손으로 고친"));

        // 스위치를 끄면 적재만 하고 반영하지 않는다
        let reg = reg_with_item(ItemSpec { stack_max: 5, auto_apply_measured: Some(false), ..Default::default() });
        let got = reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(22, 1001, 3, 237.0, 2)).unwrap()).unwrap().unwrap();
        assert!(!got.applied && got.reason.contains("AutoApplyMeasured"));
        assert!(reg.item(1001).unwrap().unwrap().spec.profiles.is_empty());
        // 그래도 표본은 남아 있고, 손으로 Apply 하면 들어간다
        assert_eq!(reg.bead_samples(1001, None, 50).unwrap().len(), 1);
        let done = reg.apply_bead_sample(1001, Some("GR2"), 22, true).unwrap();
        assert!(done.applied, "{}", done.reason);
        assert_eq!(reg.item(1001).unwrap().unwrap().spec.measured_aboves(), vec![0, 1, 2]);
        assert!(reg.bead_samples(1001, None, 50).unwrap()[0].applied);
    }

    /// 크기가 다른 두 스택은 프로파일이 나란히 살고, 파생 곡선에서 겹치는 하중은 새 표본이 덮는다.
    #[test]
    fn profiles_of_different_stack_sizes_live_side_by_side() {
        let reg = reg_with_item(ItemSpec { stack_max: 8, ..Default::default() });
        reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(51, 1001, 3, 237.0, 2)).unwrap()).unwrap();
        assert_eq!(reg.item(1001).unwrap().unwrap().spec.measured_counts(), vec![3]);
        reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(52, 1001, 6, 231.0, 2)).unwrap()).unwrap();
        let spec = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!(spec.measured_counts(), vec![3, 6], "3 단 프로파일이 6 단 때문에 사라지지 않는다");
        assert_eq!(spec.profile(3).unwrap().abs_upper_bead(2), Some(237.0 + 220.0));
        assert_eq!(spec.profile(6).unwrap().abs_upper_bead(2), Some(231.0 + 220.0));
        assert_eq!(spec.measured_aboves(), vec![0, 1, 2, 3, 4, 5]);
        // 겹치는 하중 0..2 는 새 표본(seq 52)이 덮었다
        assert_eq!(above(&spec, 1).sample_seq, 52);
        assert_eq!(above(&spec, 1).pressed_height, Some(231.0));
    }

    #[test]
    fn history_is_bounded_and_newest_first() {
        let reg = reg_with_item(ItemSpec { stack_max: 5, ..Default::default() });
        for seq in 1..=(HISTORY_LIMIT as u32 + 5) {
            reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(seq, 1001, 3, 237.0, 2)).unwrap()).unwrap();
        }
        let rows = reg.bead_samples(1001, None, 100).unwrap();
        assert_eq!(rows.len(), HISTORY_LIMIT);
        assert_eq!(rows[0].sample.seq, HISTORY_LIMIT as u32 + 5);
        assert_eq!(rows.last().unwrap().sample.seq, 6);
        // 로봇별 필터
        assert!(reg.bead_samples(1001, Some("GR1"), 100).unwrap().is_empty());
        assert_eq!(reg.bead_samples(1001, Some("GR2"), 100).unwrap().len(), HISTORY_LIMIT);
    }

    #[test]
    fn an_older_sample_can_be_applied_by_hand() {
        let reg = reg_with_item(ItemSpec { stack_max: 5, ..Default::default() });
        reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(31, 1001, 3, 237.0, 2)).unwrap()).unwrap();
        reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(32, 1001, 3, 230.0, 2)).unwrap()).unwrap();
        let before = reg.item(1001).unwrap().unwrap().spec.compression.unwrap();
        let got = reg.apply_bead_sample(1001, None, 31, false).unwrap();
        assert!(got.applied);
        let after = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!(after.profile(3).unwrap().sample_seq, 31);
        assert_eq!(after.profile(3).unwrap().abs_upper_bead(2), Some(237.0 + 220.0));
        assert_eq!(above(&after, 0).pressed_height, Some(237.0));
        assert!((after.compression.unwrap() - before).abs() > 0.1, "눌림양이 옛 표본 값으로 바뀌지 않았다");
        // 없는 seq
        assert!(reg.apply_bead_sample(1001, None, 999, false).is_err());
        // 거부되는 표본은 손으로도 못 넣는다
        reg.ingest_bead_sample(&sample_from_entry("GR2", &entry(33, 1001, 3, 237.0, 4)).unwrap()).unwrap();
        assert!(reg.apply_bead_sample(1001, None, 33, true).is_err());
    }

    #[test]
    fn unknown_item_is_recorded_but_not_applied() {
        let reg = reg_with_item(ItemSpec::default());
        let s = sample_from_entry("GR2", &entry(41, 2002, 3, 237.0, 2)).unwrap();
        let got = reg.ingest_bead_sample(&s).unwrap().unwrap();
        assert!(!got.applied && got.reason.contains("등록"));
        assert_eq!(reg.bead_samples(2002, None, 10).unwrap().len(), 1);
    }
}
