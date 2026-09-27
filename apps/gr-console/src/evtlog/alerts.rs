//! Alert rules: judged by the writer thread on every stored row (PLC and console), matches become `alerts` rows and
//! the `alert` SSE event on `/api/events/stream`.
//!
//! Rows that reach the console long after they happened (collector backlog after a restart) do not alert — see
//! [`BACKLOG_MS`].

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError, RwLock};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use evt_catalog::Catalog;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::catalog::{EventRow, Texts, now_ms};
use super::routes::{log, num, opt};
use super::store::{Row, Store};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// A row stored more than this after its own time (rx_ts − plc_ts) is backlog, not news.
pub const BACKLOG_MS: i64 = 10 * 60_000;

/// Stored `match_json`. Every present field must hold; nothing present = every row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plc: Option<String>,
    /// Category name or id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cat: Option<String>,
    /// Event names or code numbers, any of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
    /// Level name or id — this level and above.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_lvl: Option<String>,
    /// Case-insensitive, over the rendered text, the event name and the detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_contains: Option<String>,
    /// Exact value of A (on/off events: 1 = ON only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_eq: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    #[serde(rename = "match")]
    pub m: RuleMatch,
    pub cooldown_s: u32,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    pub id: i64,
    pub name: String,
    plc: Option<String>,
    cat: Option<u8>,
    /// (category when given by name, code).
    codes: Vec<(Option<u8>, u32)>,
    min_lvl: Option<u8>,
    text: Option<String>,
    a_eq: Option<i64>,
    cooldown_ms: i64,
}

fn blank(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Checks the names against the catalog. Disabled rules compile too (the API validates them the same way).
pub fn compile(r: &Rule, cat: &Catalog) -> Result<Compiled, String> {
    if r.name.trim().is_empty() {
        return Err("name 이 비었습니다".into());
    }
    let m = &r.m;
    let cat_id = blank(&m.cat).map(|c| c.parse::<u8>().ok().or_else(|| cat.cat_id(c)).ok_or_else(|| format!("cat: {c} 를 모릅니다"))).transpose()?;
    let min_lvl = blank(&m.min_lvl).map(|l| l.parse::<u8>().ok().or_else(|| cat.level_id(l)).ok_or_else(|| format!("min_lvl: {l} 를 모릅니다"))).transpose()?;
    let mut codes = Vec::new();
    for c in m.codes.iter().map(|c| c.trim()).filter(|c| !c.is_empty()) {
        match c.parse::<u32>() {
            Ok(n) => codes.push((None, n)),
            Err(_) => {
                let e = cat.event_by_name(c).ok_or_else(|| format!("codes: {c} 이벤트를 모릅니다"))?;
                codes.push((Some(e.cat), e.code.map(u32::from).unwrap_or(0)));
            }
        }
    }
    Ok(Compiled {
        id: r.id,
        name: r.name.trim().to_string(),
        plc: blank(&m.plc).map(str::to_string),
        cat: cat_id,
        codes,
        min_lvl,
        text: blank(&m.text_contains).map(str::to_lowercase),
        a_eq: m.a_eq,
        cooldown_ms: i64::from(r.cooldown_s) * 1000,
    })
}

impl Compiled {
    /// `text` = lower-cased rendered text + name + detail (only asked for when the rule has `text_contains`).
    pub fn matches(&self, r: &Row, text: &mut dyn FnMut() -> String) -> bool {
        if self.plc.as_ref().is_some_and(|p| !p.eq_ignore_ascii_case(&r.plc)) {
            return false;
        }
        if self.cat.is_some_and(|c| c != r.cat) {
            return false;
        }
        if self.min_lvl.is_some_and(|l| r.lvl < l) {
            return false;
        }
        if !self.codes.is_empty() && !self.codes.iter().any(|(k, c)| *c == r.code && k.is_none_or(|k| k == r.cat)) {
            return false;
        }
        if self.a_eq.is_some_and(|a| a != r.a) {
            return false;
        }
        match &self.text {
            Some(t) => text().contains(t.as_str()),
            None => true,
        }
    }
}

/// One alert the writer is about to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fire {
    pub ts: i64,
    pub rule_id: i64,
    pub rule_name: String,
    pub event_id: i64,
}

