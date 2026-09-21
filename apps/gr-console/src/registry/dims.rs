//! 측정 기록 → 화물 규격 치수(InnerDiameter · UpperBeadHeight · Height) **제안 · 검토 적용 · 이력**.
//!
//! 비드 프로파일(SKU, `beads.rs`)과 달리 이 셋은 `StockItem`(PLC 로 가는 품목 값)이라 로봇 위치를 바로
//! 바꾼다(G = 내경 − 30, 적층 Z). 그래서 **자동으로 쓰지 않는다**(사용자 결정 2026-09-21): 측정으로
//! 제안을 만들고, 사람이 필드를 골라 적용하며, 바꾼 것은 전부 `item_dim_changes` 에 남아 되돌릴 수 있다.
//!
//! # 표본
//!
//! MEASLOG `Kind = 1`(MeasureItem), `Status = 2`(DONE) 만 쓴다 — `MeasureItemReconcile` 이 끝낸 값이다
//! (`MeasureLogAdd.scl` 의 `MEAS_ITEM_*`):
//!
//! | 필드 | Data | 조건 |
//! |---|---|---|
//! | InnerDiameter | [1] 레이저·토크 조정 내경 | — |
//! | UpperBeadHeight | [2] 상부 비드(IN/OUT 평균, 셀 바닥 기준) | 1 단(`Cmd.Item.Count = 1`) |
//! | Height | [3] 타이어 높이(IN/OUT 평균, 셀 바닥 기준) | 1 단 |
//!
//! 높이 둘은 셀 바닥에서 잰 값이라 **1 단일 때만** 품목 치수다. PICK(`Kind = 4`)의 내경은 내려가며 호 일부만
//! 맞춘 값이라(실측 50 mm 넘게 어긋남) 쓰지 않고, SKU `EachHeight` 는 품목 높이와 뜻이 달라 쓰지 않는다.
//!
//! # 제안
//!
//! 필드마다 최근 `window`(기본 5) 개 표본의 **중앙값**(0.1 mm 반올림). 표본 편차(max − min)가 필드 한도를
//! 넘으면 `unstable`, 등록값(0 이 아닌)과 한도 넘게 다르면 `outlier`(재고 품목이 틀려 다른 타이어를 잰 경우 —
//! 실측 예: 20" 품목에 ID 560) — 둘 다 적용은 막지 않지만 기본 선택에서 빠지고 화면이 먼저 말한다. StackMax 는 SKU 에서 실제로 잰 최대 단수를
//! **참고로만** 보인다(적용 대상 아님).

use std::collections::HashMap;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use gr_proto::{MEAS_LOG_KIND_ITEM, MEAS_LOG_KIND_SKU, StockItem};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::xlsx::{self, ItemRow, SpecPatch};
use super::{ItemEntry, Registry};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util::now_str;

pub const DEFAULT_WINDOW: usize = 5;
const MAX_WINDOW: usize = 50;
/// 이만큼(mm) 안 바뀌면 "변경 없음" — 표시 분해능(0.1 mm)의 절반.
const SAME_EPS: f32 = 0.05;
const STATUS_DONE: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimField {
    InnerDiameter,
    UpperBeadHeight,
    Height,
}

impl DimField {
    pub const ALL: [DimField; 3] = [DimField::InnerDiameter, DimField::UpperBeadHeight, DimField::Height];

