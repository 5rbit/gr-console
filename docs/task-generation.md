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

## 화면 (작업 명령 → 자동 생성)

오른쪽 칸의 탭 셋 중 첫째다: **자동 생성 · 순차 생성 · 단일 생성**. 규칙 만들기·고치기는 팝업(행을 누르면 열린다),
가중치와 파라미터도 팝업(탭 안 도구 `⋯`). 자동 생성 스위치는 켤 때 한 번 되묻는다(끄기는 바로).
같은 내용을 **화면 머리띠 오른쪽 끝 도구 `⋯` → "자동 생성 — 규칙 · 판단 기준…"** 팝업으로도 볼 수 있다 —
다른 탭에서 작업하다 규칙·기준만 확인할 때. 팝업은 열려 있을 때만 조회한다.

조회가 실패해도(엔진 재시작 · 연결 끊김) 머리띠의 스위치·규칙 추가·도구는 남는다 — 사유 띠와 "다시 시도" 가
서고, 실패 토스트는 **처음 한 번만** 뜬다(마지막으로 읽은 값은 아래에 그대로).

규칙 표 한 줄에 `상태`(칩) · `만든 건`(누적, 툴팁에 마지막 시각)이 선다. 상태 칩은 `꺼짐`(규칙 off) ·
`조건 대기`(조건 거짓) · `만들 수 있음`(조건 참 · 자동 꺼짐) · `생성` · `예정` · `진행 중` · `대기`(영역·짝) ·
`못 만듦`(자동 셀 없음 · 미등록 · 거리). 규칙이 없으면 표 대신 "첫 규칙 만들기" 빈 상태가 선다.

**판단 기준** 묶음이 규칙마다 조건 항목을 한 줄씩 펼친다 — `Rule` · `보는 값`(`STATION 2101 Req`,
`출발 셀 401 재고` …) · `지금`(`0`, `3 개 / 필요 1`, `자동 선택(oldest)`) · `충족`, 머리에 `충족 n / 전체`.
항목은 `fires` 가 보는 것과 같아서(조건 + 품목 확정 + 출발·도착 준비) 조건이 왜 거짓인지 그 줄에서 읽힌다
(`taskgen::rule_terms` → API `rules[].terms`). 줄을 누르면 그 규칙 편집 팝업이 열린다.

**엔진이 보는 값** 묶음은 규칙과 무관하게 매 판정의 입력을 보인다 — 등록된 스테이션 전부(CVOK · Req ·
ItemExist, 켠 규칙이 보는 줄에 `●`), 셀(재고가 있거나 규칙이 겨냥한 것 — 재고 · 남은 칸), 로봇(X · 생성 작업
유무), 그리고 영역 간격 · 로봇당 큐 깊이 · 판정 주기 한 줄(`taskgen::EngineInputs` → API `inputs`).

아래 **모니터** 묶음은 지표 한 줄 · 후보(점수 근거 툴팁) · 예정 큐다.

## 생성 조건 (GCS 가 알아야 보낸다)

Task 는 아래가 모두 갖춰질 때만 만든다 — 하나라도 없으면 후보가 되지 않고, 판단 기준 목록이 그 줄을 아직으로 보인다.

1. **품목** : 규칙이 정한 품목, 없으면 **출발의 콘솔 재고**가 아는 품목. 출발이 스테이션이어도 같다 —
   GRM 은 품목 코드를 주지 않으므로(STATION 트래킹에는 OD 만 있다) 콘솔 재고(컨베이어 트래킹이 옮긴다)나
   규칙이 알아야 한다. 품목은 콘솔 품목 목록에도 있어야 한다(규격 · StackMax 를 알아야 Z 를 만든다).
2. **출발 준비** : 콘솔 재고 ≥ 필요 수량(셀 · 스테이션 같은 규칙).
3. **도착 준비** : 남은 칸(StackMax − 재고) ≥ 필요 수량. 팔렛 스테이션이면 다음 슬롯이 있어야 한다.
4. **위치 등록** : 출발 · 도착 둘 다 레지스트리에 있어 X 를 알아야 한다.

1~3 은 **규칙마다 켜고 끈다**(`Rule.cond`, 기본 모두 켜짐) — 규칙 팝업의 "생성 조건" 스위치 넷:
품목 확정 · 출발 재고 · 도착 칸(StackMax) · Use(사용) 확인. 끈 항목은 판정에서 빠지고 판단 기준 목록에
"무시 (조건 끔)" 으로 남으며, 규칙 표의 `조건` 열이 `2/4` 처럼 켜진 수(툴팁에 끈 항목 이름)를 좁은 칸에서도
보여 준다. 끈 조건은 제출 문까지 함께 완화된다(`allow_unknown_item` · `ignore_stack_max`).
**4(위치 등록)는 끌 수 없다** — 위치 없이는 명령을 만들 수 없다. Use 확인은 자동 셀 선택에서는 늘 걸린다
(`in_filter`), 고정 대상에만 이 스위치가 적용된다.

