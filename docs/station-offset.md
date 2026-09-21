# 스테이션 보정 (Station offset)

콘솔이 스테이션 대상 태스크를 보낼 때 GRM `StationCenterAdjust` 와 **같은 식**으로 컨베이어 방향(RotateType)
과 레이저 측정 트래킹을 X·Y 에 더한다. 계산은 `apps/gr-console/src/issue/station_offset.rs`(순수 함수 + 테스트),
스냅샷 읽기와 적용은 `issue/mod.rs`, 제출 시 재계산은 `ledger/ops.rs`.

## 왜 필요한가

- GRM 은 **자기 시나리오 경로**(`RobotScenarioTask` → `StationCenterAdjust`)로 보내는 태스크에만 보정을 더한다.
- 콘솔이 GRM `OPCUA.GR[n].CMD` 에 쓰는 태스크는 GRM 이 그대로 중계하고, GR2 는 `UL_ParseOpcUaCommand` →
  `PL_Task_V2` 에서 `Task.Position` 으로 곧장 간다.
- 그래서 보정 전 콘솔은 스테이션 태스크를 `Info.Position` 만으로 보냈다 — 타이어 중심이 아니라 스테이션
  기준점으로 간다.

## PLC 원문 (siemens/export)

| 무엇 | 파일 |
|---|---|
| 측정 → 트래킹(`OutterDiameter = Info.Width − (Lmin+Rmin)`, 흐름 가로 오프셋) | `GRM_PLC/blocks/MeasuringStationOffset.scl` |
| 앞 스테이션 → 이 스테이션 인계, 픽 완료 시 Now → Last 후 0 | `GRM_PLC/blocks/FB_CL_Station.scl` |
| 트래킹 검증(300 < OD ≤ 1100, \|TX\|,\|TY\| ≤ 300) | `GRM_PLC/blocks/isValidTrackingData.scl` (AllowOffsetRange 300 은 FB_CL_Station) |
| 보정 적용 | `GRM_PLC/blocks/StationCenterAdjust.scl` (호출: `304. Scenario/RobotScenarioTask.scl`) |
| `OPCUA.STATION[n]` ← `STATION.Station[n]` 매 스캔 복사 | `GRM_PLC/blocks/801. Communication/Comm_CV.scl` |
| GR2 영역 검증(410-419) | `GR2_PLC/blocks/isValidTaskArea.scl` |
| 슬롯 번호 | `isStationTask.scl` — `StationNo := workCell MOD 100` (STATION_MIN 1 .. STATION_MAX 32) |

## 식

`OD = GRM STATION.Station[n].Tracking.Now.OutterDiameter`, `Now = …Tracking.Now.TaskOffset`, `n = id MOD 100`.

| RotateType (GRM Para) | TX'                | TY'                |
|---:|---|---|
| 1, 5 | Now[X]       | −OD × 0.5 |
| 2, 6 | Now[X]       | +OD × 0.5 |
| 3, 7 | +OD × 0.5    | Now[Y]    |
| 4, 8 | −OD × 0.5    | Now[Y]    |
| 0 / 그 외 | Now[X]  | Now[Y]    |

- `X = Info.X + TX'`, `Y = Info.Y + TY'`. **Z·G 는 건드리지 않는다**(콘솔 규칙 그대로: Z 적층, G = 내경 − 30).
- 위 표는 **측정 트래킹이 있을 때**(`od_source: "tracking"`). 측정값이 없으면 아래 품목 스펙 폴백.
- 적용 종류: PICK · DROP · MEASURE. (GRM 은 모든 종류에 더하지만 콘솔은 MOVE·UP 에 붙이지 않는다.)
- `position_override` 가 있으면 보정하지 않는다 → 감사 기록 `mode: "override"`.
- 요청 `station_offset: "off"`(또는 `false`) → 보정하지 않는다 → `mode: "off"`. 기본은 켜짐(`"auto"`).
- 콘솔은 **아무것도 PLC 에 쓰지 않는다**(GRM 은 `Now.TaskOffset` 에 ±OD/2 를 되써 넣는다 — 아래 열린 질문 e).

