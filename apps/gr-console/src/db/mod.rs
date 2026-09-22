//! SQLite (rusqlite, WAL) behind a single mutex; blocking work runs on the blocking pool.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("migrations/0001_init.sql")),
    ("0002_registry", include_str!("migrations/0002_registry.sql")),
    ("0003_ledger", include_str!("migrations/0003_ledger.sql")),
    ("0004_scenario", include_str!("migrations/0004_scenario.sql")),
    ("0005_stock", include_str!("migrations/0005_stock.sql")),
    // 0006…0011 을 하나로 합쳤다(배포 전 정리, 결정 2026-09-18). 0005 까지 올라간 DB 도 이 하나로 따라온다.
    ("0006_console_v2", include_str!("migrations/0006_console_v2.sql")),
    // 팔렛 설정을 품목(패턴)·로봇(드래그 방향)·스테이션(여부)으로 나눔(결정 2026-09-21).
    ("0007_pallet_by_item", include_str!("migrations/0007_pallet_by_item.sql")),
    // 측정 반영으로 바꾼 화물 규격 치수의 이력(되돌리기용, 2026-09-21).
    ("0008_item_dim_changes", include_str!("migrations/0008_item_dim_changes.sql")),
    // 설정 변경 이력(기본값 저장마다 이전/이후) — 2026-09-21
    ("0009_settings_history", include_str!("migrations/0009_settings_history.sql")),
    // 재고 스냅샷(전체 비우기 · Excel 가져오기 · 되돌리기 직전) — 되돌리기용, 2026-09-22
    ("0010_stock_snapshots", include_str!("migrations/0010_stock_snapshots.sql")),
];