/// Enabled rules + the last time each fired (cooldown, console clock).
#[derive(Default)]
pub struct Engine {
    rules: RwLock<Vec<Compiled>>,
    last: Mutex<HashMap<i64, i64>>,
}

impl Engine {
    pub fn set_rules(&self, rules: Vec<Compiled>) {
        *self.rules.write().unwrap_or_else(PoisonError::into_inner) = rules;
    }

    pub fn set_last(&self, last: HashMap<i64, i64>) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = last;
    }

    /// Rules in order; within one batch a rule's cooldown already counts its earlier hits.
    pub fn evaluate(&self, rows: &[(i64, Row)], render: &dyn Fn(i64, &Row) -> String) -> Vec<Fire> {
        let rules = self.rules.read().unwrap_or_else(PoisonError::into_inner);
        if rules.is_empty() {
            return Vec::new();
        }
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out = Vec::new();
        for (id, r) in rows {
            if r.rx_ts - r.plc_ts > BACKLOG_MS {
                continue;
            }
            let mut cache: Option<String> = None;
            let mut text = || cache.get_or_insert_with(|| render(*id, r).to_lowercase()).clone();
            for rule in rules.iter() {
                if !rule.matches(r, &mut text) {
                    continue;
                }
                if last.get(&rule.id).is_some_and(|t| r.rx_ts - t < rule.cooldown_ms) {
                    continue;
                }
                last.insert(rule.id, r.rx_ts);
                out.push(Fire { ts: r.rx_ts, rule_id: rule.id, rule_name: rule.name.clone(), event_id: *id });
            }
        }
        out
    }
}

// ---------------------------------------------------------------- store

fn rule_of(r: &rusqlite::Row) -> rusqlite::Result<Rule> {
    let m: String = r.get(3)?;
    Ok(Rule { id: r.get(0)?, name: r.get(1)?, enabled: r.get::<_, i64>(2)? != 0, m: serde_json::from_str(&m).unwrap_or_default(), cooldown_s: r.get(4)?, updated_at: r.get(5)? })
}

pub fn rules(store: &Store) -> rusqlite::Result<Vec<Rule>> {
    store.db().with(|c| {
        let mut st = c.prepare("SELECT id, name, enabled, match_json, cooldown_s, updated_at FROM alert_rules ORDER BY id")?;
        let it = st.query_map([], rule_of)?;
        it.collect()
    })
}

