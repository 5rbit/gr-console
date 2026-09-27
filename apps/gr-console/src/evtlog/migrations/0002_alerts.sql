-- 알림 규칙과 알림 기록. 규칙은 행이 저장될 때 서버가 판정한다(evtlog::alerts).
-- match_json: {"plc": "GR2"?, "cat": "ALARM"?, "codes": ["ALM_TO_FAULT", "601"]?, "min_lvl": "ERROR"?, "text_contains": "..."?}
CREATE TABLE IF NOT EXISTS alert_rules (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    match_json TEXT NOT NULL,
    cooldown_s INTEGER NOT NULL DEFAULT 60,
    updated_at TEXT NOT NULL
);

-- event_id 는 events.id — 보존 기간으로 행이 지워질 수 있어 외래 키를 걸지 않는다.
CREATE TABLE IF NOT EXISTS alerts (
    id INTEGER PRIMARY KEY,
    ts INTEGER NOT NULL,
    rule_id INTEGER NOT NULL,
    rule_name TEXT NOT NULL,
    event_id INTEGER NOT NULL,
    acked_at INTEGER
);
CREATE INDEX IF NOT EXISTS alerts_acked ON alerts(acked_at, id);
CREATE INDEX IF NOT EXISTS alerts_ts ON alerts(ts);

-- 기본 규칙(한 번만 — 마이그레이션은 한 번 돈다)
INSERT INTO alert_rules (name, enabled, match_json, cooldown_s, updated_at) VALUES
    ('ERROR 레벨 전체', 1, '{"min_lvl":"ERROR"}', 30, datetime('now')),
    ('EMS', 1, '{"codes":["SAFE_EMS","SAFE_GRM_EMS","CMD_EMS"]}', 0, datetime('now')),
    ('FAULT 전환', 1, '{"codes":["ALM_TO_FAULT"]}', 0, datetime('now'));