### 측정값이 없을 때 — 등록 품목 스펙 폴백

`Info.Position` 은 타이어 중심이 아니라 **벽/스토퍼 기준점**이다. 그래서 측정 트래킹이 없다고 보정을 아예
빼면 로봇이 벽으로 간다. 측정 OD 를 못 읽으면(스냅샷 없음 · 슬롯 Id 불일치 · `Now.OutterDiameter = 0`)
**등록 품목 스펙의 `OuterDiameter`** 로 대신 보정한다.

| RotateType | TX'       | TY'       |
|---:|---|---|
| 1, 5 | 0         | −ItemOD × 0.5 |
| 2, 6 | 0         | +ItemOD × 0.5 |
| 3, 7 | +ItemOD × 0.5 | 0     |
| 4, 8 | −ItemOD × 0.5 | 0     |
| 0 / 그 외 | 0    | 0         |

- **GR2 `isValidTaskArea` 의 StationCenterOffset 과 완전히 같은 식**이다(`gr2_center_offset` 하나를 폴백과
  영역 선검사가 같이 쓴다) → 폴백으로 만든 위치는 GR2 영역 검사에서 거리 0 으로 통과한다.
- 흐름 **가로축 오프셋은 0** 이다 — 그 값은 레이저가 재는 값이라 측정 없이는 알 수 없다.
  (측정 OD 가 0 인데 `Now.TaskOffset` 이 남아 있으면 경고만 하고 쓰지 않는다.)
- RotateType 0/정의 밖이면 보정 0 + 경고(GR2 도 보정을 기대하지 않는다).
- 품목 외경도 0 이면 보정 없이 예전 규칙대로 **막는다**(아래 가드).
- GRM 자기 시나리오 경로는 여전히 **측정 OD** 로 옮긴다 → 같은 스테이션이라도 GRM 이 보낸 태스크와
  콘솔이 보낸 태스크가 **측정 OD − 공칭 OD** 만큼 다를 수 있다(보통 몇 mm, 마진 300/650 안).
- 감사 기록: `od_source`(`"tracking"` | `"item_spec"` | `"none"`), `od_used`(실제로 쓴 외경).
  `od` 는 **측정값 그대로**(폴백일 때 0).

## 데이터 출처

1. GRM `OPCUA` DB `STATION[n]` (fast tier 500 ms) — 그 원소의 `Para.Info.Id` 가 대상 id 일 때.
2. 아니면 GRM `STATION` DB `Station[n]` (slow tier).
3. 둘 다 Id 가 다르면 "슬롯 Id 불일치 — 스테이션 푸시 필요", 스냅샷이 없으면 "GRM 스냅샷 없음".
4. 1-3 이 실패하거나 `Now.OutterDiameter = 0` 이면 **태스크 품목의 `Item.OuterDiameter`**
   (`Composed.task.item.outer_diameter` — 콘솔 품목 레지스트리/스펙) → `od_source: "item_spec"`.
5. 품목 외경도 0 이면 `od_source: "none"` — 보정 없음.

RotateType 은 GRM `Para` 값을 쓰고, 콘솔 레지스트리 값과 다르면 경고한다. 스냅샷 시각·나이(`age_ms`)를 결과에
싣고 5 초보다 오래되거나 GRM S7 연결이 끊겼으면 경고한다.

## 가드

