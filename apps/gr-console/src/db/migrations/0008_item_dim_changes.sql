-- 화물 규격 치수 변경 이력 — 측정 반영(`registry::dims`)·되돌리기가 남긴다.
-- 한 줄 = 품목 하나의 필드 하나. `samples` 는 근거가 된 측정 기록 `[{plc, seq}]` JSON(되돌리기는 '[]').
CREATE TABLE IF NOT EXISTS item_dim_changes (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    code     INTEGER NOT NULL,
    field    TEXT    NOT NULL,
    before   REAL    NOT NULL,
    after    REAL    NOT NULL,
    source   TEXT    NOT NULL,
    samples  TEXT    NOT NULL DEFAULT '[]',
    note     TEXT    NOT NULL DEFAULT '',
    at       TEXT    NOT NULL,
    reverted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS item_dim_changes_code ON item_dim_changes(code, id);
