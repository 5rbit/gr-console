//! 스케줄링 · 생성 파라미터 — 한 곳(sqlite `sched_params`, 버전별 JSON + 바뀐 값 이력). 영역 · 실행기 · 생성 엔진 ·
//! 동기화 · 재고 검사가 모두 여기서 읽는다(흩어진 상수 없음). 화면은 `spec()` 으로 단위 · 범위 · 기본값 · 출처 · 도움말을 받는다.

use std::collections::BTreeMap;
use std::sync::{PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// 두 로봇 X 사이 최소 간격(mm) — PLC PARA p11/p12 + p16 + p13.
    pub anticol_separation_mm: f32,
    pub anticol_enabled: bool,
    /// 로봇별 추가 여유(mm, 영역 양쪽) — 그리퍼 · 타이어 폭.
    pub robot_margin_mm: BTreeMap<u8, f32>,
    /// 생성 판정 주기(ms).
    pub gen_tick_ms: u64,
    /// 거리 감점(점수/m).
    pub gen_distance_per_m: f32,
    /// 로봇에서 첫 목표까지 이보다 멀면 후보로 안 본다(m, 0 = 제한 없음).
    pub gen_max_distance_m: f32,
    /// 대기 가점(점수/분).
    pub gen_age_per_min: f32,
    /// 영역에 막힌 후보 표시 감점.
    pub gen_blocked_penalty: f32,
    /// 새 스테이션 요청 규칙의 기본값 — Req AND CVOK.
    pub station_require_cvok: bool,
    /// PLC 에 실행 중 + 이만큼만(고정 1).
    pub issue_queue_depth: u32,
    /// 영역 교착으로 보기 전 기다림(ms).
    pub area_deadlock_ms: u64,
    /// 에코 대기 한계(ms, 없으면 gr-console.toml `cmd.echo_timeout_ms`).
    pub echo_timeout_ms: Option<u64>,
    /// 제출 실패 뒤 다시 시도까지(ms) · 최대 횟수(생성 발행기).
    pub issue_retry_backoff_ms: u64,
    pub issue_max_retries: u32,
    /// 짝: PICK 이 PLC 에 받아진 뒤 DROP(고정) · 짝 사이 정지 미룸(고정).
    pub pair_drop_after_pick_accepted: bool,
    pub pair_defer_stop: bool,
    /// StackMax 초과 DROP 을 제출 때 거부(끄면 경고만).
    pub stack_max_enforce: bool,
    /// 동기화 경고가 서기까지(ms) · 판정 주기(ms).
    pub sync_debounce_ms: u64,
    pub sync_poll_ms: u64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            anticol_separation_mm: SAFE_SEPARATION_MM,
            anticol_enabled: true,
            robot_margin_mm: BTreeMap::new(),
            gen_tick_ms: 1000,
            gen_distance_per_m: 0.0,
            gen_max_distance_m: 0.0,
            gen_age_per_min: 0.0,
            gen_blocked_penalty: 0.0,
            station_require_cvok: true,
            issue_queue_depth: 1,
            area_deadlock_ms: 10_000,
            echo_timeout_ms: None,
            issue_retry_backoff_ms: 2000,
            issue_max_retries: 3,
            pair_drop_after_pick_accepted: true,
            pair_defer_stop: true,
            stack_max_enforce: true,
            sync_debounce_ms: 3000,
            sync_poll_ms: 1000,
        }
    }
}

impl Params {
    pub fn margin(&self, robot: u8) -> f32 {
        self.robot_margin_mm.get(&robot).copied().unwrap_or(0.0).max(0.0)
    }
}