| 조건 | 결과 |
|---|---|
| 측정 OD 를 못 읽음(스냅샷/슬롯 실패 또는 OD = 0) **이고 품목 외경 > 0** | 품목 스펙 폴백 + 경고("GRM 측정값 없음 — 등록 품목 OuterDiameter …"). **막지 않는다** |
| 측정 OD 도 품목 외경도 없음(OD = 0 && Item.OuterDiameter ≤ 0) 이고 스테이션이 센서(IOLinkMasterModule ≠ 0 && 포트 ≠ 0) 또는 트래킹(Group ≠ 0 && ConnectionPrev ≠ 0) — PICK·MEASURE | **제출 거부**(409). 보정을 끄면 통과 |
| GRM 스냅샷/슬롯을 못 읽고 품목 외경도 없음 — 위와 같은 스테이션의 PICK·MEASURE | **제출 거부** |
| OD ≠ 0 인데 `isValidTrackingData` ≠ 0 (OD 범위·오프셋 300 초과) — PICK·DROP·MEASURE | **제출 거부** |
| `Status.MeasuringError` / `Status.DataMissMatch` | 경고 |
| `Tracking.Staged` ≠ 0 (보류 측정값) | 경고 |
| RotateType GRM ≠ 레지스트리, 스냅샷 오래됨, GRM 연결 끊김, OD = 0 인데 TaskOffset 남음 | 경고 |
| GR2 영역 선검사: `|X − (Info.X + cx)| < 마진`, `|Y − (Info.Y + cy)| < 마진`, `Z > Info.Z` (cx·cy = 품목 외경/2, 부호는 RotateType 표와 같음; 마진 650 if GR2 `STATION.Station[n].TaskType ≠ 0` else 300) | 경고(GR2 거부 411/412/413 예상) |
| GR2 `STATION` 에 그 Id 가 없음 | 경고(419 예상) |

미리보기(`POST /api/issue/compose`)는 거부 사유를 `station_offset.blocked` 와 경고로 보여 주기만 한다.
실제 거부는 `POST /api/tasks`(바로 제출) · `POST /api/tasks/{id}/submit` · 재제출 · 시나리오 스텝 제출에서.

## 제출 시 재계산

트래킹은 컨베이어가 돌면서 바뀌고 픽 뒤에 지워진다. 그래서 작성 때 계산한 위치를 믿지 않는다.

- `ledger::ops::create_and_submit` (작성 라우트, 시나리오 러너 — 게이트 대기가 끝난 뒤, 재제출) 과
  `ledger::ops::submit` (초안 제출) 이 `issue::refresh_station_offset` 을 불러 **레지스트리 Info + 지금 스냅샷**
  으로 X·Y 를 다시 쓴다. Z·G 는 작성 때 값.
- 바로 제출에서 거부되면 원장에 초안을 남기지 않는다. 초안 만들기(`?submit=false`)는 거부 없이 기록만 한다.

## API · 원장

- `TaskRequest.station_offset`: `"auto"`(기본, 직렬화 생략) | `"off"`; `true`/`false`/`null` 도 받는다.
- `Composed.station_offset` / `LedgerEntry.station_offset` (원장 `doc_json` 에 저장 — 마이그레이션 없음):

```json
{ "station_id": 2102, "slot": 2, "rotate_type": 6, "od": 638.6, "od_used": 638.6, "od_source": "tracking",
  "now_tx": -121.5, "now_ty": 0,
  "tx_applied": -121.5, "ty_applied": 319.3, "base_xy": [7008.5, 6756.5], "final_xy": [6887.0, 7075.8],
  "source": "OPCUA", "snapshot_at": "2026-09-14T13:54:08+09:00", "age_ms": 180, "mode": "auto",
  "warnings": [], "blocked": null, "gr2_expected_xy": [7008.5, 7076.5], "gr2_margin": 300 }
```

품목 스펙 폴백이면 `"od": 0, "od_used": 640, "od_source": "item_spec", "tx_applied": 0, "ty_applied": 320`
(가로축 0). 예전 원장 기록에는 두 필드가 없다 — 화면은 `od` 유무로 `tracking`/`none` 을 추정한다.

화면: 작업 작성 카드의 "스테이션 보정" 블록(끄기 스위치), Task 상세의 "스테이션 보정" 섹션(제출 시점 기록).
둘 다 `OdSource`(Tracking / ItemSpec · 쓴 외경)를 `Applied TX / TY` 옆에 보이고, 폴백이면 경고색과
한 줄 안내가 붙는다.

제출 시 재계산은 폴백에도 그대로 적용된다 — 작성과 제출 사이에 타이어가 측정되면 그때는 측정값
(`od_source: "tracking"`)으로 다시 계산된다.

