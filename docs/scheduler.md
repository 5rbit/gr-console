# Task 스케줄러 (GCS)

`apps/gr-console/src/taskgen/` — 콘솔이 GCS 로서 PICK/DROP 짝을 만든다. 예전 "자동 생성"(규칙을 대상마다 하나씩)을
**모든 셀·스테이션에 공통으로 도는 정책 + 요청 목록**으로 바꾼 것이다. 규칙(사용자 규칙)은 예외용으로 남는다.

화면: **스케줄러** 탭(요청 · 수요 · 결정 · 정책 · 기록). 작업 명령 화면의 자동 생성 탭은 한 줄 요약(스위치 · 다음
후보 · 예정 수)만 남고, 레이아웃 맵 모니터링 모드가 대상마다 PICK/DROP 준비 표시(▲ ▼)를 그린다.

## 흐름

```
스테이션 프로파일(역할 · 내리기 방식)   ─┐
GRM PI/PO · 미래 재고 · 예약            ─┼─► 준비 상태(ready): 대상마다 PICK · DROP · MEASURE 가능 여부와 사유
                                        │
요청 목록(상위 · 사용자)  ── 먼저 ──────┤
정책(입고 자동 · 출고 자동 · 측정 먼저 ·├─► 파생 규칙(수요) ─► 후보(규칙 × 로봇 × 자동 셀) ─► 점수 ─► 영역 선택
     빈 시간 정리)                      │                                   ─► 예정 큐 ─► 발행(짝 · 게이트 · 깊이)
사용자 규칙(예외)                       ─┘
```

## 운전자 규칙 (2026-09-28)

1. **미래 재고** — 로봇에 배정된 이송 이후에 만드는 작업은 그 이송이 끝난 뒤의 재고로 판단한다. 판단 재고 =
   재고 표 + 진행 중 Task + 보내지 않은 짝 초안(Draft) + 예정 큐의 아직 안 보낸 스텝.
2. **스테이션 작업은 늘 Req** — 스테이션 PICK/DROP/MEASURE 는 `CVOK & Req` 가 있어야 만든다(어떤 규칙이든).
   PICK 은 화물 있음(`ItemExist`), DROP 은 내리기 방식에 따라(하나씩 = 비어 있음, 스택 = 스테이션 최대 높이 · 품목 StackMax, 팔렛 = 다음 슬롯).
   앞 작업의 인계가 끝나야 한다(`PO.CVNO 0 & PO.Comp 0`), GRM 값이 신선해야 한다.
3. **짝이 깨지면 취소 = 사람이 화물을 들어낸다** — 짝 DROP 취소(또는 Hand 정리)는 재고에 반영하지 않고 Hand 를
   비우고 이송 지시를 중단한다. 짝으로 묶인 Task 는 같이 지운다.
4. **스테이션마다 설정** — 역할(PICK · DROP · 둘 다 · 끔)과 내리기 방식(하나씩 · 스택 · 팔렛).
5. **요청 먼저** — 상위(호스트) 요청과 사용자 요청(입고 · 출고 · 이동)을 정책 수요보다 먼저 수행한다.

## 스테이션 프로파일 (`station_profile` 표, 콘솔 소유)

| 필드 | 값 | 뜻 |
|---|---|---|
| `role` | `pick` · `drop` · `both` · `off` | 로봇이 이 스테이션에서 PICK(입고) / DROP(출고) 을 하나. 프로파일이 없으면 정책이 보지 않는다(사용자 규칙은 신호만 본다). |
| `drop_mode` | `single` · `stack` · `pallet` | 하나씩(비어 있어야) · 스택(높이까지 쌓음) · 팔렛(다음 슬롯). `pallet` 은 `pallet_station` 과 같이 저장된다. |
| `max_height_mm` | 0 = 높이 제한 없음 | 스택 스테이션에 쌓을 수 있는 최대 높이. 단수 한도(StackMax)는 **품목 규격**의 값이라 스테이션에 두지 않는다 — 스택 DROP 은 둘 다 본다. |
| `weight` | 점수 | 이 스테이션 수요에 더한다. |
| `max_wait_s` | 0 = 없음 | 준비된 뒤 이만큼 못 만들면 이벤트 로그 경고(`CON_SCHED`). |

