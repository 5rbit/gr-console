-- per-cell stock (console-owned: the PLC has no inventory table)
CREATE TABLE IF NOT EXISTS stock (
  cell_id INTEGER PRIMARY KEY,
  item_code INTEGER NOT NULL DEFAULT 0,
  count INTEGER NOT NULL DEFAULT 0,
  note TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL
);
-- completed tasks already folded into stock (idempotent replay of ledger events)
CREATE TABLE IF NOT EXISTS stock_applied (task_id TEXT PRIMARY KEY, applied_at TEXT NOT NULL);
