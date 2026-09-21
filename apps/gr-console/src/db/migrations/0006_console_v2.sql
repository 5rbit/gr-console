-- 콘솔 v2 — 측정 로그 재키잉, 화물 규격 확장, 팔렛 프로파일·패턴, SKU 비드 표본.
--
-- 0006…0011 로 나뉘어 있던 이번 주기의 변경을 하나로 합쳤다(배포된 적 없음 — 결정 2026-09-18).
-- 0005 까지 올라간 기존 DB 와 새 DB 가 **같은 모양**으로 끝나야 하므로, 이미 있던 표를 손대는 문장
-- (meas_entries 재키잉, settings 기본값 이전)은 그대로 남겨 둔다.

-- ── meas_entries: 키를 (plc, seq) 로 ────────────────────────────────────────
-- GR PLC 는 저마다 MEASLOG Seq 를 1 부터 센다. 옛 `seq PRIMARY KEY` 로는 GR1 과 GR2 의 같은 Seq 행이
-- 서로를 덮었다. 기존 행은 자기 plc 열을 그대로 들고 옮겨 간다.
DROP TABLE IF EXISTS meas_entries_new;
CREATE TABLE meas_entries_new (plc TEXT NOT NULL, seq INTEGER NOT NULL, ts TEXT NOT NULL, kind INTEGER NOT NULL, status INTEGER NOT NULL, code INTEGER NOT NULL DEFAULT 0, entry_json TEXT NOT NULL, PRIMARY KEY (plc, seq));
INSERT OR REPLACE INTO meas_entries_new (plc, seq, ts, kind, status, code, entry_json) SELECT plc, seq, ts, kind, status, code, entry_json FROM meas_entries;
DROP TABLE meas_entries;
ALTER TABLE meas_entries_new RENAME TO meas_entries;
CREATE INDEX IF NOT EXISTS meas_entries_kind ON meas_entries(plc, kind, seq);
CREATE INDEX IF NOT EXISTS meas_entries_code ON meas_entries(plc, code, seq);

-- ── 화물 규격 확장(콘솔 소유) ──────────────────────────────────────────────
-- 단수/팔레트 Max, 무게, 잰 스택 크기별 절대 비드 프로파일 (`registry::spec::ItemSpec`).
-- PLC 와이어 `LGR_Stock_Item` 은 그대로 — 이 열은 콘솔의 Z 규칙·재고 한도·표시에만 쓴다. '{}' = 모두 기본값.
ALTER TABLE tire_codes ADD COLUMN spec_json TEXT NOT NULL DEFAULT '{}';

-- ── 스테이션별 팔렛 프로파일(콘솔 소유) ────────────────────────────────────
-- 켜진(enabled) 프로파일이 있는 스테이션만 compose 가 팔렛 슬롯을 쓴다.
CREATE TABLE IF NOT EXISTS pallet_profile (
    station_id  INTEGER PRIMARY KEY,
    flow        TEXT    NOT NULL,
    gap         REAL    NOT NULL DEFAULT 50,
    rotation    INTEGER NOT NULL DEFAULT 0,
    mirror_x    INTEGER NOT NULL DEFAULT 0,
    mirror_y    INTEGER NOT NULL DEFAULT 0,
    pallet_size REAL    NOT NULL DEFAULT 1600,
    enabled     INTEGER NOT NULL DEFAULT 0,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL
);