추정(`guess`): GRM 연결로 — 라인 시작(prev 0 · next ≠ 0) = `drop`, 라인 끝(next 0 · prev ≠ 0) = `pick`, 연결 없음 =
`both`, 가운데 = `off`, 팔렛 스테이션 = `drop` + `pallet`. 추정은 저장하지 않는다(화면에서 적용).

## 준비 상태 (`GET /api/taskgen/ready`, mux 이벤트 `ready`)

대상(셀 · 스테이션)마다 `pick` · `drop` · `measure` 각각 `{ok, why, expr}`, 미래 재고, 예약(누가 이 대상을 쓰나).
스테이션은 `expr` 에 신호 조건식을 항목별로 싣는다(`CVOK & Req & ItemExist & CVNO 0 & Comp 0`, 꺼져 있어야 하는 항목은 `0`) —
수요 탭 · 맵 호버 · 모니터링 정보 카드가 충족은 초록, 미충족은 빨강으로 그린다.

- 스테이션 PICK: 역할 pick/both · 신선 · `CVOK & Req & ItemExist` · 인계 끝 · 예약 없음. 품목을 모르면 `ok` 이지만
  `need_item` — 요청이나 규칙이 품목을 정하지 않으면 입고를 만들지 않는다(`hints` = 트래킹 OD 에 맞는 품목).
- 스테이션 DROP: 역할 drop/both · 신선 · `CVOK & Req` · 인계 끝 · 예약 없음 · 방식별(하나씩 `ItemExist 0`, 스택 = 얹은 뒤 높이 ≤ `max_height_mm`
  이고 품목 StackMax 이내, 팔렛 다음 슬롯). 스택 높이 = 단마다 눌린 높이의 합(`Height − Compression·위에 얹힌 수`).
- 스테이션 MEASURE: 신선 · `CVOK & Req & ItemExist` · 인계 끝 · 예약 없음.
- 셀 PICK: Use · 미래 재고 ≥ 1 · 품목 앎. 셀 DROP: Use · 미래 남은 칸 ≥ 1(비었으면 칸 제한 없음).
  셀은 예약으로 막지 않는다(미래 재고가 반영한다) — 예약은 보이기만 한다.

## 요청 목록 (`transfer_requests` 표, `GET/POST /api/requests`)

| 필드 | 뜻 |
|---|---|
| `id` | `RQ-YYMMDD-NNNN` |
| `source` | `host`(상위) · `user` |
| `ext_ref` | 상위 참조 번호 — 같은 값으로 다시 보내면 새로 만들지 않고 기존 것을 돌려준다(멱등). |
| `kind` | `inbound`(스테이션 → 셀) · `outbound`(셀 → 스테이션) · `move`(셀 → 셀) |
| `item_code` | 입고: 스테이션 품목을 콘솔이 모를 때 이 값으로 확정. 출고: 내보낼 품목(없으면 가장 오래된 재고). |
| `qty` | 수량(짝 수). `done` · `active` · `failed` 는 이송 지시(`source = req:<id>`)에서 센다. |
| `from` · `to` | 비면 자동: 입고 from = 준비된 PICK 스테이션 아무거나, to = 정책의 입고 셀 고르기. 출고 from = 정책의 출고 셀 고르기, to = 준비된 DROP 스테이션 아무거나. |
| `priority` | 요청끼리의 순서(클수록 먼저, 같으면 먼저 들어온 것). |
| `state` | `open` → `active`(지시 진행 중) → `done` · `canceled` · `failed` |

요청마다 파생 규칙이 대상 스테이션별로 생긴다(`req:<id>@<station>`) — 점수 = `request_priority`(정책, 기본 1000) +
`priority` × 10 + 대기 가점. 그래서 요청이 정책 수요보다 늘 먼저다. 남은 수(`qty − done − active`)가 0 이면 만들지 않는다.

## 정책 (`GenConfig.policy`)

