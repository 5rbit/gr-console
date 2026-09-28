# EVTLOG v2 행 인코딩 — ErrorList 항목 (Alarm / Warn / Operator / Info)

PLC 구현의 기준 문서다. 34 B `EVT_Entry` 와 `EVTLOG`(DB950) 레이아웃은 **그대로** 두고, 필드의 뜻만 정한다.
정본(무슨 항목이 있나)은 ErrorList 워크북이고, 콘솔은 그것을 `gr-contract errorlist` 로 읽은
`plc/contract/<PLC>/errorlist.json` 으로 문장을 만든다. 옛 행(V1.6.1 카탈로그 행)은 계속 읽힌다.

## 1. 한 줄 요약

```text
Cat  = 6 ALARM (Alarm, Warn) | 18 OPERATOR | 19 INFO
Code = Trans × 10000 + ErrorList 번호          F3119 발생 = 13119, W1101 해제 = 21101, I0301 = 30301
Src  = ALARM: 1 FAULT / 2 WARN                 OPERATOR·INFO: ErrorList 의 "Src = …" 값 (없으면 0)
A, B = 발생·순간: ErrorList 값 A / 값 B         해제: A = flicker 수, B = 켜져 있던 ms      억제 요약: A = 억제 건수, B = 0
Lvl  = 항목 레벨, 전이와 무관: Alarm ERROR(4) · Warn WARN(3) · Operator / Info INFO(2)
Ctx  = 지금과 같다 (로봇: 현재 WorkId, GRM: 0 또는 호출이 준 값)
```

왜 이 모양인가 (처음 제안 `Code = 번호, Src = 전이` 와 다른 점):

- ErrorList 가 **Src 를 값 칸으로 쓴다** — I0301 `Src = task type`, O0601 `Src = pNNN`, I0501 `Src = PARA 항목`,
  GRM I0301 `Src = 1·2 BRCV, 11·12 GET`. 그래서 Src 는 값으로 두고 전이를 Code 의 만 자리로 옮겼다. 번호는 4 자리라
  `Code mod 10000` 이 곧 ErrorList 번호다.
- ALARM 은 값을 A / B 로만 붙인다(`알람 통합` 시트). 그래서 ALARM 의 Src 는 영역(FAULT / WARN)을 싣는다 —
  F1101 · W1101 처럼 같은 번호가 두 영역에 있다. 옛 ALM_RAISED 의 Src 도 영역이었다.
- 옛 ALARM 행(601..604)과는 `Code ≥ 10000` 으로 갈린다. 카탈로그 코드는 모두 10000 미만이다.

## 2. 필드

| 필드 | 형 | v2 값 |
|---|---|---|
| `Cat` | USInt | `6` ALARM = Alarm + Warn, `18` OPERATOR, `19` INFO (`EVT_CAT_ALARM` · `EVT_CAT_OPERATOR` · `EVT_CAT_INFO`) |
| `Lvl` | USInt | Alarm `4` ERROR, Warn `3` WARN, Operator · Info `2` INFO (`EVT_LVL_*`). **모든 전이가 같은 레벨** — `Cfg.MinLevel` 이 발생만 남기고 해제를 버리는 일이 없다 |
| `Code` | UInt | `Trans × 10000 + N`. `N` = ErrorList 번호 1..9999(`F0101` → 101, `I5101` → 5101). 최대 49999 |
| `Src` | UInt | ALARM: 영역 `1` FAULT / `2` WARN(`EVT_ALARM_AREA_FAULT/WARN`) — F1101 과 W1101 처럼 번호가 같은 둘을 가른다. OPERATOR · INFO: 발생 · 순간 행은 ErrorList 값 칸의 `Src = …`(예: I0301 Src = task type, O0601 Src = pNNN), 없으면 `0`. 해제 · 요약 행은 `0` |
| `A` | DInt | 발생 · 순간: ErrorList 값 A. 해제: **flicker 수**. 요약: 억제 건수 |
| `B` | DInt | 발생 · 순간: ErrorList 값 B. **붙일 값이 하나도 없는 ALARM**(값 A · 값 B 뜻이 둘 다 빈 알람)은 로봇 `TASK.Proc.Step.Now`(옛 ALM_RAISED 와 같다), GRM 0. 뜻이 없는 쪽은 그 밖에는 0. 해제: **켜져 있던 ms**(발생 → 마지막 하강). 요약: 0 |
| `Ctx` | UDInt | 바꾸지 않는다 |

### Trans (`Code / 10000`, `EVT_TRANS_*`)

