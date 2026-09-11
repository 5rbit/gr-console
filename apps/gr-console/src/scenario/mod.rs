//! Scenario manager (M3-C slice). M1 ships the storage shell and an idle runner so the API surface exists.

pub mod routes;

use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<Json>,
    pub repeat: u32,
    pub created_at: String,
    pub updated_at: String,
}
impl Default for Scenario {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), description: String::new(), steps: vec![], repeat: 1, created_at: now_str(), updated_at: now_str() }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RunState {
    pub run_id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub state: String,
    pub iteration: u32,
    pub total_iterations: Option<u32>,
    pub step_index: u32,
    pub step_count: u32,
    pub current_task_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub error: Option<String>,
    pub results: Vec<Json>,
}

pub struct Runner {
    db: Db,
    pub run: Mutex<RunState>,
    pub events: broadcast::Sender<RunState>,
}

impl Runner {
    pub fn new(db: Db) -> Arc<Runner> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(Runner { db, run: Mutex::new(RunState { state: "idle".into(), ..Default::default() }), events: tx })
    }

    pub fn current(&self) -> RunState {
        self.run.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn list(&self) -> Result<Vec<Scenario>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM scenarios ORDER BY updated_at DESC")?;
            let it = st.query_map([], |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<Scenario>, ApiError> {
        Ok(self.list()?.into_iter().find(|s| s.id == id))
    }

    pub fn save(&self, mut s: Scenario) -> Result<Scenario, ApiError> {
        if s.id.is_empty() {
            s.id = uuid::Uuid::new_v4().to_string();
            s.created_at = now_str();
        }
        s.updated_at = now_str();
        let doc = serde_json::to_string(&s)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO scenarios (id, name, doc_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name, doc_json=excluded.doc_json, updated_at=excluded.updated_at",
                (&s.id, &s.name, &doc, &s.created_at, &s.updated_at),
            )
        })?;
        Ok(s)
    }

    pub fn delete(&self, id: &str) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM scenarios WHERE id = ?1", [id]))? > 0)
    }
}