| 필드 | 기본 | 뜻 |
|---|---|---|
| `inbound_auto` | true | 요청이 없어도 PICK 스테이션에 화물이 준비되면 셀로 적재(품목을 알 때만). |
| `inbound_dest` | nearest · `near_drop` | 적재 셀 고르기. `near_drop` 이면 가장 가까운 DROP 스테이션 기준. |
| `outbound_auto` | **false** | 요청이 없어도 DROP 스테이션이 요청하면 가장 오래된 재고를 내보낸다. |
| `outbound_source` | oldest | 출고 셀 고르기. |
| `measure_first` | true | 입고 품목에 비드 프로파일이 없으면 그 스테이션에서 먼저 MEASURE. |
| `consolidate` | false | 빈 시간 정리 — 로봇이 `consolidate_idle_s` 동안 할 일이 없으면 같은 품목을 한 셀로 모은다. |
| `*_priority` | 입고 40 · 출고 60 · 측정 80 · 요청 1000 · 정리 5 | 수요 기본 점수. |

## 엔진 동작

- 중복 방지 키 = 파생 규칙 id(스테이션 · 요청 단위) — 한 스테이션 작업이 다른 스테이션을 막지 않는다.
- 스테이션 예약 = 모든 경로(단일 · 순차 · 생성)의 살아 있는 Task + 예정 큐 — 같은 스테이션에 두 작업을 만들지 않는다.
- 실패 냉각: 같은 키가 중단되면 `gen_fail_cooldown_s` × 2^(n−1)(최대 `gen_fail_cooldown_max_s`) 동안 만들지 않는다.
  `gen_fail_alert` 번째부터 이벤트 로그 경고. 화면에서 풀 수 있다(`DELETE /api/taskgen/cooldown/{key}`).
- 로봇 Hand 에 화물이 있는데 짝 DROP 이 없으면 그 로봇에는 만들지 않는다 → 결정 탭의 "Hand 정리"(사람이 제거).
- 거리 = 로봇이 **앞 작업을 끝낸 자리**(마지막 목표 X, 없으면 지금 X)에서 — 이어지는 작업(복합 사이클)이 가까우면 먼저.
- 판정 기록(`taskgen_log`)과 KPI(`GET /api/taskgen/kpi`)는 재시작해도 남는다.
- 시뮬레이션(`POST /api/taskgen/simulate`): 저장 전 설정으로 한 번 판정해 후보·선택을 돌려준다(만들지 않음).

## API

| 메서드 · 경로 | 뜻 |
|---|---|
| `GET /api/taskgen` | 설정 · 규칙 상태(파생 포함, `origin`) · 입력 · 후보 · 예정 · 지표 · 냉각 · Hand 경고 |
| `PUT /api/taskgen/config` | 설정(정책 · 가중 · 사용자 규칙) 저장 |
| `POST /api/taskgen/simulate` | `{config}` → 판정 결과(저장 · 생성 없음) |
| `GET /api/taskgen/ready` | 준비 상태 스냅샷(mux `ready` 도 같은 모양, 바뀔 때만) |
| `GET /api/taskgen/stations` | 등록 스테이션마다 `{id, profile, guess, pallet}` |
| `PUT /api/taskgen/stations/{id}` | 프로파일 저장(`role: null` 이면 지움) |
| `POST /api/taskgen/stations/apply-guess` | `{ids?}` 추정 적용(비면 프로파일 없는 스테이션 전부) |
| `POST /api/taskgen/migrate-rules` | `{dry_run}` 기본 규칙 한 벌(`measure-first-*` · `store-near-out-*` · `ship-*`)을 프로파일 · 정책으로 옮기고 그 규칙을 끈다 |
| `DELETE /api/taskgen/cooldown/{key}` | 냉각 풀기 |
| `POST /api/taskgen/hand/{robot}/remove` | Hand 정리 — 사람이 화물을 제거(재고 반영 없음, 짝 Task 취소, 지시 중단) |
| `GET /api/taskgen/log?limit&kind` | 판정 기록 |
| `GET /api/taskgen/kpi?hours` | KPI |
| `GET /api/requests?state&limit` | 요청 목록(`state=open` = open · active) |
| `POST /api/requests` | 요청 하나(`ext_ref` 멱등) |
| `POST /api/requests/batch` | 여러 개(상위 목록) — 결과 배열 |
| `PATCH /api/requests/{id}` | `{priority?, qty?, note?}` |
| `POST /api/requests/{id}/cancel` | 취소(남은 수만 — 진행 중 지시는 그대로 끝난다) |
