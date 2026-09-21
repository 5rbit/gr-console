# Task 생성 엔진 (콘솔 = GCS)

`apps/gr-console/src/taskgen/` — 콘솔이 GCS(SRC 5000)로서 **유일한 작업 생성원**이다. GRM PLC 는 명령 중계
(`OPCUA.GR[n].CMD`)와 신호원(`OPCUA.STATION[n].Interlock.PI`)일 뿐이다. 규칙(파라미터)으로 후보를 만들고,
상황별 가중치로 순서를 정하고, 두 로봇 영역이 겹치지 않는 것만 **예정**으로 생성한다. 보내기는 시나리오
실행기와 같은 규칙을 따른다.

## 참고: 지금 GRM PLC 동작 (읽기 전용)

- GRM 은 판단해서 만들지 않는다. HMI 고정 시나리오 리스트를 인덱스 순서로 보낸다(`PL_Scenario.scl`,
  `RobotScenarioTask.scl`). 오프라인 Auto 에서만(`PL_Auto.scl:37`). 스테이션 인터록은 `OR true` 로 무효
  (`RobotScenarioTask.scl:297`), 생성 시점 충돌 회피는 주석 처리(`PL_Scenario.scl:90-108`).
- 콘솔 입력: `OPCUA.STATION[n].Interlock.PI` — 계약상 Bool 구조체 `LGR_Interface_CV_PI`(CVOK · Req · MeasReq ·
  ItemExist), Para · Tracking, 로봇 STAT(Accept · Queue · Drive 위치).

## 규칙

| 필드 | 뜻 |
| --- | --- |
| `trigger` | `manual`(요청 버튼 수만큼, DB 에 남음) · `station_req`(PI.Req) · `station_item`(PI.ItemExist) — 둘 다 `require_cvok`(기본 켜짐: CVOK 도) · `cell_stock`(셀 재고 ≥ min, 품목 선택) |
| `action` | `transfer`(PICK→DROP 짝, 이송 지시 하나) · `move` · `measure` |
| `from_auto` / `to_auto` | 셀 자동 선택: 구역·행·열 필터. 출발 `oldest`(재고 갱신이 가장 오래된) / `nearest`, 도착은 가까운 순, `same_item_first` 면 같은 품목 셀 먼저(아니면 빈 셀 먼저). StackMax 칸, 다른 로봇 영역 밖만 |
| `pallet_auto` | 도착이 팔렛 스테이션 — DROP 을 미리 작성해 다음 슬롯이 있을 때만 후보 |
| `robots` · `priority` · `enabled` | 보낼 로봇(비면 전부) · 기본 점수 · 규칙별 켜기 |

규칙 하나는 진행 중인 생성 작업이 끝나기 전에는 다시 만들지 않는다. 설정은 `taskgen_config`(버전별 JSON).
전체 자동 생성(`auto`)은 기본 **꺼짐** — 꺼져 있어도 후보·점수·못 된 사유는 계산해 보여 준다.

## 우선순위

점수 = `priority` + 대상 가중(스테이션/셀) + 품목 가중(규격) + 로봇 가중 + 대기 가점(`gen_age_per_min` × 분) − 거리 감점(`gen_distance_per_m` × m). `gen_max_distance_m` 보다 먼 후보는 만들지 않는다. 순서는 점수 내림차순,
같으면 조건이 먼저 참이 된 순 — 결정적이다. 화면의 후보 행 툴팁이 점수 근거를 보인다.

## 영역 (충돌 방지 구역)

- 간격 = `anticol_separation_mm`(기본 PLC PARA p11 1903 + p16 400 + p13 100 = 2403) + 두 로봇의 `robot_margin_mm`.
- 후보 영역 = 로봇 시작 X(앞 Task 목표 → 지금 X) ~ 목표, 짝이면 DROP 목표까지.
- 다른 로봇이 잡은 영역 = 지금 X + 발행된 진행 중 Task 목표 + 예정 영역 + 이번 판정에서 고른 것.
  **PLC 가 받은 명령(Accepted/Queued/Running)이 있으면 지금 X 는 빼고 목표만**(X 에서 서로 건너지 못한다).