    pub fn key(self) -> &'static str {
        match self {
            DimField::InnerDiameter => "inner_diameter",
            DimField::UpperBeadHeight => "upper_bead_height",
            DimField::Height => "height",
        }
    }
    pub fn parse(s: &str) -> Option<DimField> {
        DimField::ALL.into_iter().find(|f| f.key() == s)
    }
    /// MEASLOG `Data[]` 인덱스(`MEAS_ITEM_*`).
    fn data_index(self) -> usize {
        match self {
            DimField::InnerDiameter => 1,
            DimField::UpperBeadHeight => 2,
            DimField::Height => 3,
        }
    }
    /// 셀 바닥 기준 높이라 1 단일 때만 품목 치수다.
    fn needs_single(self) -> bool {
        self != DimField::InnerDiameter
    }
    /// 말이 되는 범위(mm) — 밖이면 표본에서 뺀다(센서 미감지 0 · 튄 값).
    fn plausible(self) -> (f32, f32) {
        match self {
            DimField::InnerDiameter => (200.0, 1100.0),
            DimField::UpperBeadHeight => (10.0, 700.0),
            DimField::Height => (50.0, 700.0),
        }
    }
    /// 표본 편차 한도(mm) — 넘으면 `unstable`.
    pub fn spread_limit(self) -> f32 {
        match self {
            DimField::InnerDiameter => 3.0,
            DimField::UpperBeadHeight | DimField::Height => 5.0,
        }
    }
    /// 등록값과 이만큼(mm) 넘게 다르면 `outlier` — 재고 품목이 틀렸을(다른 타이어를 잰) 가능성이 크다.
    pub fn outlier_limit(self) -> f32 {
        match self {
            DimField::InnerDiameter => 15.0,
            DimField::UpperBeadHeight | DimField::Height => 30.0,
        }
    }
    pub fn get(self, it: &StockItem) -> f32 {
        match self {
            DimField::InnerDiameter => it.inner_diameter,
            DimField::UpperBeadHeight => it.upper_bid_height,
            DimField::Height => it.height,
        }
    }
    pub fn set(self, it: &mut StockItem, v: f32) {
        match self {
            DimField::InnerDiameter => it.inner_diameter = v,
            DimField::UpperBeadHeight => it.upper_bid_height = v,
            DimField::Height => it.height = v,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DimSample {
    pub plc: String,
    pub seq: u32,
    pub at: String,
    pub value: f32,
    pub cell_id: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct DimSuggestion {
    pub field: DimField,
    pub current: f32,
    /// 최근 `window` 개 중앙값(0.1 mm). 표본이 없으면 None.
    pub suggested: Option<f32>,
    pub n: usize,
    pub spread: Option<f32>,
    pub spread_limit: f32,
    pub delta: Option<f32>,
    pub unstable: bool,
    /// 등록값(0 이 아닌)과 `outlier_limit` 넘게 다르다 — 재고 품목이 맞는지부터 확인할 것.
    pub outlier: bool,
    pub outlier_limit: f32,
    /// 제안이 지금 값과 다르다(0.05 mm 넘게).
    pub changed: bool,
    /// 제안에 쓴 표본(최신 순).
    pub samples: Vec<DimSample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StackRef {
    pub current: u8,
    /// SKU 에서 실제로 잰 최대 단수(참고).
    pub observed_max: Option<u32>,
    pub samples: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemDims {
    pub code: u32,
    pub name: String,
    pub window: usize,
    pub fields: Vec<DimSuggestion>,
    pub stack_max: StackRef,
    /// 적용할 제안이 하나라도 있다.
    pub has_changes: bool,
}

fn num(j: &Json) -> Option<f32> {
    j.as_f64().map(|v| v as f32).filter(|v| v.is_finite())
}

pub fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

pub fn median(v: &[f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let m = s.len() / 2;
    Some(if s.len() % 2 == 1 { s[m] } else { (s[m - 1] + s[m]) / 2.0 })
}

/// 한 필드의 표본 — 조건을 지난 MeasureItem 기록만, 최신 순.
pub fn samples_for(field: DimField, code: u32, recs: &[(String, Json)]) -> Vec<DimSample> {
    let (lo, hi) = field.plausible();
    let mut out: Vec<DimSample> = recs
        .iter()
        .filter(|(_, e)| e["Kind"].as_u64() == Some(MEAS_LOG_KIND_ITEM as u64) && e["Status"].as_u64() == Some(STATUS_DONE))
        .filter(|(_, e)| e["Cmd"]["Item"]["Code"].as_u64() == Some(code as u64))
        .filter(|(_, e)| !field.needs_single() || e["Cmd"]["Item"]["Count"].as_u64() == Some(1))
        .filter_map(|(plc, e)| {
            let v = num(&e["Data"][field.data_index()])?;
            (lo..=hi).contains(&v).then(|| DimSample {
                plc: plc.clone(),
                seq: e["Seq"].as_u64().unwrap_or(0) as u32,
                at: e["TimeStamp"].as_str().unwrap_or("").to_string(),
                value: v,
                cell_id: e["Cmd"]["Cell"]["Id"].as_u64().unwrap_or(0) as u32,
            })
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at).then(b.seq.cmp(&a.seq)));
    out
}

pub fn suggest(item: &ItemEntry, recs: &[(String, Json)], window: usize) -> ItemDims {
    let window = window.clamp(1, MAX_WINDOW);
    let fields: Vec<DimSuggestion> = DimField::ALL
        .into_iter()
        .map(|field| {
            let mut samples = samples_for(field, item.code, recs);
            samples.truncate(window);
            let vals: Vec<f32> = samples.iter().map(|s| s.value).collect();
            let current = field.get(&item.item);
            let suggested = median(&vals).map(round1);
            let spread = (!vals.is_empty()).then(|| {
                let (mn, mx) = vals.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                round1(mx - mn)
            });
            let delta = suggested.map(|s| round1(s - current));
            DimSuggestion {
                field,
                current,
                suggested,
                n: vals.len(),
                spread,
                spread_limit: field.spread_limit(),
                delta,
                unstable: spread.is_some_and(|s| s > field.spread_limit()),
                outlier: current > 0.0 && delta.is_some_and(|d| d.abs() > field.outlier_limit()),
                outlier_limit: field.outlier_limit(),
                changed: delta.is_some_and(|d| d.abs() > SAME_EPS),
                samples,
            }
        })
        .collect();
    let sku: Vec<u32> = recs
        .iter()
        .filter(|(_, e)| e["Kind"].as_u64() == Some(MEAS_LOG_KIND_SKU as u64) && e["Status"].as_u64() == Some(STATUS_DONE))
        .filter(|(_, e)| e["Cmd"]["Item"]["Code"].as_u64() == Some(item.code as u64))
        .filter_map(|(_, e)| e["Data"][17].as_f64().map(|v| v.round() as u32).filter(|v| *v > 0))
        .collect();
    let has_changes = fields.iter().any(|f| f.changed);
    ItemDims {
        code: item.code,
        name: item.name.clone(),
        window,
        fields,
        stack_max: StackRef { current: item.spec.stack_max, observed_max: sku.iter().copied().max(), samples: sku.len() },
        has_changes,
    }
}

// ---- 이력

#[derive(Clone, Debug, Serialize)]
pub struct DimChange {
    pub id: i64,
    pub code: u32,
    pub field: String,
    pub before: f32,
    pub after: f32,
    /// `measured` | `revert`
    pub source: String,
    pub samples: Json,
    pub note: String,
    pub at: String,
    pub reverted: bool,
}

impl Registry {
    /// `ch` = (필드, 이전, 이후).
    fn add_dim_change(&self, code: u32, ch: (DimField, f32, f32), source: &str, samples: &Json, note: &str) -> Result<i64, ApiError> {
        let (field, before, after) = ch;
        let s = samples.to_string();
        Ok(self.db.with(|c| {
            c.execute(
                "INSERT INTO item_dim_changes (code, field, before, after, source, samples, note, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (code, field.key(), before, after, source, s, note, now_str()),
            )?;
            Ok(c.last_insert_rowid())
        })?)
    }

    pub fn dim_changes(&self, code: Option<u32>, limit: usize) -> Result<Vec<DimChange>, ApiError> {
        type Row = (i64, i64, String, f64, f64, String, String, String, String, i64);
        let rows: Vec<Row> = self.db.with(|c| {
            let mut st = c.prepare("SELECT id, code, field, before, after, source, samples, note, at, reverted FROM item_dim_changes WHERE (?1 IS NULL OR code = ?1) ORDER BY id DESC LIMIT ?2")?;
            let it = st.query_map((code.map(|c| c as i64), limit as i64), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(id, code, field, before, after, source, samples, note, at, rev)| DimChange {
                id,
                code: code as u32,
                field,
                before: before as f32,
                after: after as f32,
                source,
                samples: serde_json::from_str(&samples).unwrap_or(Json::Array(vec![])),
                note,
                at,
                reverted: rev != 0,
            })
            .collect())
    }

    fn dim_change(&self, id: i64) -> Result<Option<DimChange>, ApiError> {
        Ok(self.dim_changes(None, i64::MAX as usize)?.into_iter().find(|c| c.id == id))
    }

    /// 필드 값들을 품목에 쓰고 이력을 남긴다. 값이 그대로인 필드는 건너뛴다. 반환: (갱신된 품목, 이력 id).
    fn set_dims(&self, code: u32, values: &[(DimField, f32, Json)], source: &str, note: &str) -> Result<(ItemEntry, Vec<i64>), ApiError> {
        let e = self.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code}")))?;
        let mut item = e.item.clone();
        let mut changed: Vec<(DimField, f32, f32, &Json)> = Vec::new();
        for (f, v, samples) in values {
            let (lo, hi) = f.plausible();
            if !v.is_finite() || !(lo..=hi).contains(v) {
                return Err(ApiError::BadRequest(format!("{} = {v} 가 범위({lo}..{hi}) 밖", f.key())));
            }
            let before = f.get(&item);
            if (v - before).abs() <= SAME_EPS {
                continue;
            }
            f.set(&mut item, *v);
            changed.push((*f, before, *v, samples));
        }
        if changed.is_empty() {
            return Ok((e, vec![]));
        }
        xlsx::validate_item(&ItemRow { code, name: e.name.clone(), item: item.clone(), note: e.note.clone(), spec: SpecPatch::default() }).map_err(ApiError::BadRequest)?;
        let saved = self.upsert_item_full(code, &e.name, &item, &e.note, None)?;
        let mut ids = Vec::new();
        for (f, before, after, samples) in changed {
            ids.push(self.add_dim_change(code, (f, before, after), source, samples, note)?);
        }
        Ok((saved, ids))
    }

    fn mark_dim_reverted(&self, id: i64) -> Result<(), ApiError> {
        self.db.with(|c| c.execute("UPDATE item_dim_changes SET reverted = 1 WHERE id = ?1", [id]))?;
        Ok(())
    }
}

// ---- 표본 모으기(로봇마다 MEASLOG 미러)

/// MeasureItem · SKU 기록 — `code` 가 있으면 그 품목만. 로봇(PLC)마다 따로 센 Seq 라 plc 를 붙여 둔다.
fn gather(st: &AppState, code: Option<u32>) -> Result<Vec<(String, Json)>, ApiError> {
    let limit = if code.is_some() { 400 } else { 5000 };
    let mut out = Vec::new();
    for r in st.robots.iter() {
        for kind in [MEAS_LOG_KIND_ITEM, MEAS_LOG_KIND_SKU] {
            let (rows, _) = r.measure.entries(None, Some(kind), code, limit)?;
            out.extend(rows.into_iter().map(|e| (r.plc.clone(), e)));
        }
    }
    Ok(out)
}

fn apply_one(st: &AppState, code: u32, fields: &[String], window: usize, note: &str) -> Result<(ItemEntry, Vec<i64>), ApiError> {
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code}")))?;
    let dims = suggest(&e, &gather(st, Some(code))?, window);
    let mut values = Vec::new();
    for k in fields {
        let f = DimField::parse(k).ok_or_else(|| ApiError::BadRequest(format!("알 수 없는 필드 {k}")))?;
        let s = dims.fields.iter().find(|s| s.field == f).expect("all fields");
        let v = s.suggested.ok_or_else(|| ApiError::Conflict(format!("{code} {k}: 측정 표본 없음")))?;
        let refs = json!(s.samples.iter().map(|x| json!({ "plc": x.plc, "seq": x.seq })).collect::<Vec<_>>());
        values.push((f, v, refs));
    }
    st.registry.set_dims(code, &values, "measured", note)
}

// ---- 라우트

#[derive(Deserialize, Default)]
struct WindowQuery {
    window: Option<usize>,
    /// 일괄 목록: `true`(기본)면 적용할 제안이 있는 품목만.
    only_changes: Option<bool>,
}

#[derive(Deserialize)]
struct ApplyBody {
    fields: Vec<String>,
    #[serde(default)]
    window: Option<usize>,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct BulkItem {
    code: u32,
    fields: Vec<String>,
}
#[derive(Deserialize)]
struct BulkBody {
    items: Vec<BulkItem>,
    #[serde(default)]
    window: Option<usize>,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize, Default)]
struct ChangesQuery {
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
struct RevertQuery {
    force: Option<String>,
}

async fn item_dims(State(st): State<AppState>, Path(code): Path<u32>, Query(q): Query<WindowQuery>) -> ApiResult<ItemDims> {
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code}")))?;
    Ok(axum::Json(suggest(&e, &gather(&st, Some(code))?, q.window.unwrap_or(DEFAULT_WINDOW))))
}

async fn dims_suggest(State(st): State<AppState>, Query(q): Query<WindowQuery>) -> ApiResult<Vec<ItemDims>> {
    let recs = gather(&st, None)?;
    let mut by: HashMap<u64, Vec<(String, Json)>> = HashMap::new();
    for (plc, e) in recs {
        by.entry(e["Cmd"]["Item"]["Code"].as_u64().unwrap_or(0)).or_default().push((plc, e));
    }
    let only = q.only_changes.unwrap_or(true);
    let window = q.window.unwrap_or(DEFAULT_WINDOW);
    let out = st
        .registry
        .items()?
        .iter()
        .filter_map(|it| {
            let d = suggest(it, by.get(&(it.code as u64)).map(Vec::as_slice).unwrap_or(&[]), window);
            (if only { d.has_changes } else { d.fields.iter().any(|f| f.n > 0) }).then_some(d)
        })
        .collect();
    Ok(axum::Json(out))
}

async fn dims_apply(State(st): State<AppState>, Path(code): Path<u32>, axum::Json(b): axum::Json<ApplyBody>) -> ApiResult<Json> {
    let (item, ids) = apply_one(&st, code, &b.fields, b.window.unwrap_or(DEFAULT_WINDOW), &b.note)?;
    Ok(axum::Json(json!({ "code": item.code, "changes": ids, "item": item })))
}

async fn dims_apply_bulk(State(st): State<AppState>, axum::Json(b): axum::Json<BulkBody>) -> ApiResult<Json> {
    let window = b.window.unwrap_or(DEFAULT_WINDOW);
    let results: Vec<Json> = b
        .items
        .iter()
        .map(|it| match apply_one(&st, it.code, &it.fields, window, &b.note) {
            Ok((_, ids)) => json!({ "code": it.code, "ok": true, "changes": ids }),
            Err(e) => json!({ "code": it.code, "ok": false, "error": e.to_string() }),
        })
        .collect();
    let applied = results.iter().filter(|r| r["ok"] == true).count();
    Ok(axum::Json(json!({ "applied": applied, "failed": results.len() - applied, "results": results })))
}

async fn item_dim_changes(State(st): State<AppState>, Path(code): Path<u32>, Query(q): Query<ChangesQuery>) -> ApiResult<Vec<DimChange>> {
    Ok(axum::Json(st.registry.dim_changes(Some(code), q.limit.unwrap_or(50))?))
}

async fn all_dim_changes(State(st): State<AppState>, Query(q): Query<ChangesQuery>) -> ApiResult<Vec<DimChange>> {
    Ok(axum::Json(st.registry.dim_changes(None, q.limit.unwrap_or(100))?))
}

/// 되돌리기 — 지금 값이 그 변경의 `after` 그대로일 때만(그 뒤에 누가 또 고쳤으면 409, `?force=1` 로 강행).
async fn dim_revert(State(st): State<AppState>, Path((code, id)): Path<(u32, i64)>, Query(q): Query<RevertQuery>) -> ApiResult<Json> {
    let ch = st.registry.dim_change(id)?.filter(|c| c.code == code).ok_or_else(|| ApiError::NotFound(format!("change {id}")))?;
    if ch.reverted {
        return Err(ApiError::Conflict(format!("변경 #{id} 는 이미 되돌렸습니다")));
    }
    let f = DimField::parse(&ch.field).ok_or_else(|| ApiError::BadRequest(format!("알 수 없는 필드 {}", ch.field)))?;
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code}")))?;
    let now = f.get(&e.item);
    let force = matches!(q.force.as_deref(), Some("1" | "true"));
    if (now - ch.after).abs() > SAME_EPS && !force {
        return Err(ApiError::Conflict(format!("{} 의 지금 값 {now} 가 변경 #{id} 의 적용 값 {} 와 다릅니다 — 그 뒤에 바뀌었습니다", f.key(), ch.after)));
    }
    let (item, ids) = st.registry.set_dims(code, &[(f, ch.before, json!([]))], "revert", &format!("revert #{id}"))?;
    st.registry.mark_dim_reverted(id)?;
    Ok(axum::Json(json!({ "code": item.code, "changes": ids, "item": item })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/items/dims/suggest", get(dims_suggest))
        .route("/api/items/dims/apply-bulk", post(dims_apply_bulk))
        .route("/api/items/dims/changes", get(all_dim_changes))
        .route("/api/items/{code}/dims", get(item_dims))
        .route("/api/items/{code}/dims/apply", post(dims_apply))
        .route("/api/items/{code}/dims/changes", get(item_dim_changes))
        .route("/api/items/{code}/dims/changes/{id}/revert", post(dim_revert))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: u32, id: f32, ub: f32, h: f32) -> ItemEntry {
        ItemEntry {
            code,
            name: "t".into(),
            item: StockItem { code, count: 1, inner_diameter: id, upper_bid_height: ub, height: h, ..Default::default() },
            note: String::new(),
            spec: Default::default(),
            updated_at: String::new(),
        }
    }
    fn rec(seq: u32, kind: u8, status: u64, code: u32, count: u32, data: &[f32]) -> (String, Json) {
        let mut d = vec![0.0f32; 20];
        d[..data.len()].copy_from_slice(data);
        (
            "GR2".into(),
            json!({ "Seq": seq, "Kind": kind, "Status": status, "TimeStamp": format!("2026-09-21 10:00:{seq:02}"), "Data": d,
            "Cmd": { "Item": { "Code": code, "Count": count }, "Cell": { "Id": 408 } } }),
        )
    }

    #[test]
    fn median_and_round() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(round1(370.4901), 370.5);
    }

    #[test]
    fn samples_filter_kind_status_code_count_range() {
        let recs = vec![
            rec(1, 1, 2, 1501, 1, &[2.0, 370.5, 199.6, 230.9]),
            rec(2, 1, 4, 1501, 1, &[4.0, 360.0, 199.0, 230.0]), // MISMATCH — 제외
            rec(3, 4, 2, 1501, 1, &[2.0, 318.7]),               // PICK — 제외
            rec(4, 1, 2, 1502, 1, &[2.0, 371.0, 200.0, 231.0]), // 다른 품목
            rec(5, 1, 2, 1501, 3, &[2.0, 371.0, 600.0, 690.0]), // 3 단 — 높이는 제외, 내경은 포함
            rec(6, 1, 2, 1501, 1, &[2.0, 0.0, 0.0, 0.0]),       // 미감지 0 — 범위 밖
        ];
        let id: Vec<u32> = samples_for(DimField::InnerDiameter, 1501, &recs).iter().map(|s| s.seq).collect();
        assert_eq!(id, vec![5, 1]); // 최신 순
        let h: Vec<u32> = samples_for(DimField::Height, 1501, &recs).iter().map(|s| s.seq).collect();
        assert_eq!(h, vec![1]);
    }

    #[test]
    fn suggest_median_window_spread() {
        let recs: Vec<_> = [370.0, 371.0, 372.0, 380.0, 369.0, 300.5].iter().enumerate().map(|(i, v)| rec(i as u32 + 1, 1, 2, 1501, 1, &[2.0, *v, 200.0, 230.0])).collect();
        let d = suggest(&item(1501, 372.0, 200.0, 225.0), &recs, 5);
        let id = &d.fields[0];
        // 최신 5 개 = seq 6..2 → 300.5(범위 안) 372 380 369 371 → 중앙 371
        assert_eq!(id.n, 5);
        assert_eq!(id.suggested, Some(371.0));
        assert_eq!(id.delta, Some(-1.0));
        assert!(id.changed);
        assert!(id.unstable); // 편차 79.5 > 3
        let h = &d.fields[2];
        assert_eq!(h.suggested, Some(230.0));
        assert_eq!(h.delta, Some(5.0));
        assert!(!id.outlier); // |−1| ≤ 15
        let ub = &d.fields[1];
        assert!(!ub.changed); // 200 = 200
        assert!(d.has_changes);
    }

    #[test]
    fn outlier_only_against_a_registered_value() {
        let recs = vec![rec(1, 1, 2, 2001, 1, &[2.0, 559.9, 273.0, 304.0])];
        let d = suggest(&item(2001, 504.0, 0.0, 245.0), &recs, 5);
        assert!(d.fields[0].outlier); // ID +55.9 > 15
        assert!(!d.fields[1].outlier); // 비드 등록값 0 = 미입력 → 채우는 것이라 경고 아님
        assert!(d.fields[2].outlier); // 높이 +59 > 30
    }

    #[test]
    fn stack_reference_from_sku() {
        let mut d = [0.0f32; 18];
        d[0] = 2.0;
        d[17] = 6.0;
        let recs = vec![
            rec(1, 2, 2, 2011, 6, &d),
            rec(2, 2, 3, 2011, 9, &{
                let mut e = d;
                e[17] = 9.0;
                e
            }),
        ];
        let s = suggest(&item(2011, 377.0, 0.0, 296.0), &recs, 5).stack_max;
        assert_eq!(s.observed_max, Some(6)); // 오류(3) 기록은 제외
        assert_eq!(s.samples, 1);
    }

    #[test]
    fn set_dims_writes_history_and_skips_same() {
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        reg.upsert_item_full(1501, "225/45ZR15", &StockItem { code: 1501, count: 1, inner_diameter: 372.0, height: 225.0, ..Default::default() }, "", None).unwrap();
        let (it, ids) = reg.set_dims(1501, &[(DimField::InnerDiameter, 370.5, json!([{"plc":"GR2","seq":475}])), (DimField::Height, 225.0, json!([]))], "measured", "").unwrap();
        assert_eq!(it.item.inner_diameter, 370.5);
        assert_eq!(ids.len(), 1); // 높이는 그대로라 이력 없음
        let ch = reg.dim_changes(Some(1501), 10).unwrap();
        assert_eq!((ch[0].before, ch[0].after, ch[0].field.as_str()), (372.0, 370.5, "inner_diameter"));
        assert!(reg.set_dims(1501, &[(DimField::InnerDiameter, 5.0, json!([]))], "measured", "").is_err()); // 범위 밖
    }
}