-- ── 편집 가능한 팔렛 패턴 저장소(콘솔 소유) ────────────────────────────────
-- 첫 기동 때 내장 사양서 R4(`pallet/spec_r4.json`)로 채우고(settings `pallet.seed`), 그 뒤로는 운전자가
-- 화면에서 고친다. 생성기·계획·compose 는 이 표를 읽는다.
CREATE TABLE IF NOT EXISTS pallet_flow (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL,
    drag_kind   TEXT    NOT NULL CHECK (drag_kind IN ('in', 'out')),
    -- spec_r4 | custom | import
    source      TEXT    NOT NULL,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL,
    -- 사양서 흐름 id(초기화·비교 기준). 사양서와 무관하게 만든 흐름이면 NULL
    based_on    TEXT,
    -- 사양서 참고 정보: source_slide, also_slides, reference, robot, zone, screen_axes, notes
    meta_json   TEXT    NOT NULL DEFAULT '{}',
    sort        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS pallet_pattern (
    flow_id     TEXT    NOT NULL,
    pattern     INTEGER NOT NULL,
    od_min      REAL    NOT NULL,
    od_max      REAL    NOT NULL,
    -- [{slot:"C#n", seq, u:[x,y], drag_dir}] — u 는 기계 축 정규화 오프셋(최소 중심거리 단위)
    slots_json  TEXT    NOT NULL,
    note        TEXT    NOT NULL DEFAULT '',
    updated_at  TEXT    NOT NULL,
    -- 사양서 참고 정보: order_text, drawn, notes
    meta_json   TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (flow_id, pattern)
);

-- ── SKU 측정 표본(`registry::beads`) ───────────────────────────────────────
-- PLC 가 measureSKU 로 준 단별 비드를 품목 코드별로 적재한다. 규격에 반영했는지(`applied`)와 못 넣은
-- 이유(`reason`)를 같이 들고 있어 화면이 "왜 안 바뀌었는지" 를 보인다. 품목·PLC 당 최근 20 개만 남긴다.
CREATE TABLE IF NOT EXISTS item_bead_samples (
  code INTEGER NOT NULL,
  plc TEXT NOT NULL,
  seq INTEGER NOT NULL,
  at TEXT NOT NULL DEFAULT "",
  status INTEGER NOT NULL DEFAULT 0,
  diag_flags INTEGER NOT NULL DEFAULT 0,
  total_count INTEGER NOT NULL DEFAULT 0,
  each_height REAL NOT NULL DEFAULT 0,
  total_height REAL NOT NULL DEFAULT 0,
  layer_offset REAL NOT NULL DEFAULT 0,
  sample_json TEXT NOT NULL,
  applied INTEGER NOT NULL DEFAULT 0,
  reason TEXT NOT NULL DEFAULT "",
  recorded_at TEXT NOT NULL,
  PRIMARY KEY (code, plc, seq)
);
CREATE INDEX IF NOT EXISTS item_bead_samples_code ON item_bead_samples(code, seq DESC);

-- ── 저장된 기본값 이전(settings `defaults`) ────────────────────────────────
-- 드래그 기본 거리 150 mm (결정 2026-09-18). 저장된 기본값의 0 은 "정하지 않음" 이었으므로 150 으로 옮긴다.
-- 운전자가 일부러 넣은 다른 값은 건드리지 않는다.
UPDATE settings SET value_json = json_set(value_json, '$.base.drag_in_dist', 150)
  WHERE key = 'defaults' AND json_valid(value_json) AND COALESCE(json_extract(value_json, '$.base.drag_in_dist'), 0) = 0;
UPDATE settings SET value_json = json_set(value_json, '$.base.drag_out_dist', 150)
  WHERE key = 'defaults' AND json_valid(value_json) AND COALESCE(json_extract(value_json, '$.base.drag_out_dist'), 0) = 0;

-- 그립 기준에서 맨 비드(`bead`, 상부 비드를 그대로 잡기)를 뺀다 (결정 2026-09-18).
-- 남는 기준은 `mid`(Height/2, 기본)와 `pick_bead`(= 상부 비드 − PickBeadOffset, 화면 이름 `bead+offset`)뿐이라
-- 저장돼 있던 `bead` 는 `pick_bead` 로 옮긴다. 값 이름 자체(`pick_bead`)는 그대로 둔다.
UPDATE settings SET value_json = json_set(value_json, '$.grip_ref', 'pick_bead')
  WHERE key = 'defaults' AND json_valid(value_json) AND json_extract(value_json, '$.grip_ref') = 'bead';

-- 이미 발행된 작업이 들고 있는 요청 덮어쓰기(`tasks.doc_json` 의 `request.grip_ref`).
UPDATE tasks SET doc_json = json_set(doc_json, '$.request.grip_ref', 'pick_bead')
  WHERE json_valid(doc_json) AND json_extract(doc_json, '$.request.grip_ref') = 'bead';