| 값 | 상수 | 언제 |
|---|---|---|
| 1 | `EVT_TRANS_RAISE` | 상태형 항목의 비트가 올라갔다 (열린 발생이 없을 때만) |
| 2 | `EVT_TRANS_CLEAR` | 비트가 `T_off` = 2 s 동안 계속 내려가 있었다 |
| 3 | `EVT_TRANS_MOMENT` | 순간형 항목 한 번 |
| 4 | `EVT_TRANS_SUPPRESSED` | 억제 요약 (아래 4 절) |

`EVT_TRANS_MUL` = 10000. 상수는 `plc/evtlog/catalog.toml` 의 `[enum.trans]` → `just gen-evt`.

## 3. 무엇이 v2 행인가

- **ErrorList 의 Alarm / Warn 행 전부** (Fault 시트 두 장) — `Cat` 6.
- **Operator / Info 시트의 상태형 · 순간형 행** (HMI 표시 Y/N 무관, Byte/Bit 가 있는 행) — `Cat` 18 / 19.
  HMI 클래스 TASK(작업 수명 I03xx)도 INFO(19)다. 콘솔이 ErrorList 의 HMI 클래스로 Task 를 따로 거른다.
- **로그전용 Info 행(I51xx..I55xx)은 v2 행이 아니다.** 변경 내역의 `catalog : NAME` 이 가리키는 카탈로그 이벤트를
  **지금 인코딩 그대로** 쓴다(STEP 은 Code = 새 step 이라 번호를 실을 자리가 없다). 콘솔은 `errorlist.json` 의
  `catalog` 로 그 행을 Info 유형 · 코드로 읽는다.
- Operator / Info 행이 `catalog : NAME` 을 가지면(예: O0101 ← CMD_JOG Src=1, I0301 ← TASK_ACCEPTED) **v2 행이 그
  카탈로그 이벤트를 대신한다** — PLC 는 둘 다 쓰지 않는다. 옛 행은 같은 참조로 계속 그 유형으로 읽힌다.
- 알람에 합친 이벤트(ErrorList `알람 통합` 시트, 예: ILK_TIMEOUT → F3118/F3119 A = step)는 그 값이 알람 발생 행의
  A / B 다(`SetAlarmV`). 옛 `ALM_RAISED` · `ALM_CLEARED`(601/602)는 v2 ALARM 행이 대신한다.

## 4. 넘침 방지 (PLC)

| 규칙 | 값 | 동작 |
|---|---|---|
| 해제 지연 `T_off` | 2 s | 비트가 내려가면 T_off 를 잰다. 그 안에 다시 올라가면 행을 쓰지 않고 `flicker += 1`. T_off 가 차면 해제 행(A = flicker, B = 발생부터 마지막 하강까지 ms). 해제 행의 시각은 하강 + 2 s |
| 코드별 상한 | 5 행 / 60 s | 한 코드의 발생 · 순간 행이 창 안에서 5 를 넘으면 쓰지 않고 센다. **발생을 쓴 항목의 해제는 늘 쓴다**, 발생을 억제한 항목은 그 해제도 억제(셈에 넣는다) |
| 순간형 중복 | 1 s | 같은 코드의 순간 행이 1 s 안에 다시 오면 쓰지 않고 센다 |
| 억제 요약 | 창이 끝날 때 | 센 것이 있으면 `Trans` 4 한 행: A = 억제 건수, B = 0, Src = ALARM 은 영역 · 그 밖은 0 |

억제 셈은 코드마다 따로다. 부팅 직후(OB100)에는 열린 발생이 없다 — 이미 켜진 비트는 첫 스캔에 발생으로 쓴다.
콘솔은 새 epoch(재부팅) 때 열린 알람을 모두 닫는다.

## 5. 예

| 무엇 | Cat | Lvl | Src | Code | A | B |
|---|---|---|---|---|---|---|
| F3119 발생 (step 400 인터락, 알람 통합: A = step) | 6 | 4 | 1 | 13119 | 400 | 0 |
| F3119 해제, flicker 2, 4.2 s | 6 | 4 | 1 | 23119 | 2 | 4200 |
| F0101 발생 (붙일 값 없음 → B = step) | 6 | 4 | 1 | 10101 | 0 | 300 |
| W1101 발생 (A = LagError ×10) | 6 | 3 | 2 | 11101 | 2150 | 0 |
| W1101 억제 요약 12 건 | 6 | 3 | 2 | 41101 | 12 | 0 |
| O0101 X Jog Speed 전진 시작 (A = 시작 위치 ×10) | 18 | 2 | 0 | 10101 | 5984 | 0 |
| O0101 종료 3.0 s | 18 | 2 | 0 | 20101 | 0 | 3000 |
| I0301 작업 수락 (Src = task type) | 19 | 2 | 16#50 | 30301 | Cell.Id | WorkId |
| I0108 Mode FAULT (순간) | 19 | 2 | 0 | 30108 | 16#80 | 16#20 |
| I5101 Step 변경 (로그전용 → 카탈로그 그대로) | 3 | 2 | ProcNo | 새 step | 체류 ms | 이전 step |