/// `ALTER TABLE … ADD COLUMN …` 중 **이미 있는 열**을 주석으로 지운 사본.
///
/// SQLite 에는 `ADD COLUMN IF NOT EXISTS` 가 없다. 문제는 이번 주기의 개발용 DB 다: 0006…0011 을
/// 하나로 합치기 전에 만들어져서 `tire_codes.spec_json` 은 이미 들고 있는데 `schema_migrations` 에는
/// 합친 이름(`0006_console_v2`)이 없다. 그대로 돌리면 `duplicate column name: spec_json` 으로
/// **기동 자체가 막힌다**(새 DB 와 0005 에서 멈춘 DB 는 멀쩡하다).
///
/// 마이그레이션을 손대는 대신 여기서 한 문장만 건너뛴다 — 나머지 문장은 이미 `IF NOT EXISTS` 이거나
/// 여러 번 돌려도 같은 결과라(재키잉·`UPDATE`) 셋 다 같은 모양으로 끝난다.
///
/// 한 줄에 한 문장인 `ALTER` 만 본다(이 저장소의 규약). 여러 줄로 쓴 `ALTER` 는 그대로 나가고,
/// 그건 그대로 실패하는 편이 낫다 — 조용히 건너뛰면 열이 없는 DB 가 생긴다.
fn skip_existing_columns(conn: &Connection, sql: &str) -> rusqlite::Result<String> {
    let mut out = String::with_capacity(sql.len());
    for line in sql.lines() {
        if let Some((table, column)) = added_column(line) {
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2", (&table, &column), |r| r.get(0))?;
            if n > 0 {
                tracing::info!(table = %table, column = %column, "이미 있는 열 — ALTER 건너뜀");
                out.push_str("-- 건너뜀(열이 이미 있다): ");
                out.push_str(line.trim());
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// `ALTER TABLE <표> ADD [COLUMN] <열> …` 이면 (표, 열). 아니면 `None`.
fn added_column(line: &str) -> Option<(String, String)> {
    let mut it = line.split_whitespace();
    if !it.next()?.eq_ignore_ascii_case("ALTER") {
        return None;
    }
    if !it.next()?.eq_ignore_ascii_case("TABLE") {
        return None;
    }
    let table = it.next()?;
    if !it.next()?.eq_ignore_ascii_case("ADD") {
        return None;
    }
    let mut column = it.next()?;
    if column.eq_ignore_ascii_case("COLUMN") {
        column = it.next()?;
    }
    Some((unquote(table), unquote(column)))
}

/// 식별자에서 따옴표·대괄호·뒤따르는 `;` 를 걷어 낸다.
fn unquote(s: &str) -> String {
    s.trim_end_matches(';').trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']' || c == '\'').to_string()
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        // 옛 실행 파일이 새 DB 를 열면 조용히 오동작한다(0007 이 표를 지웠다 등) — 모르는 마이그레이션이면 멈춘다.
        db.refuse_newer_schema(MIGRATIONS)?;
        // 적용할 마이그레이션이 있는 **기존** DB 는 먼저 통째로 떠 둔다(data/backup) — 업그레이드 되돌리기용.
        let pending = db.pending(MIGRATIONS)?;
        if !pending.is_empty() && db.applied_count()? > 0 {
            let dir = path.parent().map(|p| p.join("backup")).unwrap_or_else(|| PathBuf::from("backup"));
            let stamp: String = crate::util::now_str().chars().filter(char::is_ascii_digit).take(14).collect();
            let dest = dir.join(format!("gr-console-{stamp}-before-{}.db", pending[0]));
            db.backup_into(&dest)?;
            tracing::info!(to = %dest.display(), pending = ?pending, "DB backed up before migration");
        }
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> anyhow::Result<Db> {
        Db::open_memory_upto(MIGRATIONS.len())
    }

    /// 앞에서 `n` 개의 마이그레이션만 적용한 메모리 DB — "그때의 DB" 를 흉내 내는 시험용.
    #[cfg(test)]
    pub fn open_memory_upto(n: usize) -> anyhow::Result<Db> {
        let conn = Connection::open_in_memory()?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.migrate_list(&MIGRATIONS[..n])?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        self.migrate_list(MIGRATIONS)
    }

    fn migrate_list(&self, list: &[(&str, &str)]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY, applied_at TEXT NOT NULL);")?;
        for (name, sql) in list {
            let done: bool = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE name = ?1", [name], |r| r.get::<_, i64>(0))? > 0;
            if !done {
                // 마이그레이션 하나 = 트랜잭션 하나(기록 포함). 중간에 죽으면 통째로 되돌아가 다음 기동에서 처음부터 —
                // 예전에는 0006 의 DROP 과 RENAME 사이에서 죽으면 유일한 사본을 다음 기동이 지웠다.
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(&skip_existing_columns(&tx, sql)?)?;
                tx.execute("INSERT INTO schema_migrations (name, applied_at) VALUES (?1, ?2)", (name, crate::util::now_str()))?;
                tx.commit()?;
                tracing::info!(migration = name, "applied");
            }
        }
        Ok(())
    }

    /// 이 실행 파일이 모르는 마이그레이션이 DB 에 있으면 오류 — 더 새 버전이 만든 DB 다.
    fn refuse_newer_schema(&self, known: &[(&str, &str)]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let has_table: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'", [], |r| r.get(0))?;
        if has_table == 0 {
            return Ok(());
        }
        let mut q = conn.prepare("SELECT name FROM schema_migrations ORDER BY name")?;
        let names: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let unknown: Vec<String> = names.into_iter().filter(|n| !known.iter().any(|(k, _)| k == n)).collect();
        if !unknown.is_empty() {
            anyhow::bail!("이 DB 는 더 새 버전의 gr-console 이 만든 것입니다(모르는 마이그레이션: {}). 새 실행 파일을 쓰거나, data/backup 의 이전 백업으로 되돌리세요.", unknown.join(", "));
        }
        Ok(())
    }

    /// 아직 적용 안 된 마이그레이션 이름(순서대로).
    fn pending(&self, list: &[(&'static str, &str)]) -> anyhow::Result<Vec<&'static str>> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let has_table: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'", [], |r| r.get(0))?;
        if has_table == 0 {
            return Ok(list.iter().map(|(n, _)| *n).collect());
        }
        let mut out = Vec::new();
        for (name, _) in list {
            let done: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE name = ?1", [name], |r| r.get(0))?;
            if done == 0 {
                out.push(*name);
            }
        }
        Ok(out)
    }

    fn applied_count(&self) -> anyhow::Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let has_table: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'", [], |r| r.get(0))?;
        if has_table == 0 {
            return Ok(0);
        }
        Ok(conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?)
    }

    /// 일관된 사본 한 파일(`VACUUM INTO`) — 실행 중에도 WAL 까지 합친 스냅샷이 된다. 대상이 있으면 오류.
    pub fn backup_into(&self, dest: &Path) -> anyhow::Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dest.exists() {
            anyhow::bail!("백업 대상이 이미 있습니다: {}", dest.display());
        }
        let p = dest.to_string_lossy().replace('\'', "''");
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute_batch(&format!("VACUUM INTO '{p}'"))?;
        Ok(())
    }

    /// WAL 을 본 DB 로 합치고 잘라 낸다 — 종료 절차의 마지막 DB 단계. `(busy, wal_pages, moved_pages)`.
    /// `busy != 0` 이면 다른 연결이 읽는 중이라 다 옮기지 못한 것(다음 실행에서 자동으로 이어진다).
    pub fn checkpoint(&self) -> rusqlite::Result<(i64, i64, i64)> {
        self.with(|c| c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))
    }

    /// Runs a closure with the connection (synchronously; keep it short).
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        f(&conn)
    }

    #[allow(dead_code)]
    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut conn)
    }

    /// Persistent counter (e.g. work_id / task_id allocation). Returns the new value.
    pub fn next_counter(&self, name: &str, start: i64) -> rusqlite::Result<i64> {
        self.with(|c| {
            c.execute("INSERT INTO counters(name, value) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET value = value + 1", (name, start))?;
            c.query_row("SELECT value FROM counters WHERE name = ?1", [name], |r| r.get(0))
        })
    }

    pub fn setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT value_json FROM settings WHERE key = ?1", [key], |r| r.get(0)).map(Some).or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.with(|c| c.execute("INSERT INTO settings(key, value_json) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json", (key, value)).map(|_| ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0005 까지만 올라간 DB 를 흉내 낸다 — 0006 이 합쳐진 뒤에도 그 DB 가 따라올 수 있어야 한다.
    const UPTO_0005: usize = 5;

    /// `sqlite_master` 의 표·인덱스 정의 전부(이름 순) — 두 DB 가 같은 모양인지 보는 지문.
    fn schema(db: &Db) -> Vec<(String, String, String)> {
        db.with(|c| {
            let mut q = c.prepare("SELECT type, name, COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
        })
        .unwrap()
    }

    fn applied(db: &Db) -> Vec<String> {
        db.with(|c| {
            let mut q = c.prepare("SELECT name FROM schema_migrations ORDER BY name")?;
            q.query_map([], |r| r.get(0))?.collect()
        })
        .unwrap()
    }

    /// 새 DB 는 합친 0006 과 그 뒤 0007 이 돈다.
    #[test]
    fn a_fresh_db_runs_the_single_v2_migration() {
        let db = Db::open_memory().unwrap();
        assert_eq!(
            applied(&db),
            vec![
                "0001_init",
                "0002_registry",
                "0003_ledger",
                "0004_scenario",
                "0005_stock",
                "0006_console_v2",
                "0007_pallet_by_item",
                "0008_item_dim_changes",
                "0009_settings_history",
                "0010_stock_snapshots"
            ]
        );
        // 합친 마이그레이션이 만든 것들이 다 있다
        let names: Vec<String> = schema(&db).into_iter().map(|(_, n, _)| n).collect();
        for t in ["pallet_item", "pallet_robot", "pallet_station", "pallet_flow", "pallet_pattern", "item_bead_samples", "meas_entries", "item_dim_changes", "stock_snapshots"] {
            assert!(names.iter().any(|n| n == t), "{t} 가 없다: {names:?}");
        }
        let spec_col: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM pragma_table_info('tire_codes') WHERE name = 'spec_json'", [], |r| r.get(0))).unwrap();
        assert_eq!(spec_col, 1);
    }

    /// 0005 에서 멈춘 DB 에 0006 을 얹으면 새 DB 와 **똑같은 모양**이 된다.
    #[test]
    fn a_db_stopped_at_0005_ends_up_identical_to_a_fresh_one() {
        let old = Db::open_memory_upto(UPTO_0005).unwrap();
        assert_eq!(applied(&old).len(), UPTO_0005);
        old.migrate().unwrap();
        assert_eq!(applied(&old), applied(&Db::open_memory().unwrap()));
        assert_eq!(schema(&old), schema(&Db::open_memory().unwrap()));
    }

    /// 합치기 전(0006…0011)에 만들어진 개발용 DB — **열과 표는 이미 있는데** `schema_migrations` 에는
    /// 합친 이름이 없다. 그대로 돌면 `duplicate column name: spec_json` 으로 기동이 막혔다(2026-09-18).
    #[test]
    fn a_db_that_already_has_the_v2_column_and_tables_still_migrates() {
        let db = Db::open_memory_upto(UPTO_0005).unwrap();
        // 그때의 DB 를 그대로 흉내 낸다: 0006 의 내용은 돌았지만 이름은 기록되지 않았다.
        let (name, sql) = MIGRATIONS[UPTO_0005];
        assert_eq!(name, "0006_console_v2");
        db.with(|c| c.execute_batch(sql)).unwrap();
        assert_eq!(applied(&db).len(), UPTO_0005, "이름은 아직 기록되지 않았다");

        db.migrate().unwrap();

        let fresh = Db::open_memory().unwrap();
        assert_eq!(applied(&db), applied(&fresh));
        assert_eq!(schema(&db), schema(&fresh), "다시 돌려도 새 DB 와 같은 모양이다");
        let spec_cols: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM pragma_table_info('tire_codes') WHERE name = 'spec_json'", [], |r| r.get(0))).unwrap();
        assert_eq!(spec_cols, 1, "열이 두 번 붙지 않는다");
    }

    /// 이미 있는 열만 건너뛴다 — 없는 열의 `ALTER` 는 그대로 나간다(조용히 건너뛰면 열이 없는 DB 가 생긴다).
    #[test]
    fn only_columns_that_already_exist_are_skipped() {
        let db = Db::open_memory().unwrap();
        let out = db
            .with(|c| {
                skip_existing_columns(
                    c,
                    "ALTER TABLE tire_codes ADD COLUMN spec_json TEXT NOT NULL DEFAULT '{}';\nALTER TABLE tire_codes ADD COLUMN brand_new TEXT;\nCREATE INDEX IF NOT EXISTS x ON tire_codes(code);\n",
                )
            })
            .unwrap();
        assert!(out.contains("-- 건너뜀(열이 이미 있다): ALTER TABLE tire_codes ADD COLUMN spec_json"), "{out}");
        assert!(out.contains("\nALTER TABLE tire_codes ADD COLUMN brand_new TEXT;"), "{out}");
        assert!(out.contains("CREATE INDEX IF NOT EXISTS x ON tire_codes(code);"), "{out}");
        assert_eq!(added_column("ALTER TABLE t ADD name TEXT"), Some(("t".to_string(), "name".to_string())), "COLUMN 은 생략할 수 있다");
        assert_eq!(added_column("CREATE TABLE t (a TEXT)"), None);
    }

    /// 재키잉은 행을 지키고, PLC 마다 1 부터 세는 Seq 가 더는 서로를 덮지 않는다.
    #[test]
    fn meas_entries_keep_their_rows_and_get_a_plc_key() {
        let db = Db::open_memory_upto(UPTO_0005).unwrap();
        db.with(|c| {
            c.execute("INSERT INTO meas_entries (seq, plc, ts, kind, status, code, entry_json) VALUES (1, 'GR1', 't1', 1, 0, 1001, '{}')", ())?;
            c.execute("INSERT INTO meas_entries (seq, plc, ts, kind, status, code, entry_json) VALUES (2, 'GR2', 't2', 1, 0, 1002, '{}')", ())
        })
        .unwrap();
        db.migrate().unwrap();
        let rows: Vec<(String, i64, String)> = db
            .with(|c| {
                let mut q = c.prepare("SELECT plc, seq, ts FROM meas_entries ORDER BY plc, seq")?;
                q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
            })
            .unwrap();
        assert_eq!(rows, vec![("GR1".to_string(), 1, "t1".to_string()), ("GR2".to_string(), 2, "t2".to_string())]);
        // 이제 GR2 도 Seq 1 을 가질 수 있다
        db.with(|c| c.execute("INSERT INTO meas_entries (plc, seq, ts, kind, status, code, entry_json) VALUES ('GR2', 1, 't3', 1, 0, 1003, '{}')", ())).unwrap();
        let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM meas_entries", [], |r| r.get(0))).unwrap();
        assert_eq!(n, 3);
    }

    /// 저장된 기본값 이전: 드래그 거리 0 → 150, 그립 기준 `bead` → `pick_bead`(기본값·발행된 작업 둘 다).
    #[test]
    fn stored_defaults_move_to_the_new_values() {
        let db = Db::open_memory_upto(UPTO_0005).unwrap();
        db.set_setting("defaults", r#"{"version":3,"grip_ref":"bead","base":{"drag_in_dist":0,"drag_out_dist":70}}"#).unwrap();
        db.with(|c| {
            for (id, grip) in [("t1", "\"bead\""), ("t2", "\"mid\""), ("t3", "null")] {
                c.execute(
                    "INSERT INTO tasks (id, seq, work_id, task_id, origin, plc, state, doc_json, created_at, updated_at) VALUES (?1, 1, 1, 1, 'ui', 'GR2', 'draft', ?2, '', '')",
                    (id, format!(r#"{{"request":{{"type":"PICK","grip_ref":{grip}}}}}"#)),
                )?;
            }
            Ok(())
        })
        .unwrap();
        db.migrate().unwrap();
        let d = db.setting("defaults").unwrap().unwrap();
        assert!(d.contains(r#""grip_ref":"pick_bead""#), "{d}");
        assert!(d.contains(r#""drag_in_dist":150"#), "{d}");
        assert!(d.contains(r#""drag_out_dist":70"#), "운전자가 넣은 값은 그대로 둔다: {d}");
        let grips = db
            .with(|c| {
                let mut q = c.prepare("SELECT id, json_extract(doc_json, '$.request.grip_ref') FROM tasks ORDER BY id")?;
                q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        assert_eq!(grips, vec![("t1".into(), Some("pick_bead".to_string())), ("t2".into(), Some("mid".into())), ("t3".into(), None)]);
    }

    /// 적용 전 백업 · 트랜잭션 · 다운그레이드 차단(포터블/업그레이드 안전).
    #[test]
    fn upgrade_backs_up_and_refuses_newer_schema() {
        let dir = std::env::temp_dir().join(format!("grc-db-{}", uuid::Uuid::new_v4()));
        let path = dir.join("gr-console.db");
        std::fs::create_dir_all(&dir).unwrap();
        {
            // "옛 버전" DB: 앞 5 개만
            let conn = Connection::open(&path).unwrap();
            let db = Db { conn: Arc::new(Mutex::new(conn)) };
            db.migrate_list(&MIGRATIONS[..5]).unwrap();
        }
        let db = Db::open(&path).unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir.join("backup")).unwrap().collect();
        assert_eq!(backups.len(), 1, "업그레이드 전 백업 한 개");
        drop(db);
        // 새 버전이 붙인 모르는 마이그레이션 → 이 실행 파일은 열지 않는다
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("INSERT INTO schema_migrations (name, applied_at) VALUES ('9999_future', 'x')", []).unwrap();
        }
        let err = Db::open(&path).err().expect("newer schema refused").to_string();
        assert!(err.contains("9999_future"), "{err}");
        // 이미 최신이면 백업을 또 만들지 않는다
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_into_writes_a_consistent_copy() {
        let dir = std::env::temp_dir().join(format!("grc-bk-{}", uuid::Uuid::new_v4()));
        let db = Db::open(&dir.join("a.db")).unwrap();
        db.set_setting("k", "v").unwrap();
        let dest = dir.join("copy").join("b.db");
        db.backup_into(&dest).unwrap();
        let copy = Db::open(&dest).unwrap();
        assert_eq!(copy.setting("k").unwrap().as_deref(), Some("v"));
        assert!(db.backup_into(&dest).is_err(), "덮어쓰지 않는다");
        drop((db, copy));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
