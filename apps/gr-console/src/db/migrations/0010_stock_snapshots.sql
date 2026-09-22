-- 재고 스냅샷 — 한꺼번에 바꾸는 조작(전체 비우기 · Excel 가져오기 · 되돌리기) 직전의 재고 표 전체(되돌리기용, 2026-09-22).
-- rows_json = `[StockEntry]`(0 개 줄 포함 그대로), row_count = 재고가 있는(count > 0) 칸 수, total = 개수 합.
-- restored_at = 이 스냅샷으로 마지막으로 되돌린 시각, restored_from = `restore-before` 스냅샷이 가리키는 복원 대상 id.
CREATE TABLE IF NOT EXISTS stock_snapshots (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at    TEXT    NOT NULL,
    reason        TEXT    NOT NULL,
    rows_json     TEXT    NOT NULL,
    row_count     INTEGER NOT NULL,
    total         INTEGER NOT NULL,
    restored_at   TEXT,
    restored_from INTEGER
);
