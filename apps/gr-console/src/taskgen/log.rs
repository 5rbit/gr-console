//! 판정 기록(`taskgen_log`)과 KPI — 재시작해도 남는다. 판정마다가 아니라 **일이 생긴 때만** 적는다
//! (생성 · 발행 · 끝남 · 중단 · 냉각 · 요청 상태 · Hand 정리 · 대기 경고).
//! `num` 은 종류마다 하나의 수: generated = 준비 → 생성까지 기다린 초.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

/// 30 일 · 20 만 줄보다 오래된 것은 지운다(`prune`).
const KEEP_DAYS: i64 = 30;
const KEEP_ROWS: i64 = 200_000;

pub fn write(db: &Db, kind: &str, key: &str, robot: Option<u8>, detail: &str, num: Option<f64>) {
    let r = db.with(|c| c.execute("INSERT INTO taskgen_log (at, kind, key, robot, detail, num) VALUES (?1,?2,?3,?4,?5,?6)", (now_str(), kind, key, robot, detail, num)));
    if let Err(e) = r {
        tracing::warn!(%e, kind, key, "taskgen log write failed");
    }
}

/// `now_str` 과 같은 모양(로컬 오프셋 RFC 3339)의 과거 시각 — 기록 시각과 문자열로 비교한다.
fn ago(d: time::Duration) -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    (now - d).format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

