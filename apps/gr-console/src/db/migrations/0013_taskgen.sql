-- Task 생성 규칙·가중치(버전별 JSON, 최신 = 가장 큰 version) — taskgen/run.rs
CREATE TABLE IF NOT EXISTS taskgen_config (version INTEGER PRIMARY KEY, doc_json TEXT NOT NULL, saved_at TEXT NOT NULL);
-- 스케줄링·생성 파라미터(버전별 JSON + 바뀐 값 이력) — params.rs
CREATE TABLE IF NOT EXISTS sched_params (version INTEGER PRIMARY KEY, doc_json TEXT NOT NULL, saved_at TEXT NOT NULL, saved_by TEXT NOT NULL DEFAULT '', changes_json TEXT NOT NULL DEFAULT '[]');
-- 생성 엔진 예정 큐 · 수동 요청 수(재시작 뒤에도 이어서, 중복 생성 없음)
CREATE TABLE IF NOT EXISTS taskgen_queue (id TEXT PRIMARY KEY, doc_json TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS taskgen_manual (rule_id TEXT PRIMARY KEY, count INTEGER NOT NULL);
