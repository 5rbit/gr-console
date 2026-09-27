-- 이벤트 화면의 이름 붙인 필터. query = 화면 URL 의 e.* 쿼리(링크 복사와 같은 모양).
CREATE TABLE IF NOT EXISTS saved_filters (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    query TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
