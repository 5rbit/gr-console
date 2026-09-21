//! 편집 가능한 팔렛 패턴 저장소(`pallet_flow` · `pallet_pattern`, 마이그레이션 `0009_pallet_pattern`).
//!
//! - 첫 기동 때 내장 사양서 R4 로 채운다(`ensure_seeded`, settings `pallet.seed`). 그 뒤 운전자가 흐름을 다 지워도
//!   다시 채우지 않는다 — 되돌리기는 흐름별 초기화(`reset`)나 가져오기로 한다.
//! - 모든 변경은 한 트랜잭션: 읽기(`Snapshot`) → 순수 변경·검증(`edit.rs`) → 쓰기. 검증 실패면 아무것도 안 쓴다.
//! - 흐름 이름 바꾸기는 그 흐름을 쓰는 스테이션 프로파일(`pallet_profile.flow`)도 같이 바꾼다.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::edit::{self, Change, CreateFlow, FlowPatch, ImportReport, PatternBody, Snapshot};
use super::{DragKind, Drawn, Flow, Library, ScreenAxes, Spec, SpecPattern, SpecSlot, seed};
use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

pub const SEED_KEY: &str = "pallet.seed";

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct FlowMeta {
    source_slide: u8,
    also_slides: Vec<u8>,
    reference: bool,
    robot: String,
    zone: String,
    screen_axes: ScreenAxes,
    notes: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct PatternMeta {
    order_text: String,
    drawn: Drawn,
    notes: Vec<String>,
}

fn conv(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

type FlowRow = (String, String, String, String, String, String, Option<String>, String);
type PatternRow = (String, i64, f64, f64, String, String, String, String);

fn load(c: &Connection) -> rusqlite::Result<Snapshot> {
    let mut st = c.prepare("SELECT id, name, drag_kind, source, note, updated_at, based_on, meta_json FROM pallet_flow ORDER BY sort, id")?;
    let rows: Vec<FlowRow> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?.collect::<Result<_, _>>()?;
    let mut flows = Vec::with_capacity(rows.len());
    for (id, name, kind, source, note, updated_at, based_on, meta) in rows {
        let m: FlowMeta = serde_json::from_str(&meta).map_err(conv)?;
        flows.push(Flow {
            id,
            name,
            source_slide: m.source_slide,
            also_slides: m.also_slides,
            drag_kind: if kind == "out" { DragKind::Out } else { DragKind::In },
            reference: m.reference,
            robot: m.robot,
            zone: m.zone,
            screen_axes: m.screen_axes,
            notes: m.notes,
            patterns: vec![],
            source,
            based_on,
            note,
            updated_at,
        });
    }
    let mut st = c.prepare("SELECT flow_id, pattern, od_min, od_max, slots_json, note, updated_at, meta_json FROM pallet_pattern ORDER BY flow_id, pattern")?;
    let rows: Vec<PatternRow> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?.collect::<Result<_, _>>()?;
    for (flow_id, pattern, od_min, od_max, slots, note, updated_at, meta) in rows {
        let Some(f) = flows.iter_mut().find(|f| f.id == flow_id) else { continue };
        let slots: Vec<SpecSlot> = serde_json::from_str(&slots).map_err(conv)?;
        let m: PatternMeta = serde_json::from_str(&meta).map_err(conv)?;
        f.patterns.push(SpecPattern {
            pattern: pattern.clamp(0, 255) as u8,
            od_min: od_min as f32,
            od_max: od_max as f32,
            order_text: m.order_text,
            drawn: m.drawn,
            slots,
            notes: m.notes,
            note,
            updated_at,
        });
    }
    let mut usage: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    let mut st = c.prepare("SELECT station_id, flow FROM pallet_profile ORDER BY station_id")?;
    let rows: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    for (sid, flow) in rows {
        usage.entry(flow.to_ascii_uppercase()).or_default().push(sid as u16);
    }
    Ok(Snapshot { lib: Library { flows }, usage })
}

fn write_flow(c: &Connection, f: &Flow) -> rusqlite::Result<()> {
    let meta = FlowMeta {
        source_slide: f.source_slide,
        also_slides: f.also_slides.clone(),
        reference: f.reference,
        robot: f.robot.clone(),
        zone: f.zone.clone(),
        screen_axes: f.screen_axes.clone(),
        notes: f.notes.clone(),
    };
    c.execute(
        "INSERT INTO pallet_flow (id, name, drag_kind, source, note, updated_at, based_on, meta_json, sort) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, COALESCE((SELECT MAX(sort) + 1 FROM pallet_flow), 0)) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, drag_kind = excluded.drag_kind, source = excluded.source, note = excluded.note, \
         updated_at = excluded.updated_at, based_on = excluded.based_on, meta_json = excluded.meta_json",
        params![f.id, f.name, f.drag_kind.as_str(), f.source, f.note, f.updated_at, f.based_on, serde_json::to_string(&meta).map_err(conv)?],
    )?;
    c.execute("DELETE FROM pallet_pattern WHERE flow_id = ?1", [&f.id])?;
    for p in &f.patterns {
        let meta = PatternMeta { order_text: p.order_text.clone(), drawn: p.drawn.clone(), notes: p.notes.clone() };
        c.execute(
            "INSERT INTO pallet_pattern (flow_id, pattern, od_min, od_max, slots_json, note, updated_at, meta_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![f.id, p.pattern, p.od_min as f64, p.od_max as f64, serde_json::to_string(&p.slots).map_err(conv)?, p.note, p.updated_at, serde_json::to_string(&meta).map_err(conv)?],
        )?;
    }
    Ok(())
}

fn apply(c: &Connection, ch: &Change) -> rusqlite::Result<()> {
    if let Some((old, new)) = &ch.rename {
        c.execute("UPDATE pallet_flow SET id = ?2 WHERE id = ?1", params![old, new])?;
        c.execute("UPDATE pallet_pattern SET flow_id = ?2 WHERE flow_id = ?1", params![old, new])?;
        c.execute("UPDATE pallet_profile SET flow = ?2 WHERE upper(flow) = upper(?1)", params![old, new])?;
    }
    for id in &ch.delete {
        c.execute("DELETE FROM pallet_pattern WHERE flow_id = ?1", [id])?;
        c.execute("DELETE FROM pallet_flow WHERE id = ?1", [id])?;
    }
    for f in &ch.write {
        write_flow(c, f)?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct PalletStore {
    db: Db,
}

impl PalletStore {
    pub fn new(db: Db) -> PalletStore {
        PalletStore { db }
    }

    /// 한 번도 채운 적이 없으면 내장 사양서로 채운다(이미 있는 id 는 건드리지 않는다). 채웠으면 `true`.
    pub fn ensure_seeded(&self) -> Result<bool, ApiError> {
        let now = now_str();
        Ok(self.db.with_mut(|c| {
            let tx = c.transaction()?;
            let done: Option<String> = tx.query_row("SELECT value_json FROM settings WHERE key = ?1", [SEED_KEY], |r| r.get(0)).optional()?;
            if done.is_some() {
                return Ok(false);
            }
            for mut f in Library::from_seed(seed()).flows {
                let exists = tx.query_row("SELECT COUNT(*) FROM pallet_flow WHERE id = ?1", [&f.id], |r| r.get::<_, i64>(0))? > 0;
                if exists {
                    continue;
                }
                f.updated_at = now.clone();
                f.patterns.iter_mut().for_each(|p| p.updated_at = now.clone());
                write_flow(&tx, &f)?;
            }
            let mark = serde_json::to_string(&json!({ "version": seed().version, "at": now })).map_err(conv)?;
            tx.execute("INSERT INTO settings(key, value_json) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json", params![SEED_KEY, mark])?;
            tx.commit()?;
            Ok(true)
        })?)
    }

    pub fn snapshot(&self) -> Result<Snapshot, ApiError> {
        Ok(self.db.with(load)?)
    }

    /// 생성기·계획·compose 가 읽는 현재 패턴.
    pub fn library(&self) -> Result<Library, ApiError> {
        Ok(self.snapshot()?.lib)
    }

    /// 읽기-변경-쓰기 한 트랜잭션. 닫개가 `Err` 면 아무것도 쓰지 않는다.
    fn mutate<T>(&self, f: impl FnOnce(&Snapshot, &str) -> Result<(Change, T), ApiError>) -> Result<T, ApiError> {
        let now = now_str();
        self.db.with_mut(|c| {
            let tx = c.transaction()?;
            let snap = load(&tx)?;
            match f(&snap, &now) {
                Ok((ch, v)) => {
                    apply(&tx, &ch)?;
                    tx.commit()?;
                    Ok(Ok(v))
                }
                Err(e) => Ok(Err(e)),
            }
        })?
    }

    pub fn create(&self, req: &CreateFlow) -> Result<(Flow, Vec<String>), ApiError> {
        self.mutate(|s, now| {
            let (f, w) = edit::create_flow(s, req, now)?;
            Ok((Change { write: vec![f.clone()], ..Default::default() }, (f, w)))
        })
    }

    pub fn update_flow(&self, id: &str, patch: &FlowPatch) -> Result<(Flow, Vec<String>), ApiError> {
        self.mutate(|s, now| {
            let (f, ch, w) = edit::update_flow(s, id, patch, now)?;
            Ok((ch, (f, w)))
        })
    }

    pub fn delete_flow(&self, id: &str) -> Result<String, ApiError> {
        self.mutate(|s, _| {
            let ch = edit::delete_flow(s, id)?;
            let removed = ch.delete.first().cloned().unwrap_or_default();
            Ok((ch, removed))
        })
    }

    pub fn upsert_pattern(&self, id: &str, no: u8, body: &PatternBody) -> Result<(Flow, SpecPattern, Vec<String>), ApiError> {
        self.mutate(|s, now| {
            let (f, p, w) = edit::upsert_pattern(s, id, no, body, now)?;
            Ok((Change { write: vec![f.clone()], ..Default::default() }, (f, p, w)))
        })
    }

    pub fn delete_pattern(&self, id: &str, no: u8) -> Result<Flow, ApiError> {
        self.mutate(|s, now| {
            let f = edit::finish_delete_pattern(edit::delete_pattern(s, id, no)?, now)?;
            Ok((Change { write: vec![f.clone()], ..Default::default() }, f))
        })
    }

    pub fn reset(&self, id: &str, pattern: Option<u8>) -> Result<Flow, ApiError> {
        self.mutate(|s, now| {
            let f = edit::reset_flow(s, id, pattern, now)?;
            Ok((Change { write: vec![f.clone()], ..Default::default() }, f))
        })
    }

    /// 흐름 id + 패턴 번호로 병합. `dry_run` 이면 보고서만(쓰지 않음), 아니면 오류가 하나라도 있을 때 400.
    pub fn import(&self, doc: &Spec, dry_run: bool) -> Result<ImportReport, ApiError> {
        self.mutate(|s, now| {
            let (writes, mut report) = edit::merge_import(&s.lib, doc, now);
            report.dry_run = dry_run;
            if dry_run {
                return Ok((Change::default(), report));
            }
            if !report.ok {
                return Err(ApiError::BadRequest(format!("가져오기 거부 — {}", report.all_errors().join(" · "))));
            }
            Ok((Change { write: writes, ..Default::default() }, report))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pallet::edit::diff_flow;
    use crate::pallet::profiles::{Profile, Profiles};
    use crate::pallet::{GenInput, Transform, generate};

    fn seeded() -> (Db, PalletStore) {
        let db = Db::open_memory().unwrap();
        let s = PalletStore::new(db.clone());
        assert!(s.ensure_seeded().unwrap());
        (db, s)
    }

    fn body(p: &SpecPattern) -> PatternBody {
        PatternBody { pattern: None, od_min: p.od_min, od_max: p.od_max, slots: p.slots.clone(), note: p.note.clone() }
    }

    #[test]
    fn seed_on_empty_db_once() {
        let (db, s) = seeded();
        let lib = s.library().unwrap();
        let sp = seed();
        assert_eq!(lib.flows.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), sp.flows.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), "seed order kept");
        for (a, b) in lib.flows.iter().zip(&sp.flows) {
            assert_eq!((a.source.as_str(), a.based_on.as_deref()), ("spec_r4", Some(b.id.as_str())));
            assert!(!a.updated_at.is_empty());
            assert_eq!((a.reference, &a.screen_axes, &a.notes, &a.zone), (b.reference, &b.screen_axes, &b.notes, &b.zone));
            assert_eq!(diff_flow(a).status, "spec", "{} round-trips through sqlite", a.id);
            for (p, q) in a.patterns.iter().zip(&b.patterns) {
                assert_eq!((&p.order_text, &p.drawn, &p.notes, &p.slots), (&q.order_text, &q.drawn, &q.notes, &q.slots));
            }
        }
        assert!(!s.ensure_seeded().unwrap(), "second start does not seed again");
        // deleting everything does not re-seed
        for f in &lib.flows {
            s.delete_flow(&f.id).unwrap();
        }
        assert!(!s.ensure_seeded().unwrap());
        assert!(s.library().unwrap().flows.is_empty());
        assert!(db.setting(SEED_KEY).unwrap().unwrap().contains("R4-251213"));
    }

    #[test]
    fn crud_validation_through_store() {
        let (_, s) = seeded();
        let (c, _) = s.create(&CreateFlow { id: "HP_IN_SITE".into(), copy_from: Some("HP_IN".into()), ..Default::default() }).unwrap();
        assert_eq!(c.based_on.as_deref(), Some("HP_IN"));
        assert!(matches!(s.create(&CreateFlow { id: "HP_IN_SITE".into(), copy_from: Some("HP_IN".into()), ..Default::default() }), Err(ApiError::Conflict(_))));
        let p4 = c.pattern(4).unwrap().clone();
        let mut b = body(&p4);
        b.slots[0].seq = 2;
        let before = s.library().unwrap();
        assert!(matches!(s.upsert_pattern("HP_IN_SITE", 4, &b), Err(ApiError::BadRequest(m)) if m.contains("Seq")));
        assert_eq!(s.library().unwrap(), before, "failed validation writes nothing");
        // new pattern 1 in an OD gap-free place fails on overlap, then succeeds above the top
        let one = PatternBody { pattern: None, od_min: 900.0, od_max: 1000.0, slots: vec![SpecSlot { slot: "C#1".into(), seq: 1, u: [0.0, 0.0], drag_dir: 0 }], note: String::new() };
        assert!(matches!(s.upsert_pattern("HP_IN_SITE", 1, &one), Err(ApiError::BadRequest(m)) if m.contains("겹칩니다")));
        let (f, p, _) = s.upsert_pattern("HP_IN_SITE", 1, &PatternBody { od_min: 938.0, ..one }).unwrap();
        assert_eq!((p.pattern, f.patterns.len(), f.patterns[0].pattern), (1, 8, 1));
        // renumber 1 → 10, then delete
        let (f, _, _) = s.upsert_pattern("HP_IN_SITE", 1, &PatternBody { pattern: Some(10), ..body(&p) }).unwrap();
        assert!(f.pattern(1).is_none() && f.pattern(10).is_some());
        assert_eq!(s.delete_pattern("HP_IN_SITE", 10).unwrap().patterns.len(), 7);
        assert!(matches!(s.delete_pattern("HP_IN_SITE", 10), Err(ApiError::NotFound(_))));
        let lib = s.library().unwrap();
        assert_eq!(lib.flow("HP_IN_SITE").unwrap().patterns.len(), 7);
        assert_eq!(lib.flows.last().unwrap().id, "HP_IN_SITE", "new flows go last");
    }

    #[test]
    fn generator_reads_edited_pattern() {
        let (_, s) = seeded();
        let p3 = s.library().unwrap().flow("HP_IN").unwrap().pattern(3).unwrap().clone();
        let mut b = body(&p3);
        b.slots.iter_mut().find(|x| x.slot == "C#2").unwrap().drag_dir = 5;
        s.upsert_pattern("HP_IN", 3, &b).unwrap();
        let lib = s.library().unwrap();
        let g = generate(&lib, &GenInput { flow: "HP_IN", pattern: None, od: 780.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).unwrap();
        let c2 = g.slots.iter().find(|x| x.slot == "C#2").unwrap();
        assert_eq!((g.pattern, c2.drag_dir, c2.drag_type), (3, 5, 0x10));
        assert!(!g.pattern_updated_at.is_empty());
        assert_eq!(g.pattern_updated_at, lib.flow("HP_IN").unwrap().pattern(3).unwrap().updated_at);
    }

    #[test]
    fn reset_and_diff_through_store() {
        let (_, s) = seeded();
        let p5 = s.library().unwrap().flow("OP_OUT").unwrap().pattern(5).unwrap().clone();
        let mut b = body(&p5);
        b.slots[0].u = [b.slots[0].u[0] + 0.25, b.slots[0].u[1]];
        s.upsert_pattern("OP_OUT", 5, &b).unwrap();
        s.delete_pattern("OP_OUT", 9).unwrap();
        let d = diff_flow(s.library().unwrap().flow("OP_OUT").unwrap());
        assert_eq!(d.status, "modified");
        let p = d.patterns.iter().find(|p| p.pattern == 5).unwrap();
        assert_eq!(p.slots.iter().filter(|x| x.status == "changed").count(), 1);
        assert_eq!(d.patterns.iter().find(|p| p.pattern == 9).unwrap().status, "removed");
        s.reset("OP_OUT", Some(5)).unwrap();
        let d = diff_flow(s.library().unwrap().flow("OP_OUT").unwrap());
        assert_eq!((d.status, d.patterns.iter().find(|p| p.pattern == 5).unwrap().status), ("modified", "same"));
        s.reset("OP_OUT", None).unwrap();
        assert_eq!(diff_flow(s.library().unwrap().flow("OP_OUT").unwrap()).status, "spec");
    }

    #[test]
    fn import_dry_run_then_merge() {
        let (_, s) = seeded();
        let mut doc = edit::to_doc(&s.library().unwrap(), "t");
        doc.flows.retain(|f| f.id == "HP_IN");
        doc.flows[0].patterns.retain(|p| p.pattern == 2);
        doc.flows[0].patterns[0].slots[1].drag_dir = 8;
        let before = s.library().unwrap();
        let r = s.import(&doc, true).unwrap();
        assert!(r.dry_run && r.ok && r.updated == 1);
        assert_eq!(s.library().unwrap(), before, "dry run writes nothing");
        let r = s.import(&doc, false).unwrap();
        assert!(!r.dry_run && r.ok);
        let lib = s.library().unwrap();
        assert_eq!(lib.flow("HP_IN").unwrap().pattern(2).unwrap().slots[1].drag_dir, 8);
        assert_eq!(lib.flow("HP_IN").unwrap().patterns.len(), 7);
        // invalid import is refused as a whole
        doc.flows[0].patterns[0].od_min = 700.0;
        let r = s.import(&doc, true).unwrap();
        assert!(!r.ok);
        assert!(matches!(s.import(&doc, false), Err(ApiError::BadRequest(m)) if m.contains("겹칩니다")));
        assert_eq!(s.library().unwrap(), lib);
    }

    #[test]
    fn delete_refused_while_profile_uses_flow_and_rename_follows() {
        let (db, s) = seeded();
        s.create(&CreateFlow { id: "HP_IN_SITE".into(), copy_from: Some("HP_IN".into()), ..Default::default() }).unwrap();
        let profiles = Profiles::new(db.clone());
        let lib = s.library().unwrap();
        profiles.upsert(Profile { station_id: 2021, flow: "hp_in_site".into(), enabled: true, ..Default::default() }, &lib).unwrap();
        let e = s.delete_flow("HP_IN_SITE").unwrap_err();
        assert!(matches!(&e, ApiError::Conflict(m) if m.contains("2021")), "{e}");
        s.update_flow("HP_IN_SITE", &FlowPatch { id: Some("SITE_A".into()), ..Default::default() }).unwrap();
        assert_eq!(profiles.get(2021).unwrap().unwrap().flow, "SITE_A");
        assert_eq!(s.snapshot().unwrap().used_by("site_a"), vec![2021]);
        profiles.remove(2021).unwrap();
        assert_eq!(s.delete_flow("SITE_A").unwrap(), "SITE_A");
        assert!(s.library().unwrap().flow("SITE_A").is_none());
    }
}
