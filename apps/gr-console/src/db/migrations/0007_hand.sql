-- 로봇 그리퍼에 든 화물(Hand) — PICK 완료로 셀에서 옮겨 오고 DROP 완료로 셀에 내려놓는다. 키 = 로봇 상태 PLC 이름(원장 plc).
CREATE TABLE IF NOT EXISTS hand (
  plc TEXT PRIMARY KEY,
  item_code INTEGER NOT NULL DEFAULT 0,
  count INTEGER NOT NULL DEFAULT 0,
  transfer_order_id TEXT,
  updated_at TEXT NOT NULL
);