pub fn prune(db: &Db) {
    let cutoff = ago(time::Duration::days(KEEP_DAYS));
    let _ = db.with(|c| {
        c.execute("DELETE FROM taskgen_log WHERE at < ?1", [&cutoff])?;
        c.execute("DELETE FROM taskgen_log WHERE id <= (SELECT MAX(id) FROM taskgen_log) - ?1", [KEEP_ROWS])
    });
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LogRow {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub key: String,
    pub robot: Option<u8>,
    pub detail: String,
}

pub fn list(db: &Db, limit: u32, kind: Option<&str>) -> Result<Vec<LogRow>, ApiError> {
    let limit = limit.clamp(1, 2000);
    Ok(db.with(|c| {
        let row = |r: &rusqlite::Row| Ok(LogRow { id: r.get(0)?, at: r.get(1)?, kind: r.get(2)?, key: r.get(3)?, robot: r.get(4)?, detail: r.get(5)? });
        match kind {
            Some(k) => {
                let mut st = c.prepare(&format!("SELECT id, at, kind, key, robot, detail FROM taskgen_log WHERE kind = ?1 ORDER BY id DESC LIMIT {limit}"))?;
                st.query_map([k], row)?.collect::<Result<Vec<_>, _>>()
            }
            None => {
                let mut st = c.prepare(&format!("SELECT id, at, kind, key, robot, detail FROM taskgen_log ORDER BY id DESC LIMIT {limit}"))?;
                st.query_map([], row)?.collect::<Result<Vec<_>, _>>()
            }
        }
    })?)
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct StationKpi {
    pub id: u16,
    pub generated: u32,
    pub wait_avg_s: Option<f64>,
    pub wait_max_s: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct RobotKpi {
    pub id: u8,
    pub name: String,
    pub generated: u32,
    pub completed: u32,
    pub aborted: u32,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct ReqKpi {
    pub done: u32,
    pub open: u32,
    pub failed: u32,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Kpi {
    pub hours: u32,
    pub generated: u32,
    pub completed: u32,
    pub aborted: u32,
    pub cycle_avg_s: Option<f64>,
    pub cycle_p90_s: Option<f64>,
    pub stations: Vec<StationKpi>,
    pub robots: Vec<RobotKpi>,
    pub requests: ReqKpi,
}

/// 기록 한 줄에서 스테이션 id — 키 `…@2101` 또는 `…-2101`(파생 규칙 id).
pub fn station_of_key(key: &str) -> Option<u16> {
    let tail = key.rsplit(['@', '-', ':']).next()?;
    tail.parse::<u16>().ok().filter(|id| (2001..=2999).contains(id))
}

fn secs_between(a: &str, b: &str) -> Option<f64> {
    let p = |s: &str| time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok();
    Some((p(b)? - p(a)?).as_seconds_f64()).filter(|s| *s >= 0.0)
}

pub fn percentile(v: &mut [f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((v.len() as f64 - 1.0) * q).round() as usize;
    Some(v[i.min(v.len() - 1)])
}

pub fn kpi(db: &Db, hours: u32, robot_names: &[(u8, String)]) -> Result<Kpi, ApiError> {
    let hours = hours.clamp(1, 24 * 30);
    let since = ago(time::Duration::hours(i64::from(hours)));
    let rows: Vec<(String, String, Option<u8>, Option<f64>)> = db.with(|c| {
        let mut st = c.prepare("SELECT kind, key, robot, num FROM taskgen_log WHERE at >= ?1 AND kind IN ('generated','completed','aborted')")?;
        st.query_map([&since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>, _>>()
    })?;
    let mut k = Kpi { hours, ..Default::default() };
    let mut st: BTreeMap<u16, (u32, Vec<f64>)> = BTreeMap::new();
    let mut rb: BTreeMap<u8, RobotKpi> = robot_names.iter().map(|(id, n)| (*id, RobotKpi { id: *id, name: n.clone(), ..Default::default() })).collect();
    for (kind, key, robot, num) in rows {
        let r = robot.map(|id| rb.entry(id).or_insert_with(|| RobotKpi { id, name: format!("로봇 {id}"), ..Default::default() }));
        match kind.as_str() {
            "generated" => {
                k.generated += 1;
                if let Some(r) = r {
                    r.generated += 1;
                }
                if let Some(s) = station_of_key(&key) {
                    let e = st.entry(s).or_default();
                    e.0 += 1;
                    if let Some(w) = num {
                        e.1.push(w);
                    }
                }
            }
            "completed" => {
                k.completed += 1;
                if let Some(r) = r {
                    r.completed += 1;
                }
            }
            _ => {
                k.aborted += 1;
                if let Some(r) = r {
                    r.aborted += 1;
                }
            }
        }
    }
    k.stations = st
        .into_iter()
        .map(|(id, (n, mut w))| StationKpi { id, generated: n, wait_avg_s: (!w.is_empty()).then(|| w.iter().sum::<f64>() / w.len() as f64), wait_max_s: percentile(&mut w, 1.0) })
        .collect();
    k.robots = rb.into_values().collect();
    // 사이클 = 이송 지시 열림 → 끝남(done 만).
    let orders: Vec<(String, Option<String>)> = db.with(|c| {
        let mut s = c.prepare("SELECT created_at, json_extract(doc_json, '$.ended_at') FROM transfer_orders WHERE state = 'done' AND created_at >= ?1")?;
        s.query_map([&since], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()
    })?;
    let mut cyc: Vec<f64> = orders.iter().filter_map(|(a, b)| secs_between(a, b.as_deref()?)).collect();
    k.cycle_avg_s = (!cyc.is_empty()).then(|| cyc.iter().sum::<f64>() / cyc.len() as f64);
    k.cycle_p90_s = percentile(&mut cyc, 0.9);
    let (done, open, failed): (u32, u32, u32) = db.with(|c| {
        c.query_row(
            "SELECT COALESCE(SUM(state = 'done' AND updated_at >= ?1), 0), COALESCE(SUM(state IN ('open','active')), 0), COALESCE(SUM(state = 'failed' AND updated_at >= ?1), 0) FROM transfer_requests",
            [&since],
            |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u32, r.get::<_, i64>(2)? as u32)),
        )
    })?;
    k.requests = ReqKpi { done, open, failed };
    Ok(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_ids_come_out_of_keys() {
        assert_eq!(station_of_key("in-2101"), Some(2101));
        assert_eq!(station_of_key("req:RQ-260928-0001@2102"), Some(2102));
        assert_eq!(station_of_key("rule-3"), None);
        assert_eq!(station_of_key("consolidate-401"), None, "cells are not stations");
    }

    #[test]
    fn kpi_counts_what_was_written() {
        let db = Db::open_memory().unwrap();
        write(&db, "generated", "in-2101", Some(2), "a", Some(4.0));
        write(&db, "generated", "in-2101", Some(2), "b", Some(8.0));
        write(&db, "generated", "out-2103", Some(1), "c", None);
        write(&db, "completed", "in-2101", Some(2), "d", None);
        write(&db, "aborted", "out-2103", Some(1), "e", None);
        let k = kpi(&db, 24, &[(1, "GR1".into()), (2, "GR2".into())]).unwrap();
        assert_eq!((k.generated, k.completed, k.aborted), (3, 1, 1));
        let s = k.stations.iter().find(|s| s.id == 2101).unwrap();
        assert_eq!((s.generated, s.wait_avg_s, s.wait_max_s), (2, Some(6.0), Some(8.0)));
        assert_eq!(k.robots.iter().map(|r| (r.id, r.generated, r.completed, r.aborted)).collect::<Vec<_>>(), vec![(1, 1, 0, 1), (2, 2, 1, 0)]);
        assert_eq!(list(&db, 10, Some("generated")).unwrap().len(), 3);
        assert_eq!(list(&db, 2, None).unwrap()[0].kind, "aborted", "newest first");
        prune(&db);
        assert_eq!(list(&db, 10, None).unwrap().len(), 5, "fresh rows stay");
    }

    #[test]
    fn percentiles() {
        assert_eq!(percentile(&mut [], 0.9), None);
        assert_eq!(percentile(&mut [3.0, 1.0, 2.0], 1.0), Some(3.0));
        assert_eq!(percentile(&mut [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0], 0.9), Some(9.0));
    }
}