/// 한 파라미터의 설명(화면).
#[derive(Clone, Debug, Serialize)]
pub struct Spec {
    pub key: &'static str,
    pub group: &'static str,
    pub unit: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// 바꿀 수 없음(안전 규칙) — 보이기만.
    pub locked: bool,
    /// PLC 값을 따라가는 파라미터의 출처.
    pub source: Option<&'static str>,
    pub help: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn s(key: &'static str, group: &'static str, unit: &'static str, min: Option<f64>, max: Option<f64>, locked: bool, source: Option<&'static str>, help: &'static str) -> Spec {
    Spec { key, group, unit, min, max, locked, source, help }
}

pub fn spec() -> Vec<Spec> {
    vec![
        s(
            "anticol_separation_mm",
            "AntiCollision",
            "mm",
            Some(0.0),
            Some(20000.0),
            false,
            Some("하한 = PLC PARA p11/p12 XLength + p16 AntiColMargin_Avoid + p13 AntiColMargin_Default (두 로봇 중 큰 값)"),
            "두 로봇 X 사이 최소 간격. 이 안에 두 로봇 작업 영역이 겹치면 생성·발행하지 않는다. 기본 5000 mm(안전), PLC 계산값보다 작게는 저장할 수 없다.",
        ),
        s("anticol_enabled", "AntiCollision", "", None, None, false, None, "영역 검사 사용(로봇이 하나면 어차피 안 한다)."),
        s("robot_margin_mm", "AntiCollision", "mm", Some(0.0), Some(3000.0), false, None, "로봇별 추가 여유(영역 양쪽) — 그리퍼·타이어 폭. 예: {\"1\": 200}"),
        s("gen_tick_ms", "Generation", "ms", Some(200.0), Some(10000.0), false, None, "생성 판정·발행 주기."),
        s("gen_distance_per_m", "Generation", "score/m", Some(0.0), Some(100.0), false, None, "로봇에서 첫 목표까지 거리 1 m 마다 빼는 점수(가까운 로봇 먼저)."),
        s("gen_max_distance_m", "Generation", "m", Some(0.0), Some(200.0), false, None, "이보다 먼 후보는 만들지 않는다(0 = 제한 없음)."),
        s("gen_age_per_min", "Generation", "score/min", Some(0.0), Some(100.0), false, None, "조건이 참이 된 뒤 1 분마다 더하는 점수(오래 기다린 것 먼저)."),
        s("gen_blocked_penalty", "Generation", "score", Some(0.0), Some(1000.0), false, None, "영역에 막힌 후보의 표시 감점(막힌 후보는 어차피 만들지 않는다)."),
        s("station_require_cvok", "Generation", "", None, None, false, None, "새 스테이션 요청 규칙의 기본: Req AND CVOK(컨베이어 준비)."),
        s("issue_queue_depth", "Issue", "건", Some(1.0), Some(1.0), true, None, "PLC 에는 실행 중 + 다음 1 건까지만(안전 규칙, 고정)."),
        s("area_deadlock_ms", "Issue", "ms", Some(1000.0), Some(600000.0), false, None, "서 있는 로봇이 영역을 막을 때 교착으로 보고 멈추기까지(시나리오 실행기)."),
        s("echo_timeout_ms", "Issue", "ms", Some(500.0), Some(60000.0), false, Some("gr-console.toml cmd.echo_timeout_ms (비우면 그 값)"), "제출 뒤 PLC 에코를 기다리는 한계 — 넘으면 Failed."),
        s("issue_retry_backoff_ms", "Issue", "ms", Some(0.0), Some(600000.0), false, None, "생성 발행기: 제출 실패 뒤 다시 시도까지."),
        s("issue_max_retries", "Issue", "회", Some(0.0), Some(20.0), false, None, "생성 발행기: 제출 실패를 이만큼 넘기면 그 예정을 중단(이송 지시 aborted)."),
        s("pair_drop_after_pick_accepted", "Pair", "", None, None, true, None, "짝 DROP 은 PICK 이 PLC 에 받아진 뒤에만(고정)."),
        s("pair_defer_stop", "Pair", "", None, None, true, None, "PICK 이 나가면 짝 DROP 제출까지 정지·일시정지를 미룬다(고정)."),
        s("stack_max_enforce", "Stack", "", None, None, false, None, "StackMax 를 넘는 DROP 을 제출 때 거부(끄면 경고만)."),
        s("sync_debounce_ms", "Sync", "ms", Some(0.0), Some(60000.0), false, None, "로봇 실제 상태 불일치가 이만큼 이어져야 경고·자동 반영."),
        s("sync_poll_ms", "Sync", "ms", Some(200.0), Some(10000.0), false, None, "동기화 판정 주기."),
    ]
}

/// 범위·잠금 검사 — 잠긴 값은 기본값이어야 한다.
pub fn validate(p: &Params) -> Result<(), String> {
    let v = serde_json::to_value(p).map_err(|e| e.to_string())?;
    let d = serde_json::to_value(Params::default()).map_err(|e| e.to_string())?;
    for sp in spec() {
        let x = &v[sp.key];
        if sp.locked && x != &d[sp.key] {
            return Err(format!("{} 은 고정값입니다", sp.key));
        }
        let nums: Vec<f64> = match x {
            Json::Number(n) => n.as_f64().into_iter().collect(),
            Json::Object(m) => m.values().filter_map(Json::as_f64).collect(),
            _ => vec![],
        };
        for n in nums {
            if !n.is_finite() || sp.min.is_some_and(|m| n < m) || sp.max.is_some_and(|m| n > m) {
                return Err(format!("{} = {n} — 범위 {}..{} {}", sp.key, sp.min.unwrap_or(f64::MIN), sp.max.unwrap_or(f64::MAX), sp.unit));
            }
        }
    }
    Ok(())
}

/// 바뀐 값 하나 `(key, old, new)`.
pub type Change = (String, Json, Json);

/// 바뀐 값 `(key, old, new)`.
pub fn diff(old: &Params, new: &Params) -> Vec<Change> {
    let a = serde_json::to_value(old).unwrap_or_default();
    let b = serde_json::to_value(new).unwrap_or_default();
    spec().iter().filter(|sp| a[sp.key] != b[sp.key]).map(|sp| (sp.key.to_string(), a[sp.key].clone(), b[sp.key].clone())).collect()
}

/// 안전 기본 간격(mm) — PLC 계산값(p11 + p16 + p13 = 2403)보다 넉넉하게(사용자 결정 2026-09-21).
pub const SAFE_SEPARATION_MM: f32 = 5000.0;
/// 예전 기본값(PLC 합) — 저장본이 이 값이고 사람이 바꾼 적이 없으면 새 기본값으로 올린다.
pub const OLD_DEFAULT_SEPARATION_MM: f32 = 2403.0;

static CACHE: RwLock<Option<(u32, Params)>> = RwLock::new(None);

/// 사람이 `anticol_separation_mm` 을 저장한 적이 있나(이력의 바뀐 값에 그 키가 있나).
pub fn separation_user_set(db: &Db) -> bool {
    db.with(|c| c.query_row("SELECT COUNT(*) FROM sched_params WHERE changes_json LIKE '%\"anticol_separation_mm\"%'", [], |r| r.get::<_, i64>(0))).unwrap_or(0) > 0
}

/// 예전 기본값(2403) 그대로인 저장본을 안전 기본값(5000)으로 — 사람이 직접 저장한 값은 그대로 둔다.
pub fn migrate_separation(p: &mut Params, user_set: bool) -> bool {
    if !user_set && (p.anticol_separation_mm - OLD_DEFAULT_SEPARATION_MM).abs() < 0.5 {
        p.anticol_separation_mm = SAFE_SEPARATION_MM;
        return true;
    }
    false
}

/// PLC 계산 간격(두 로봇 중 큰 값)보다 작은 간격은 저장하지 않는다.
pub fn check_plc_floor(sep: f32, plc_max: Option<f64>) -> Result<(), String> {
    match plc_max {
        Some(m) if (sep as f64) < m => Err(format!("anticol_separation_mm {sep:.0} mm 는 PLC 계산 간격 {m:.0} mm(p11/p12 + p16 + p13, 두 로봇 중 큰 값)보다 작을 수 없습니다")),
        _ => Ok(()),
    }
}

/// 화면 경고 — 사람이 저장한 간격이 안전 기본값보다 작다.
pub fn warnings(db: &Db, p: &Params) -> Vec<String> {
    let mut out = Vec::new();
    if p.anticol_separation_mm < SAFE_SEPARATION_MM && separation_user_set(db) {
        out.push(format!("anticol_separation_mm {:.0} mm 는 사용자가 저장한 값 — 안전 기본값 {SAFE_SEPARATION_MM:.0} mm 보다 작습니다", p.anticol_separation_mm));
    }
    out
}

/// 지금 값(캐시). 저장된 것이 없으면 기본값 — 옛 `settings.anticol` 이 있으면 간격·사용을 거기서 가져온다.
pub fn current(db: &Db) -> Params {
    if let Some((_, p)) = CACHE.read().unwrap_or_else(PoisonError::into_inner).as_ref() {
        return p.clone();
    }
    let (v, p) = load(db);
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = Some((v, p.clone()));
    p
}

pub fn version(db: &Db) -> u32 {
    current(db);
    CACHE.read().unwrap_or_else(PoisonError::into_inner).as_ref().map(|(v, _)| *v).unwrap_or(0)
}

fn load(db: &Db) -> (u32, Params) {
    let row: Option<(u32, String)> = db
        .with(|c| {
            c.query_row("SELECT version, doc_json FROM sched_params ORDER BY version DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(Some)
                .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })
        .ok()
        .flatten();
    if let Some((v, d)) = row {
        let mut p: Params = serde_json::from_str(&d).unwrap_or_default();
        migrate_separation(&mut p, separation_user_set(db));
        return (v, p);
    }
    let mut p = Params::default();
    if let Ok(Some(s)) = db.setting("anticol")
        && let Ok(j) = serde_json::from_str::<Json>(&s)
    {
        if let Some(x) = j["separation_mm"].as_f64() {
            p.anticol_separation_mm = x as f32;
            migrate_separation(&mut p, false);
        }
        if let Some(b) = j["enabled"].as_bool() {
            p.anticol_enabled = b;
        }
    }
    (0, p)
}

/// 저장 — 검사 → 버전 +1 → 바뀐 값 이력. 바뀐 게 없으면 저장하지 않는다.
pub fn save(db: &Db, new: Params, by: &str) -> Result<(u32, Params, Vec<Change>), ApiError> {
    validate(&new).map_err(ApiError::BadRequest)?;
    let old = current(db);
    let changes = diff(&old, &new);
    let v = version(db);
    if changes.is_empty() {
        return Ok((v, old, changes));
    }
    let nv = v + 1;
    let changes_json = serde_json::to_string(&changes.iter().map(|(k, a, b)| json!({ "key": k, "old": a, "new": b })).collect::<Vec<_>>())?;
    db.with(|c| {
        c.execute(
            "INSERT INTO sched_params (version, doc_json, saved_at, saved_by, changes_json) VALUES (?1,?2,?3,?4,?5)",
            (nv, serde_json::to_string(&new).unwrap_or_default(), now_str(), by, &changes_json),
        )
    })?;
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = Some((nv, new.clone()));
    Ok((nv, new, changes))
}

/// 저장 이력(최신 먼저).
pub fn history(db: &Db, limit: u32) -> Result<Vec<Json>, ApiError> {
    Ok(db.with(|c| {
        let mut st = c.prepare("SELECT version, saved_at, saved_by, changes_json FROM sched_params ORDER BY version DESC LIMIT ?1")?;
        let it = st.query_map([limit], |r| {
            let ch: String = r.get(3)?;
            Ok(json!({ "version": r.get::<_, u32>(0)?, "saved_at": r.get::<_, String>(1)?, "saved_by": r.get::<_, String>(2)?, "changes": serde_json::from_str::<Json>(&ch).unwrap_or(Json::Null) }))
        })?;
        it.collect::<Result<Vec<_>, _>>()
    })?)
}

/// 시험용 — 캐시를 비운다(DB 마다 새로 읽게).
#[cfg(test)]
pub fn reset_cache() {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// PLC PARA 에서 읽은 충돌 방지 값(로봇별) — 콘솔 값과 비교해 보인다.
#[derive(Clone, Debug, Serialize)]
pub struct PlcAnticol {
    pub robot: u8,
    pub name: String,
    pub machine_id: Option<u64>,
    pub x_length_front: Option<f64>,
    pub x_length_rear: Option<f64>,
    pub margin_default: Option<f64>,
    pub margin_pos: Option<f64>,
    pub margin_avoid: Option<f64>,
    /// p11(p12) + p16 + p13 — 콘솔 간격과 같은 식.
    pub separation: Option<f64>,
}

pub fn plc_anticol(st: &crate::state::AppState) -> Vec<PlcAnticol> {
    st.robots
        .iter()
        .map(|r| {
            let m = st.robot_plc(r).ok().and_then(|h| h.decode_path("PARA", "Machine"));
            let g = |k: &str| m.as_ref().and_then(|m| m[k].as_f64());
            let (front, rear, def, avoid) = (g("XLengthFront"), g("XLengthRear"), g("AntiColMargin_Default"), g("AntiColMargin_Avoid"));
            let len = front.into_iter().chain(rear).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
            PlcAnticol {
                robot: r.id,
                name: r.name.clone(),
                machine_id: m.as_ref().and_then(|m| m["ID"].as_u64()),
                x_length_front: front,
                x_length_rear: rear,
                margin_default: def,
                margin_pos: g("AntiColMargin_Pos"),
                margin_avoid: avoid,
                separation: match (len, avoid, def) {
                    (Some(l), Some(a), Some(d)) => Some(l + a + d),
                    _ => None,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_locked_values_stay_fixed() {
        let p = Params::default();
        assert_eq!(p.anticol_separation_mm, 5000.0, "safe default");
        assert!(validate(&p).is_ok());
        let mut bad = p.clone();
        bad.issue_queue_depth = 2;
        assert!(validate(&bad).unwrap_err().contains("고정"));
        let mut bad = p.clone();
        bad.gen_tick_ms = 50;
        assert!(validate(&bad).unwrap_err().contains("gen_tick_ms"));
        let mut bad = p.clone();
        bad.robot_margin_mm.insert(1, 99999.0);
        assert!(validate(&bad).is_err());
        // 스펙이 모든 필드를 덮는다
        let keys: Vec<&str> = spec().iter().map(|s| s.key).collect();
        let v = serde_json::to_value(&p).unwrap();
        for k in v.as_object().unwrap().keys() {
            assert!(keys.contains(&k.as_str()), "{k} 에 스펙 없음");
        }
    }

    #[test]
    fn save_versions_and_history_with_diff() {
        reset_cache();
        let db = Db::open_memory().unwrap();
        let mut p = current(&db);
        p.anticol_separation_mm = 2500.0;
        p.robot_margin_mm.insert(2, 150.0);
        let (v, _, ch) = save(&db, p.clone(), "tester").unwrap();
        assert_eq!(v, 1);
        assert_eq!(ch.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["anticol_separation_mm", "robot_margin_mm"]);
        assert_eq!(save(&db, p.clone(), "tester").unwrap().0, 1, "no change → no new version");
        let h = history(&db, 10).unwrap();
        assert_eq!(h[0]["saved_by"], "tester");
        assert_eq!(h[0]["changes"][0]["old"], 5000.0);
        assert_eq!(current(&db).margin(2), 150.0);
        // 되돌리기 = 기본값 저장
        let (v, back, _) = save(&db, Params::default(), "tester").unwrap();
        assert_eq!((v, back.anticol_separation_mm), (2, 5000.0));
        reset_cache();
    }

    #[test]
    fn old_default_migrates_but_user_values_stay() {
        let mut p = Params { anticol_separation_mm: 2403.0, ..Default::default() };
        assert!(migrate_separation(&mut p, false));
        assert_eq!(p.anticol_separation_mm, 5000.0);
        let mut u = Params { anticol_separation_mm: 2403.0, ..Default::default() };
        assert!(!migrate_separation(&mut u, true), "user saved 2403 explicitly");
        assert_eq!(u.anticol_separation_mm, 2403.0);
        let mut other = Params { anticol_separation_mm: 3000.0, ..Default::default() };
        assert!(!migrate_separation(&mut other, false));
        // 저장본(사람이 바꾼 적 없는 예전 기본값)은 읽을 때 5000, 사람이 저장한 2403 은 그대로 + 경고
        let db = Db::open_memory().unwrap();
        let old = serde_json::to_string(&Params { anticol_separation_mm: 2403.0, ..Default::default() }).unwrap();
        db.with(|c| c.execute("INSERT INTO sched_params (version, doc_json, saved_at, saved_by, changes_json) VALUES (1, ?1, 'a', 'x', '[{\"key\":\"gen_tick_ms\"}]')", [&old])).unwrap();
        assert_eq!(load(&db).1.anticol_separation_mm, 5000.0);
        db.with(|c| {
            c.execute(
                "INSERT INTO sched_params (version, doc_json, saved_at, saved_by, changes_json) VALUES (2, ?1, 'b', 'op', '[{\"key\":\"anticol_separation_mm\",\"old\":5000,\"new\":2403}]')",
                [&old],
            )
        })
        .unwrap();
        let (_, kept) = load(&db);
        assert_eq!(kept.anticol_separation_mm, 2403.0);
        assert!(warnings(&db, &kept)[0].contains("사용자가 저장"));
    }

    #[test]
    fn separation_cannot_go_below_the_plc_value() {
        assert!(check_plc_floor(2000.0, Some(2403.0)).unwrap_err().contains("2403"));
        assert!(check_plc_floor(2403.0, Some(2403.0)).is_ok());
        assert!(check_plc_floor(100.0, None).is_ok(), "no PLC snapshot — range check only");
    }
}
