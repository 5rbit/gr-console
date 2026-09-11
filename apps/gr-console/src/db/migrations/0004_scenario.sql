-- scenarios
CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL, doc_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS scenario_runs (id TEXT PRIMARY KEY, scenario_id TEXT NOT NULL, started_at TEXT NOT NULL, ended_at TEXT, status TEXT NOT NULL, doc_json TEXT NOT NULL);
