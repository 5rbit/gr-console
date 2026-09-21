-- 이송 지시(PICK/DROP 한 짝) + 재고 변경 기록 — 재고 이동을 지시 id 로 추적한다(stock/transfer.rs).
CREATE TABLE IF NOT EXISTS transfer_orders (id TEXT PRIMARY KEY, seq INTEGER NOT NULL, plc TEXT NOT NULL, state TEXT NOT NULL, from_id INTEGER, to_id INTEGER, doc_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS transfer_orders_state ON transfer_orders(state, seq);
CREATE INDEX IF NOT EXISTS transfer_orders_plc ON transfer_orders(plc, seq);
CREATE TABLE IF NOT EXISTS stock_log (id INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, item_before INTEGER NOT NULL, count_before INTEGER NOT NULL, item_after INTEGER NOT NULL, count_after INTEGER NOT NULL, reason TEXT NOT NULL, task_id TEXT, transfer_order_id TEXT);
CREATE INDEX IF NOT EXISTS stock_log_order ON stock_log(transfer_order_id);
-- 원장 Task 에 지시 id (기존 DB 는 열이 없으면 추가, 있으면 skip_existing_columns 가 건너뜀)
ALTER TABLE tasks ADD COLUMN transfer_order_id TEXT;
CREATE INDEX IF NOT EXISTS tasks_transfer_order ON tasks(transfer_order_id);
