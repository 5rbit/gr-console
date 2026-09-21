-- 팔렛 설정을 스테이션에서 떼어 품목·로봇·스테이션으로 나눈다(결정 2026-09-21).
--
-- - 품목(`pallet_item`): 패턴 — 입고/출하 Flow, Pattern(비면 OD 자동), Gap, 배치 Rotation·Mirror, PalletSize.
--   위치 좌표는 GR1·GR2 가 같이 쓴다.
-- - 로봇(`pallet_robot`): 헤드 방향이 달라 드래그 인/아웃 방향만 로봇마다 다르다 → 방향 코드에만 거는 변환.
-- - 스테이션(`pallet_station`): 팔렛 스테이션 여부만.
--
-- 옛 `pallet_profile`(스테이션별 전부)은 운용 DB 에 행이 없었다(2026-09-21 확인). 켜짐 여부만 옮기고 지운다.

CREATE TABLE IF NOT EXISTS pallet_item (
    code        INTEGER PRIMARY KEY,
    -- PICK/MEASURE 는 flow_in 을, DROP 은 flow_out 을 먼저 쓰고 없으면 다른 쪽
    flow_in     TEXT,
    flow_out    TEXT,
    -- NULL = OuterDiameter 로 자동
    pattern     INTEGER,
    gap         REAL    NOT NULL DEFAULT 50,
    rotation    INTEGER NOT NULL DEFAULT 0,
    mirror_x    INTEGER NOT NULL DEFAULT 0,
    mirror_y    INTEGER NOT NULL DEFAULT 0,
    pallet_size REAL    NOT NULL DEFAULT 1600,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pallet_robot (
    robot       INTEGER PRIMARY KEY,
    rotation    INTEGER NOT NULL DEFAULT 0,
    mirror_x    INTEGER NOT NULL DEFAULT 0,
    mirror_y    INTEGER NOT NULL DEFAULT 0,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pallet_station (
    station_id  INTEGER PRIMARY KEY,
    enabled     INTEGER NOT NULL DEFAULT 1,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL
);

INSERT OR IGNORE INTO pallet_station (station_id, enabled, note, updated_at)
    SELECT station_id, enabled, note, updated_at FROM pallet_profile;

DROP TABLE IF EXISTS pallet_profile;
