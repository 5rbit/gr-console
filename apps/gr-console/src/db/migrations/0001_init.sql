-- console core
CREATE TABLE IF NOT EXISTS counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sync_cursor (plc TEXT NOT NULL, ring TEXT NOT NULL, top_key TEXT, PRIMARY KEY (plc, ring));
CREATE TABLE IF NOT EXISTS meas_entries (seq INTEGER PRIMARY KEY, plc TEXT NOT NULL, ts TEXT NOT NULL, kind INTEGER NOT NULL, status INTEGER NOT NULL, code INTEGER NOT NULL DEFAULT 0, entry_json TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS meas_entries_kind ON meas_entries(kind, seq);
CREATE INDEX IF NOT EXISTS meas_entries_code ON meas_entries(code, seq);
