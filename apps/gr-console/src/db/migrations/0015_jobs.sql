-- 작업 대기열(2026-10-01): 사람·자동이 만든 작업이 로봇별로 모이고, 보내기 루프 하나가 PLC 로 보낸다.
-- 작업 = 짝(PICK + DROP, 이송 지시 하나, WorkId 하나 · TaskId 1·2) 또는 한 건. 문서 전체는 doc_json.
CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY,
  seq INTEGER NOT NULL,
  robot INTEGER,
  stage TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 50,
  work_id INTEGER,
  doc_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  ended_at TEXT
);
CREATE INDEX IF NOT EXISTS jobs_stage ON jobs(stage);
CREATE INDEX IF NOT EXISTS jobs_robot ON jobs(robot, stage);
CREATE INDEX IF NOT EXISTS jobs_work ON jobs(work_id);

-- 로봇별 보내기 스위치와 멈춤 사유(거부되면 멈추고 사람을 기다린다).
CREATE TABLE IF NOT EXISTS job_dispatch (
  robot INTEGER PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 0,
  paused TEXT,
  updated_at TEXT NOT NULL
);
