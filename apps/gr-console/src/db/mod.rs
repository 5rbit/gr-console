//! SQLite (rusqlite, WAL) behind a single mutex; blocking work runs on the blocking pool.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("migrations/0001_init.sql")),
    ("0002_registry", include_str!("migrations/0002_registry.sql")),
    ("0003_ledger", include_str!("migrations/0003_ledger.sql")),
    ("0004_scenario", include_str!("migrations/0004_scenario.sql")),
];

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
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> anyhow::Result<Db> {
        let conn = Connection::open_in_memory()?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY, applied_at TEXT NOT NULL);")?;
        for (name, sql) in MIGRATIONS {
            let done: bool = conn.query_row("SELECT COUNT(*) FROM schema_migrations WHERE name = ?1", [name], |r| r.get::<_, i64>(0))? > 0;
            if !done {
                conn.execute_batch(sql)?;
                conn.execute("INSERT INTO schema_migrations (name, applied_at) VALUES (?1, ?2)", (name, crate::util::now_str()))?;
                tracing::info!(migration = name, "applied");
            }
        }
        Ok(())
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
