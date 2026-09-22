-- 설정 변경 이력 — 기본값(작업 파라미터) 등 settings 키를 저장할 때마다 한 줄(이전 · 이후 JSON).
-- 되돌리기(restore)와 "누가 언제 무엇을" 추적에 쓴다. 키마다 최근 N 줄만 남긴다(registry::defaults_io).
CREATE TABLE IF NOT EXISTS settings_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL,
    version INTEGER NOT NULL,
    at TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    before_json TEXT,
    after_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS settings_history_key ON settings_history(key, id);