/// Inserts (`id` = 0) or replaces; returns the stored rule.
pub fn save_rule(store: &Store, r: &Rule) -> rusqlite::Result<Option<Rule>> {
    let m = serde_json::to_string(&r.m).unwrap_or_else(|_| "{}".into());
    let now = crate::util::now_str();
    store.db().with(|c| {
        let id = if r.id == 0 {
            c.execute("INSERT INTO alert_rules (name, enabled, match_json, cooldown_s, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![r.name.trim(), r.enabled, m, r.cooldown_s, now])?;
            c.last_insert_rowid()
        } else {
            let n = c.execute(
                "UPDATE alert_rules SET name = ?2, enabled = ?3, match_json = ?4, cooldown_s = ?5, updated_at = ?6 WHERE id = ?1",
                rusqlite::params![r.id, r.name.trim(), r.enabled, m, r.cooldown_s, now],
            )?;
            if n == 0 {
                return Ok(None);
            }
            r.id
        };
        let mut st = c.prepare("SELECT id, name, enabled, match_json, cooldown_s, updated_at FROM alert_rules WHERE id = ?1")?;
        st.query_row([id], rule_of).map(Some)
    })
}

pub fn delete_rule(store: &Store, id: i64) -> rusqlite::Result<bool> {
    store.db().with(|c| Ok(c.execute("DELETE FROM alert_rules WHERE id = ?1", [id])? > 0))
}

/// Last alert time per rule — the cooldown survives a console restart.
pub fn last_fired(store: &Store) -> rusqlite::Result<HashMap<i64, i64>> {
    store.db().with(|c| {
        let mut st = c.prepare("SELECT rule_id, MAX(ts) FROM alerts GROUP BY rule_id")?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect()
    })
}

pub fn insert(store: &Store, fires: &[Fire]) -> rusqlite::Result<Vec<i64>> {
    store.db().with_mut(|c| {
        let tx = c.transaction()?;
        let mut ids = Vec::with_capacity(fires.len());
        {
            let mut st = tx.prepare_cached("INSERT INTO alerts (ts, rule_id, rule_name, event_id) VALUES (?1, ?2, ?3, ?4)")?;
            for f in fires {
                st.execute(rusqlite::params![f.ts, f.rule_id, f.rule_name, f.event_id])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    })
}

/// API alert: the event row is rendered, `None` once retention removed it.
#[derive(Clone, Debug, Serialize)]
pub struct AlertRec {
    pub id: i64,
    pub ts: String,
    pub ts_ms: i64,
    pub rule_id: i64,
    pub rule_name: String,
    pub acked: bool,
    pub event: Option<EventRow>,
}

/// SSE payloads (`alert` / `alert_ack`).
#[derive(Clone, Debug)]
pub enum AlertMsg {
    New(Box<AlertRec>),
    /// Acknowledged ids; empty = all.
    Ack(Vec<i64>),
}

pub fn list(store: &Store, texts: &Texts, unacked: bool, limit: usize) -> rusqlite::Result<Vec<AlertRec>> {
    let sql = format!(
        "SELECT a.id, a.ts, a.rule_id, a.rule_name, a.acked_at, e.id, e.plc, e.epoch, e.seq, e.plc_ts, e.rx_ts, e.cat, e.lvl, e.src, e.code, e.a, e.b, e.ctx, e.origin, e.detail FROM alerts a LEFT JOIN events e ON e.id = a.event_id{} ORDER BY a.id DESC LIMIT ?1",
        if unacked { " WHERE a.acked_at IS NULL" } else { "" }
    );
    store.db().with(|c| {
        let mut st = c.prepare(&sql)?;
        let it = st.query_map([limit as i64], |r| {
            let eid: Option<i64> = r.get(5)?;
            let event = match eid {
                Some(eid) => {
                    let origin: String = r.get(18)?;
                    let row = Row {
                        plc: r.get(6)?,
                        epoch: r.get(7)?,
                        seq: r.get(8)?,
                        plc_ts: r.get(9)?,
                        rx_ts: r.get(10)?,
                        cat: r.get(11)?,
                        lvl: r.get(12)?,
                        src: r.get(13)?,
                        code: r.get(14)?,
                        a: r.get(15)?,
                        b: r.get(16)?,
                        ctx: r.get(17)?,
                        origin: if origin == "console" { super::store::Origin::Console } else { super::store::Origin::Plc },
                        detail: r.get(19)?,
                    };
                    Some(texts.row(eid, &row))
                }
                None => None,
            };
            let ts: i64 = r.get(1)?;
            Ok(AlertRec { id: r.get(0)?, ts: texts.fmt_ms(ts), ts_ms: ts, rule_id: r.get(2)?, rule_name: r.get(3)?, acked: r.get::<_, Option<i64>>(4)?.is_some(), event })
        })?;
        it.collect()
    })
}

pub fn unacked_count(store: &Store) -> rusqlite::Result<i64> {
    store.db().with(|c| c.query_row("SELECT COUNT(*) FROM alerts WHERE acked_at IS NULL", [], |r| r.get(0)))
}

/// `None` = every unacknowledged alert. Returns how many changed.
pub fn ack(store: &Store, id: Option<i64>, at: i64) -> rusqlite::Result<usize> {
    store.db().with(|c| match id {
        Some(id) => c.execute("UPDATE alerts SET acked_at = ?2 WHERE id = ?1 AND acked_at IS NULL", [id, at]),
        None => c.execute("UPDATE alerts SET acked_at = ?1 WHERE acked_at IS NULL", [at]),
    })
}

// ---------------------------------------------------------------- routes

#[derive(Deserialize)]
struct RuleBody {
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(rename = "match", default)]
    m: RuleMatch,
    #[serde(default)]
    cooldown_s: u32,
}

fn yes() -> bool {
    true
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::Internal(e.to_string()))?
}

async fn rules_get(State(st): State<AppState>) -> ApiResult<Vec<Rule>> {
    let log = log(&st)?.clone();
    Ok(axum::Json(blocking(move || Ok(rules(&log.store)?)).await?))
}

async fn put_rule(st: AppState, id: i64, b: RuleBody) -> ApiResult<Rule> {
    let log = log(&st)?.clone();
    let rule = Rule { id, name: b.name, enabled: b.enabled, m: b.m, cooldown_s: b.cooldown_s.min(86_400), updated_at: String::new() };
    compile(&rule, &log.texts.catalog).map_err(ApiError::BadRequest)?;
    let saved = blocking(move || {
        let saved = save_rule(&log.store, &rule)?.ok_or_else(|| ApiError::NotFound(format!("rule {id}")))?;
        log.reload_rules();
        Ok(saved)
    })
    .await?;
    Ok(axum::Json(saved))
}

async fn rule_post(State(st): State<AppState>, axum::Json(b): axum::Json<RuleBody>) -> ApiResult<Rule> {
    put_rule(st, 0, b).await
}

async fn rule_put(State(st): State<AppState>, Path(id): Path<i64>, axum::Json(b): axum::Json<RuleBody>) -> ApiResult<Rule> {
    if id <= 0 {
        return Err(ApiError::BadRequest("id".into()));
    }
    put_rule(st, id, b).await
}

async fn rule_delete(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    blocking(move || {
        if !delete_rule(&log.store, id)? {
            return Err(ApiError::NotFound(format!("rule {id}")));
        }
        log.reload_rules();
        Ok(())
    })
    .await?;
    Ok(axum::Json(json!({ "deleted": id })))
}

#[derive(Deserialize, Default)]
struct AlertsQ {
    unacked: Option<String>,
    limit: Option<String>,
}

async fn alerts_get(State(st): State<AppState>, Query(q): Query<AlertsQ>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    let unacked = opt(&q.unacked).is_some_and(|v| v != "0" && v != "false");
    let limit = num::<usize>(&q.limit, "limit")?.unwrap_or(100).clamp(1, 1000);
    let (rows, n) = blocking(move || Ok((list(&log.store, &log.texts, unacked, limit)?, unacked_count(&log.store)?))).await?;
    Ok(axum::Json(json!({ "rows": rows, "unacked": n })))
}

async fn ack_one(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    let (n, left) = blocking(move || {
        let n = ack(&log.store, Some(id), now_ms())?;
        if n > 0 {
            let _ = log.alerts_tx.send(AlertMsg::Ack(vec![id]));
        }
        Ok((n, unacked_count(&log.store)?))
    })
    .await?;
    Ok(axum::Json(json!({ "acked": n, "unacked": left })))
}

async fn ack_all(State(st): State<AppState>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    let n = blocking(move || {
        let n = ack(&log.store, None, now_ms())?;
        if n > 0 {
            let _ = log.alerts_tx.send(AlertMsg::Ack(Vec::new()));
        }
        Ok(n)
    })
    .await?;
    Ok(axum::Json(json!({ "acked": n, "unacked": 0 })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/events/alerts", get(alerts_get))
        .route("/api/events/alerts/ack-all", post(ack_all))
        .route("/api/events/alerts/{id}/ack", post(ack_one))
        .route("/api/events/alerts/rules", get(rules_get).post(rule_post))
        .route("/api/events/alerts/rules/{id}", put(rule_put).delete(rule_delete))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::stats::tests::{raw, texts};

    fn rule(id: i64, m: RuleMatch, cooldown_s: u32) -> Rule {
        Rule { id, name: format!("r{id}"), enabled: true, m, cooldown_s, updated_at: String::new() }
    }

    #[test]
    fn rules_match_by_plc_cat_code_level_and_text() {
        let cat = crate::evtlog::catalog::embedded();
        let t = texts();
        let ems = compile(&rule(1, RuleMatch { codes: vec!["SAFE_EMS".into(), "SAFE_GRM_EMS".into(), "CMD_EMS".into()], ..Default::default() }, 0), &cat).unwrap();
        let err = compile(&rule(2, RuleMatch { min_lvl: Some("ERROR".into()), ..Default::default() }, 0), &cat).unwrap();
        let grip = compile(&rule(3, RuleMatch { plc: Some("gr2".into()), cat: Some("GRIP".into()), text_contains: Some("stoppos".into()), ..Default::default() }, 0), &cat).unwrap();
        let num = compile(&rule(4, RuleMatch { codes: vec!["704".into()], ..Default::default() }, 0), &cat).unwrap();
        let r_ems = raw("GRM", 1, 0, 9, 4, 905, 1, 1, 0, 0); // SAFE_GRM_EMS
        let r_grip = raw("GR2", 1, 0, 7, 3, 704, 5, 5984, 38, 0); // GRIP_ERROR (WARN)
        let text_of = |r: &Row| {
            let s = t.text(r).to_lowercase();
            move || s.clone()
        };
        assert!(ems.matches(&r_ems, &mut text_of(&r_ems)));
        assert!(!ems.matches(&r_grip, &mut text_of(&r_grip)));
        assert!(err.matches(&r_ems, &mut text_of(&r_ems)) && !err.matches(&r_grip, &mut text_of(&r_grip)));
        assert!(grip.matches(&r_grip, &mut text_of(&r_grip)), "plc is case-insensitive, text over the rendered text");
        let mut other = r_grip.clone();
        other.plc = "GR1".into();
        assert!(!grip.matches(&other, &mut text_of(&other)));
        assert!(num.matches(&r_grip, &mut text_of(&r_grip)), "a bare number matches the code in any category");
        let ems_on = compile(&rule(6, RuleMatch { codes: vec!["SAFE_GRM_EMS".into()], a_eq: Some(1), ..Default::default() }, 0), &cat).unwrap();
        let mut ems_off = r_ems.clone();
        ems_off.a = 0;
        assert!(ems_on.matches(&r_ems, &mut text_of(&r_ems)) && !ems_on.matches(&ems_off, &mut text_of(&ems_off)), "a_eq picks ON only");
        assert_eq!(grip.text.as_deref(), Some("stoppos"), "lower-cased once");
        // bad names are refused up front
        assert!(compile(&rule(5, RuleMatch { codes: vec!["NOPE".into()], ..Default::default() }, 0), &cat).is_err());
        assert!(compile(&rule(5, RuleMatch { min_lvl: Some("LOUD".into()), ..Default::default() }, 0), &cat).is_err());
        assert!(compile(&Rule { name: " ".into(), ..rule(5, RuleMatch::default(), 0) }, &cat).is_err());
    }

    #[test]
    fn cooldown_and_backlog() {
        let cat = crate::evtlog::catalog::embedded();
        let e = Engine::default();
        e.set_rules(vec![
            compile(&rule(1, RuleMatch { min_lvl: Some("ERROR".into()), ..Default::default() }, 10), &cat).unwrap(),
            compile(&rule(2, RuleMatch { codes: vec!["ALM_TO_FAULT".into()], ..Default::default() }, 0), &cat).unwrap(),
        ]);
        let at = |ts: i64, code: u32| {
            let mut r = raw("GR2", 1, ts, 6, 4, code, 1, 3118, 0, 0);
            r.rx_ts = ts + 100;
            r
        };
        let render = |_: i64, _: &Row| String::new();
        // first ERROR fires, the second 5 s later is inside the 10 s cooldown, the third is past it
        let rows = vec![(1, at(1_000, 601)), (2, at(6_000, 601)), (3, at(12_000, 604))];
        let f = e.evaluate(&rows, &render);
        assert_eq!(f.iter().map(|x| (x.rule_id, x.event_id)).collect::<Vec<_>>(), vec![(1, 1), (1, 3), (2, 3)]);
        // the cooldown carries over to the next batch
        assert!(e.evaluate(&[(4, at(13_000, 601))], &render).is_empty());
        // backlog: stored 11 min after it happened
        let mut old = at(100_000, 604);
        old.rx_ts = old.plc_ts + 11 * 60_000;
        assert!(e.evaluate(&[(5, old)], &render).is_empty());
        // cooldown restored from the store
        e.set_last([(2, 200_000)].into_iter().collect());
        assert_eq!(e.evaluate(&[(6, at(150_000, 604))], &render).len(), 1, "rule 2 has no cooldown");
    }

    #[test]
    fn store_crud_seeds_alerts_and_acks() {
        let t = texts();
        let s = Store::memory();
        let seeded = rules(&s).unwrap();
        assert_eq!(seeded.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["ERROR 레벨 전체", "EMS", "FAULT 전환"]);
        let cat = crate::evtlog::catalog::embedded();
        assert!(seeded.iter().all(|r| compile(r, &cat).is_ok()), "the seeded rules name real events");
        let mut r = rule(0, RuleMatch { cat: Some("GRIP".into()), ..Default::default() }, 30);
        let saved = save_rule(&s, &r).unwrap().unwrap();
        assert!(saved.id > 3);
        r.id = saved.id;
        r.enabled = false;
        assert!(!save_rule(&s, &r).unwrap().unwrap().enabled);
        assert!(save_rule(&s, &Rule { id: 999, ..r.clone() }).unwrap().is_none());
        assert!(delete_rule(&s, saved.id).unwrap());
        assert!(!delete_rule(&s, saved.id).unwrap());

        let done = s.write(vec![raw("GR2", 1, 5_000, 6, 4, 604, 1, 3118, 0, 0)], &[]).unwrap();
        let ids = insert(&s, &[Fire { ts: 5_100, rule_id: 3, rule_name: "FAULT 전환".into(), event_id: done[0].0 }, Fire { ts: 5_200, rule_id: 1, rule_name: "x".into(), event_id: 12345 }]).unwrap();
        assert_eq!(ids.len(), 2);
        let got = list(&s, &t, true, 10).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[0].event.is_none(), "a pruned event leaves the alert");
        assert_eq!(got[1].event.as_ref().map(|e| e.name.as_deref()), Some(Some("ALM_TO_FAULT")));
        assert_eq!(last_fired(&s).unwrap().get(&3), Some(&5_100));
        assert_eq!(ack(&s, Some(ids[0]), 9).unwrap(), 1);
        assert_eq!(ack(&s, Some(ids[0]), 9).unwrap(), 0, "already acknowledged");
        assert_eq!(unacked_count(&s).unwrap(), 1);
        assert_eq!(ack(&s, None, 10).unwrap(), 1);
        assert_eq!(unacked_count(&s).unwrap(), 0);
        assert_eq!(list(&s, &t, false, 10).unwrap().len(), 2);
    }
}
