# Task 생성 엔진 (콘솔)

`apps/gr-console/src/taskgen/` — 규칙(파라미터)으로 후보를 만들고, 상황별 가중치로 순서를 정하고, 두 로봇 영역이
겹치지 않는 것만 **예정**으로 생성한다. 보내기는 시나리오 실행기와 같은 규칙을 따른다.

## 지금 GRM PLC 는 어떻게 만드나 (참고, 읽기 전용)

- GRM 은 스스로 판단해 작업을 만들지 않는다. HMI 에서 고른 고정 시나리오 리스트를 인덱스 순서로 보낸다
  (`PL_Scenario.scl`, `RobotScenarioTask.scl`, `NextScenarioIndex.scl`). 오프라인 Auto 에서만 돈다(`PL_Auto.scl:37`).
- 보내는 조건은 `STAT.Task.Status.Accept` 와 `Task.Queue[1]` 가 비어 있는 것뿐이다. 스테이션 인터록
  `(PI.Req AND PI.CVOK) OR true` 는 무효화돼 있고(`RobotScenarioTask.scl:297`), 재고 조건이나 로봇 우선순위는 없다.
- 생성 시점의 충돌 회피는 주석 처리돼 있다(`PL_Scenario.scl:90-108`, `RobotAvoid` 는 호출되지 않음).
- 콘솔이 읽을 수 있는 것: `OPCUA.STATION[n]` 의 Para · Tracking · Interlock(PI: CVOK/Req/MeasReq/ItemExist,
  PO: CVNO/Comp/MeasComp/MeasErr — `Comm_CV.scl:325-352`), 로봇 STAT(Accept · Queue · Drive 위치).

## 규칙

| 필드 | 뜻 |
|---|---|
| `trigger` | `manual`(요청 버튼 수만큼) · `station_req`(PI.Req) · `station_item`(PI.ItemExist) · `cell_stock`(셀 재고 ≥ min, 품목 선택) |
| `action` | `transfer`(PICK→DROP 짝, 이송 지시 하나) · `move` · `measure` |
| `robots` | 보낼 수 있는 로봇(비면 전부) |
| `priority` | 기본 점수 |
| `enabled` | 규칙별 켜기 |

`transfer` 는 출발 셀에 재고가, 도착 셀에 칸(StackMax − 예상 재고)이 있어야 후보가 된다. 규칙 하나는 진행 중인
생성 작업이 끝나기 전에는 다시 만들지 않는다. 설정은 `taskgen_config` 표에 버전별 JSON 으로 남는다.
전체 자동 생성(`auto`)은 기본 **꺼짐** — 꺼져 있어도 후보와 점수는 계산해 보여 준다.

## 우선순위

점수 = `priority` + 대상 가중(`weights.target[스테이션/셀]`) + 품목 가중(`weights.item[코드]`) + 로봇 가중
+ 대기 시간(`age_per_min` × 분) − 거리(`distance_per_m` × 로봇 X 에서 첫 목표까지 m). 영역에 막힌 후보는 설명용으로
`blocked_penalty` 를 뺀다. 순서는 점수 내림차순, 같으면 조건이 먼저 참이 된 순 — 결정적이다. 화면의 후보 행에 점수 근거가 뜬다.

## 영역 (충돌 방지 구역)

- 간격 = PLC PARA p11(1903) + p16(400) + p13(100) = 2403 mm (`/api/anticol` 로 변경).
- 후보 영역 = 로봇 시작 X(앞 Task 목표 → 지금 X) ~ 목표, 짝이면 DROP 목표까지.
- 다른 로봇이 잡은 영역 = 지금 X + 발행된 진행 중 Task 목표 + 예정(생성됐지만 안 보낸) 영역 + 이번 판정에서 고른 것.
  **PLC 가 받은(Accepted/Queued/Running) 명령이 있으면 지금 X 는 빼고 목표만** 잡는다 — 두 로봇은 X 에서 서로를
  건너지 못하므로 목표가 비켜 있으면 가는 길도 비켜 있다.
- 생성 때와 발행 때 모두 검사한다. 겹치면 지금은 만들지 않고 다음 판정에서 다시 본다. 겹치지 않는 후보를 먼저 골라
  두 로봇이 동시에 돈다.

## 회피 순서

A 의 최선 후보가 B 의 자리를 필요로 하면:

1. B 에 비켜 가는 후보(도착점이 A 영역에서 간격 밖)가 있으면 그것을 먼저 만든다. A 는 "B 명령 수령 대기" 로 남는다.
2. B 의 명령이 PLC 에 받아지면(Accepted) B 는 목표만 잡으므로 다음 판정에서 A 가 생성된다.
3. B 에 비켜 줄 작업이 없으면 A 는 사유와 함께 기다린다. **자동 회피 MOVE 는 만들지 않는다.**
4. 서로 상대 자리를 원하면 둘 다 만들지 않고 사유를 낸다 — 겹치는 두 명령을 동시에 내지 않는다.

## 발행

예정 큐의 로봇마다 맨 앞 항목의 다음 스텝을 보낸다: 게이트 → 큐 깊이 1 → 영역 → (PICK 이면 이송 지시 열기) → 작성 →
제출(Hand · StackMax 검사). 짝 DROP 은 PICK 이 PLC 에 받아진 뒤 보내고, PICK 이 나쁘게 끝나면 보내지 않는다.
시나리오 실행 중에는 생성도 발행도 멈춘다. 안 보낸 예정은 지울 수 있다(`DELETE /api/taskgen/queue/{id}`).

## API

`GET /api/taskgen` · `PUT /api/taskgen/config` · `POST /api/taskgen/rules/{id}/request` · `DELETE /api/taskgen/queue/{id}`