## 6. PLC 쪽 준비

- `EVT_Const_Gen` 다시 생성(`just gen-evt`): `EVT_CAT_OPERATOR` 18, `EVT_CAT_INFO` 19, `EVT_TRANS_*`,
  `EVT_TRANS_MUL`, **`EVT_CATMASK_ALL` = 16#000FFFFE**. `EVTLOG.Cfg.CatMask` 의 시작값이 옛 16#0003FFFE 로 남으면
  카테고리 18 · 19 가 걸러져 Operator / Info 가 **하나도 기록되지 않는다** — 다운로드 뒤 콘솔 로거 설정에서도 확인.
- 한 줄 쓰기의 모양: `"EvtWrite"(Cat := …, Lvl := …, Src := …, Code := Trans * "EVT_TRANS_MUL" + N, A := …, B := …)`.
  상태형 항목은 비트 배열(ALARM.FAULT / WARN / EVENT / INFO, GRM FAULT / WARN / EVENT DB · INFO DB803)을 이전값과
  비교하는 한 곳에서 4 절 규칙으로 쓰는 것이 단순하다 — 호출 지점마다 로그를 부르지 않는다.
- HMI Operator / Info / Task 이산 알람 초안: `plc/generated/hmi/<unit>-{operator,info,task}.json`
  (Id = 200000 + 20000 × (GR1 0, GR2 1, GRM 2) + (Operator 10000 | Info · Task 20000) + 번호, 예 GR1 O0105 = 210105).

## 7. 콘솔

- `Cat` 6 이고 `Code ≥ 10000` 이면 v2, 아니면 옛 카탈로그 행(ALM_RAISED 601 등, `Src` = 영역 · `A` = 비트 →
  `alarms.json`). 18 · 19 의 카탈로그 코드는 없다.
- 유형(Type): v2 행은 레벨, Info 중 HMI 클래스 TASK 는 Task. 옛 ALARM 행은 영역(1 Alarm, 2 Warn, 3 Operator,
  4 TASK 영역 → Task). 그 밖의 카탈로그 행은 `errorlist.json` 의 `catalog` 참조(모두 같은 유형이거나 `Src=N` 이 하나를
  고를 때만).
- 문장: ErrorList 문구(UI 언어 KO / EN) + 전이(발생 · 해제 · 반복 억제) + 값. 값의 모양은 ErrorList 값 칸에서 짐작하고
  (`시작 위치 (mm×10)` → `시작 위치 598.4 mm`), 필요한 것만 `plc/evtlog/errorlist-format.toml` 에 적는다
  (`enum:task_type` · `div:10 mm` · `hex` · `onoff` · 영어 라벨). 코드는 문장에 넣지 않고 Type 열이 보여 준다.
- 알람 통계: v2 는 (PLC, 레벨, 번호)로 발생 ↔ 해제를 짝짓고 길이는 해제 행의 B, 옛 행은 (PLC, 영역, 비트).
  옛 FAULT Reset(ALM_RESET)은 옛 행만 닫는다 — v2 는 자기 해제 행을 쓴다.
- 알림 규칙: `types`(Alarm …) · `trans`(raise …) · `codes` 에 ErrorList 코드(`F0202`). 기본 규칙 = Alarm 발생 · EMS.

## 8. 열린 질문

1. 로그전용 Info 를 v2 로 옮길지(지금은 카탈로그 그대로, 레벨도 카탈로그 값). 옮기면 STEP 처럼 Code 에 값을 싣는
   이벤트는 A/B/Src 로 옮겨야 하는데 자리가 모자란다.
2. `Lvl` DEBUG 를 쓰는 v2 항목이 필요한가 — HMI 비표시(N) Operator 행(O0801…)도 지금은 INFO.
3. 코드별 상한(5 / 60 s)과 순간 중복(1 s)을 코드마다 다르게 둘 필요가 있는가(예: 조그 버튼).