## 슬롯 규칙과 레지스트리 푸시

PLC 는 스테이션을 **`STATION.Station[id MOD 100]`** 으로 읽는다(`isStationTask`, `StationCenterAdjust`,
GR2 `isValidTaskArea` 의 TaskType, `PL_Task_V2`). `Findindex_STATION` 은 `STATION_MIN..STATION_MAX` 전체를 Id 로 찾는다.

- `registry/plc_io.rs push_stations` 는 각 스테이션을 슬롯 `id % 100` 에 쓰고 나머지 슬롯은 0 으로 채운다.
  슬롯이 1..32 밖(`MOD 100` = 0 또는 > 32)이거나 두 id 가 같은 슬롯(2003 & 2103)이면 400 으로 거부.
  `Count` 는 여전히 스테이션 개수.
- 가져오기/비교는 `Count` 와 무관하게 배열 전체를 읽어 Id 로 맞춘다(슬롯 21 이 Count 뒤에 있을 수 있다).
- 셀은 슬롯 규칙이 없다: `Findindex_CELL` 은 `CELL_MIN..CELL_MAX` 를 보지만 GR2 `PL_Task_V2`(BlendUse)와
  `HMI_CellPos` 가 `CELL_MIN..CELL.Count` 를 돈다 → 앞에서부터 채우고 Count 를 쓰는 지금 방식이 맞다.
- 참고: PLC 의 `AddStation` 은 `Station[Count + 1]` 에 붙인다 — HMI 로 추가하면 슬롯 규칙과 어긋날 수 있다.

## 열린 질문

a. **RotateType 5-8 그림이 서로 다르다.** `isValidTaskArea` 와 `MeasuringStationOffset`/`StationCenterAdjust`
   주석의 5-8 방향 라벨(X+/X−, Y+/Y−)이 어긋난다. 코드의 CASE 는 1↔5, 2↔6, 3↔7, 4↔8 을 같은 부호로
   묶으므로 **코드를 정본으로** 삼았다(측정 식만 5-8 에서 Left/Right 가 뒤집힌다).
b. **GCS(상위) 도 `Tracking.Now` 를 더하는가?** GRM 이 GCS 명령을 중계할 때 보정하는 코드는 찾지 못했다.
   GCS 가 직접 더한다면 콘솔과 같은 규약인지 확인 필요.
c. **인계는 앞 스테이션 오프셋을 그대로 복사한다**(`FB_CL_Station`: `Now.TaskOffset[X/Y] := Prev.Now…`).
   앞·뒤 스테이션의 RotateType 축이 다르면(예: 1 → 3) 흐름 가로 오프셋이 엉뚱한 축에 남는다.
d. **GR2 는 품목 외경(`Item.OuterDiameter`)으로 검증하고 GRM 은 측정 OD 로 옮긴다.** 둘의 차이가 마진
   (300/650)을 넘으면 정상 보정도 411/412 로 거부될 수 있다. 콘솔은 경고만 한다.
   (콘솔 폴백은 GR2 와 같은 품목 외경을 쓰므로 이 차이가 없다.)
e. `StationCenterAdjust` 는 ±OD/2 를 `Tracking.Now.TaskOffset` 에 **되써 넣는다**. OD > 600 이면 그 값이 300 을
   넘어 이후 `isValidTrackingData`(AllowOffsetRange 300)가 실패 → `MeasErr` 가 설 수 있다(GRM 경로 한정).
f. ~~DROP 은 보통 트래킹이 없어(OD 0) Info 위치로 간다~~ → **품목 스펙 폴백으로 해결**(2026-09-18).
   DROP 도 `Info ± 품목 외경/2` 로 가므로 GR2 영역 검사와 어긋나지 않는다.
g. 폴백은 **공칭 외경**이라 실제 타이어가 크게 다르면 벽 쪽으로 치우친다. 센서가 있는 스테이션에서는
   측정이 끝난 뒤 집는 것이 여전히 정답 — 폴백은 "벽으로 가는 것"보다 나은 차선책이다.
