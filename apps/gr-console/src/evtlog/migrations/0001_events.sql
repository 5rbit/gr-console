-- PLC 이벤트 로그 (EVTLOG DB950 수집 + 콘솔 이벤트). 시각은 Unix epoch ms.
-- plc_ts = 사건 시각(PLC 행은 PLC 시계, 콘솔 행은 콘솔 시계), rx_ts = 콘솔이 받은 시각.
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    plc TEXT NOT NULL,
    epoch INTEGER NOT NULL DEFAULT 0,
    seq INTEGER,
    plc_ts INTEGER NOT NULL,
    rx_ts INTEGER NOT NULL,
    cat INTEGER NOT NULL,
    lvl INTEGER NOT NULL,
    src INTEGER NOT NULL DEFAULT 0,
    code INTEGER NOT NULL,
    a INTEGER NOT NULL DEFAULT 0,
    b INTEGER NOT NULL DEFAULT 0,
    ctx INTEGER NOT NULL DEFAULT 0,
    origin TEXT NOT NULL,
    detail TEXT
);
CREATE INDEX IF NOT EXISTS events_plc_ts ON events(plc, plc_ts);
CREATE INDEX IF NOT EXISTS events_ts ON events(plc_ts);
CREATE INDEX IF NOT EXISTS events_cat_code ON events(cat, code);
CREATE INDEX IF NOT EXISTS events_ctx ON events(ctx);
CREATE UNIQUE INDEX IF NOT EXISTS events_plc_seq ON events(plc, epoch, seq) WHERE seq IS NOT NULL;

-- 수집기 상태(PLC 마다): 어디까지 읽었나. 콘솔을 다시 켜도 이어 읽는다.
CREATE TABLE IF NOT EXISTS evt_state (
    plc TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL,
    boot_id INTEGER,
    last_seq INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);
