-- registry (owned by the task-issue slice)
CREATE TABLE IF NOT EXISTS tire_codes (code INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT "", item_json TEXT NOT NULL, note TEXT NOT NULL DEFAULT "", updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cells (id INTEGER PRIMARY KEY, source TEXT NOT NULL, cell_json TEXT NOT NULL, dirty INTEGER NOT NULL DEFAULT 0, plc_seen_at TEXT, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS stations (id INTEGER PRIMARY KEY, source TEXT NOT NULL, station_json TEXT NOT NULL, dirty INTEGER NOT NULL DEFAULT 0, plc_seen_at TEXT, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS defaults_profiles (id TEXT PRIMARY KEY, doc_json TEXT NOT NULL, updated_at TEXT NOT NULL);
