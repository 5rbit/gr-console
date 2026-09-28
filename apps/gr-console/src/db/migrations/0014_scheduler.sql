-- 스케줄러(docs/scheduler.md): 스테이션 프로파일(역할 · 내리기 방식), 요청 목록(상위 · 사용자), 판정 기록.
CREATE TABLE IF NOT EXISTS station_profile (station_id INTEGER PRIMARY KEY, doc_json TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS transfer_requests (id TEXT PRIMARY KEY, seq INTEGER NOT NULL, state TEXT NOT NULL, ext_ref TEXT, doc_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS transfer_requests_state ON transfer_requests(state, seq);
CREATE UNIQUE INDEX IF NOT EXISTS transfer_requests_ext ON transfer_requests(ext_ref) WHERE ext_ref IS NOT NULL;
CREATE TABLE IF NOT EXISTS taskgen_log (id INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, robot INTEGER, detail TEXT NOT NULL, num REAL);
CREATE INDEX IF NOT EXISTS taskgen_log_at ON taskgen_log(at);
CREATE INDEX IF NOT EXISTS taskgen_log_kind ON taskgen_log(kind, id);
-- 이송 지시의 출처(요청 진행을 세는 데 쓴다) — 기존 행은 doc_json 에만 있다.
ALTER TABLE transfer_orders ADD COLUMN source TEXT;
CREATE INDEX IF NOT EXISTS transfer_orders_source ON transfer_orders(source);