- 생성 때와 발행 때 모두 검사. 겹치면 지금은 만들지 않고 다음 판정에서 다시 본다. 자동 셀도 영역 밖에서 고른다.

## 회피 순서

1. A 가 B 의 자리를 필요로 하면 B 에 비켜 가는 후보(도착점이 A 영역에서 간격 밖)를 먼저 만든다. A 는 "B 명령 수령 대기".
2. B 명령이 PLC 에 받아지면 B 는 목표만 잡으므로 다음 판정에서 A 가 생성된다.
3. B 에 비켜 줄 작업이 없으면 A 는 사유와 함께 기다린다. **자동 회피 MOVE 는 만들지 않는다.**
4. 서로 상대 자리를 원하면 둘 다 만들지 않는다 — 겹치는 두 명령을 동시에 내지 않는다.

## 발행

예정 큐(`taskgen_queue`, 재시작 뒤 이어서)의 로봇마다 맨 앞 항목의 다음 스텝: 게이트 → 큐 깊이(고정 1) → 영역 →
이송 지시(짝당 한 번 — 재시도도 같은 지시, 시도는 지시 이력에) → 작성 → 제출(Hand · StackMax 검사).
짝 DROP 은 PICK 이 PLC 에 받아진 뒤, PICK 이 나쁘게 끝나면 보내지 않고 중단. 제출 실패는
`issue_retry_backoff_ms` 뒤 다시, `issue_max_retries` 를 넘으면 중단(지시 aborted). 시나리오 실행 중에는 멈춘다.
안 보낸 예정은 지울 수 있다(보내는 중이면 거부).

## 파라미터 (`params.rs`, 화면: 생성 규칙 → Parameters)

| Name | Unit | Default | 출처 |
| --- | --- | --- | --- |
| anticol_separation_mm | mm | 2403 | PLC PARA p11/p12 + p16 + p13 (로봇별 실측 비교) |
| anticol_enabled | | true | |
| robot_margin_mm | mm | {} | 로봇별 그리퍼·타이어 여유 |
| gen_tick_ms | ms | 1000 | |
| gen_distance_per_m | score/m | 0 | |
| gen_max_distance_m | m | 0 (제한 없음) | |
| gen_age_per_min | score/min | 0 | |
| gen_blocked_penalty | score | 0 | |
| station_require_cvok | | true | 새 규칙 기본 |
| issue_queue_depth | 건 | 1 (고정) | |
| area_deadlock_ms | ms | 10000 | 시나리오 실행기 교착 |
| echo_timeout_ms | ms | (toml) | gr-console.toml `cmd.echo_timeout_ms` |
| issue_retry_backoff_ms / issue_max_retries | ms / 회 | 2000 / 3 | |
| pair_drop_after_pick_accepted / pair_defer_stop | | true (고정) | |
| stack_max_enforce | | true | |
| sync_debounce_ms / sync_poll_ms | ms | 3000 / 1000 | |

저장마다 버전 +1, 바뀐 값(old → new) · 누가 · 언제가 `sched_params` 에 남는다. 행마다 기본값 되돌리기.

## 지표 · 데모

`GET /api/taskgen` 의 `metrics`: 생성 · 발행 · 끝남 · 중단 · 제출 실패, 대기 사유 종류별 횟수(area · gate ·
queue_depth · pair · retry · busy). `--demo` 는 GRM `OPCUA.STATION` 인터록을 흉내 낸다(2101 Req 10 s 켜짐 · 5 s 꺼짐,
2102 ItemExist 7.5 s 주기, CVOK 켜짐).

## API

`GET /api/taskgen` · `PUT /api/taskgen/config` · `POST /api/taskgen/rules/{id}/request` · `DELETE /api/taskgen/queue/{id}` ·
`GET/PUT /api/params` · `GET /api/params/history` · `POST /api/params/reset`