제출 직전에도 한 번 더 막는다(`issue::enforce_item_known`) — 어느 경로로도 품목 0 인 PICK/DROP 은 나가지
않는다(PLC 가 INVALID_ITEM_CODE 501 로 거부하는 값이다).

## 우선순위

점수 = `priority` + 대상 가중(스테이션/셀) + 품목 가중(규격) + 로봇 가중 + 대기 가점(`gen_age_per_min` × 분) − 거리 감점(`gen_distance_per_m` × m). `gen_max_distance_m` 보다 먼 후보는 만들지 않는다. 순서는 점수 내림차순,
같으면 조건이 먼저 참이 된 순 — 결정적이다. 화면의 후보 행 툴팁이 점수 근거를 보인다.

## 영역 (충돌 방지 구역)

- 간격 = `anticol_separation_mm`(기본 **5000 mm** — 안전값) + 두 로봇의 `robot_margin_mm`. PLC 계산값(p11 1903 + p16 400 +
  p13 100 = 2403, 두 로봇 중 큰 값)은 참고로 보이고(다르면 ≠) **하한**이다 — 그보다 작게는 저장되지 않는다.
  예전 기본값 2403 이 사람 손을 안 탄 채 저장돼 있으면 5000 으로 올리고, 사람이 저장한 값은 그대로 두되 경고한다.
- 후보 영역 = 로봇 시작 X(앞 Task 목표 → 지금 X) ~ 목표, 짝이면 DROP 목표까지.
- 다른 로봇이 잡은 영역 = 지금 X + 발행된 진행 중 Task 목표 + 예정 영역 + 이번 판정에서 고른 것.
  **PLC 가 받은 명령(Accepted/Queued/Running)이 있으면 지금 X 는 빼고 목표만**(X 에서 서로 건너지 못한다).
- 생성 때와 발행 때 모두 검사. 겹치면 지금은 만들지 않고 다음 판정에서 다시 본다. 자동 셀도 영역 밖에서 고른다.

## 회피 순서

1. A 가 B 의 자리를 필요로 하면 B 에 비켜 가는 후보(도착점이 A 영역에서 간격 밖)를 먼저 만든다. A 는 "B 명령 수령 대기".
2. B 명령이 PLC 에 받아지면 B 는 목표만 잡으므로 다음 판정에서 A 가 생성된다.
3. B 에 비켜 줄 작업이 없으면 A 는 사유와 함께 기다린다. **자동 회피 MOVE 는 만들지 않는다.**
4. 서로 상대 자리를 원하면 둘 다 만들지 않는다 — 겹치는 두 명령을 동시에 내지 않는다.

## 짝 생성 (PICK/DROP)

Task 는 **짝 단위로 만든다** — PICK 을 보낼 때 짝 DROP 을 같은 이송 지시로 **초안**까지 함께 올린다.
보내기는 하나씩이다(DROP 은 PICK 이 PLC 에 받아진 뒤). 초안은 보낼 때 다시 작성해(`ops::submit_refreshed`)
그동안 바뀐 재고·팔렛 슬롯을 반영하고, WorkId/TaskId 는 만들 때 잡힌 값을 지킨다. 짝이 중단되면 보내지 않은
초안은 지운다. 순차 생성(시나리오 실행기)도 같은 규칙이다 — 짝 DROP 초안은 같은 실행·같은 스텝 번호로 찾는다.
이 덕에 이송 지시는 열릴 때부터 `pick_task`·`drop_task` 가 모두 채워진다.

## 발행

예정 큐(`taskgen_queue`, 재시작 뒤 이어서)의 로봇마다 맨 앞 항목의 다음 스텝: 게이트 → 큐 깊이(고정 1) → 영역 →
이송 지시(짝당 한 번 — 재시도도 같은 지시, 시도는 지시 이력에) → 작성 → 제출(Hand · StackMax 검사).
짝 DROP 은 PICK 이 PLC 에 받아진 뒤, PICK 이 나쁘게 끝나면 보내지 않고 중단. 제출 실패는
`issue_retry_backoff_ms` 뒤 다시, `issue_max_retries` 를 넘으면 중단(지시 aborted). 시나리오 실행 중에는 멈춘다.
안 보낸 예정은 지울 수 있다(보내는 중이면 거부).

## 파라미터 (`params.rs`, 화면: 자동 생성 → 도구 → 스케줄링 · 생성 파라미터)

| Name | Unit | Default | 출처 |
| --- | --- | --- | --- |
| anticol_separation_mm | mm | 5000 | 하한 = PLC PARA p11/p12 + p16 + p13 (로봇별 실측 비교, 두 로봇 중 큰 값) |
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
