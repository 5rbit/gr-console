-- Task 생성 규칙·가중치(버전별 JSON, 최신 = 가장 큰 version) — taskgen/run.rs
CREATE TABLE IF NOT EXISTS taskgen_config (version INTEGER PRIMARY KEY, doc_json TEXT NOT NULL, saved_at TEXT NOT NULL);
