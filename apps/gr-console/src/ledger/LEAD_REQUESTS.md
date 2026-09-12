# ledger 슬라이스 → 리드 요청 (M3-B)

작업 범위 밖이라 손대지 않은 것들. 각 항목은 "무엇 · 왜 · 제안" 순.

## 1. `types.ts` `Task`에 `header`·`plc_name` 추가 (선택)

- 백엔드 `LedgerEntry`는 `header: Header | null`(제출 헤더 — 에코 대조 키)과 `plc_name: string`을 싣는다.
  프론트 공유 타입 `Task`에는 없어서 `lib/task/state.ts`의 `TaskEx`로 슬라이스 안에서만 넓혀 쓴다.
- 제안: `Task`에 `header: { protocol; cmd_id; cmd; src; dst; seq } | null`, `plc_name: string` 추가 후
  `TaskEx`를 지워도 된다.

## 2. `api.ts`에 새 엔드포인트 메서드 (선택)

- 추가된 엔드포인트: `GET /api/tasks/stats`, `GET /api/tasks/plc-view`, `POST /api/tasks/{id}/mark-failed`,
  `DELETE /api/tasks/{id}`, `GET /api/tasks?origin=&since=`.
- 지금은 `lib/task/actions.ts`의 `taskApi`가 `getJson/postJson/del`로 직접 부른다. 공유 클라이언트로
  올리려면 `api.taskStats / taskPlcView / taskMarkFailed(id, note?) / taskDelete` + `TaskQuery.origin/since`.

## 3. `TaskQuery`에 `origin`·`since` (선택)

- 백엔드 목록 필터에 `origin=console|scenario|external`, `since=<RFC3339>`(created_at 하한)를 더했다.
  `types.ts`의 `TaskQuery`에는 아직 없다.

## 4. `Ledger::query` 서명 유지 — 새 필터는 `query_filtered(QueryFilter)`

- 다른 슬라이스가 부르는 `query(states, type, q, limit, offset)`는 그대로 두고 `#[allow(dead_code)]`를
  달았다(현재 트리에서 호출자가 없어 `-D warnings`에 걸린다). 호출자가 생기면 attribute를 빼도 된다.

## 5. `ledger::Target`에 `PartialEq, Eq` 파생 추가 (완료, 공유 영향)

- `scenario/mod.rs`가 `Option<Target>`를 담은 구조체에 `PartialEq`를 파생해 필요했다. 다른 슬라이스에
  영향 없는 순수 추가.

## 6. 현재 트리 빌드 실패는 ledger 밖

- `scenario/runner.rs:324` (`ack` moved value), `scenario/mod.rs:88` (→ 5번으로 해결)은 시나리오 슬라이스.
- `cargo clippy -p gr-console -- -D warnings`는 `crates/plc-layout/src/parse.rs`(2건, HEAD 시점부터)와
  crate 전역 dead_code(`cmd::clear_header`, `config::plc`, `db::open_memory/with_mut`, `plc::Tier::OnDemand`,
  `PlcHandle::write`, `state.db` 등)로 실패한다. ledger 파일만 보면 깨끗하다(`--no-deps`, ledger/ 경로 0건).

## 7. ~~데모 월드 관찰~~ (닫힘 — `demo.rs`가 고쳐졌다)

- Delete 시 `removed`를 키로 dedup해 Canceled 링에 같은 키가 두 번 들어가지 않는다. 함께, 끝난
  Task는 한 틱 동안 `Now`에 남아 `Task.Status.Complete`/`Canceled` 비트를 올린 뒤 링으로 간다 —
  실 PLC의 순서이고, 동기 엔진이 그 비트로 먼저 종결하는 경로(`sync.rs` 3단계)가 데모에서 돈다.

## 9. 취소·완료 연동 (닫힘)

- `sync::apply` 3단계가 `Task.Status.Canceled`/`Complete`(Now 태스크의 비트)로 종결한다 — 링보다
  먼저. 콘솔이 요청한 것(`delete requested`/`complete requested` 이력 줄)은 종결 노트에
  `(console request)`가 붙어 PLC 자발 취소와 갈린다.
- `ops::cancel`이 `STAT.RES.Data[6]`(Delete 허용)을 `force_complete`의 Data[5]와 대칭으로 본다.
- Delete/Complete 뒤 `OP_ANSWER_MS`(5 s) 안에 원장이 안 움직이면 `System` 이력 줄
  `"Delete not answered by PLC within 5000 ms"`가 남는다 — 무시된 명령과 진행 중인 명령이 같은
  모양이던 것을 끊는다. 프런트 토스트도 돌아온 상태가 종결이 아니면 "요청 보냄 — PLC 응답 대기"다.

## 8. 검증 방법 메모

- 시나리오 슬라이스가 컴파일되지 않는 동안은 `git archive HEAD` 사본 + ledger 파일을 얹어
  (`CARGO_TARGET_DIR`는 공유) 빌드·테스트·데모 실행으로 검증했다.
