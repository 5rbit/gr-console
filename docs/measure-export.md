# 측정 이력 Excel 규격 (`measlog-xlsx/1`)

측정 모니터 → 기록 탭의 **Excel** 버튼, 또는 `GET /api/measlog/export.xlsx`.
콘솔 DB 에 쌓인 **전체** 이력에서 조건에 맞는 기록을 내보낸다(화면 표는 최근 N 건만 들고 있다).

| 쿼리 | 뜻 |
|---|---|
| `robot` | 로봇 id (없으면 기본 로봇) |
| `kind` | 1 Item · 2 Sku · 3 Floor · 4 Pick · 5 Manual |
| `code` | 품목 코드 |
| `from`, `to` | 날짜 `YYYY-MM-DD`, 양끝 포함 (PLC TimeStamp 기준) |

파일 이름 `measlog_<PLC>_<YYYYMMDDhhmmss>.xlsx`. 행 한도 100 000 건, 넘으면 Info 의 `Truncated`.

## 시트 (항상 이 순서, 기록이 없어도 머리글은 있다)

| 시트 | 내용 |
|---|---|
| Info | Format · PLC · Robot · ExportedAt · 필터 · 행 수 · Seq 범위 · Truncated, 시트별 행 수 |
| All | 모든 기록 — 공통 열 + `Kind` + Delta. Data 칸은 없음 |
| Item · Sku · Floor · Pick · Manual | 그 종류 기록 — 공통 열 + **이름 붙은 Data 열** + Delta |
| Legend | 모든 시트의 열 → 원천(`MEASLOG_HIST.Entry` 경로) · 단위 · 설명, Status 코드표, SKU DiagFlags 비트표 |

## 규칙

- 행은 `Seq` 오름차순(옛것 → 최신). 1 행 머리글, 필터·틀 고정 걸림.
- 머리글 = PLC 멤버 이름 그대로 + 단위: `InnerDia (mm)`, `Stack[3].UpperBead (mm)`.
- `TimeStamp` 는 Excel 날짜 값(`yyyy-mm-dd hh:mm:ss.000`)이라 바로 정렬·그래프·차 계산이 된다.
- 숫자는 숫자 칸. 표시는 mm 소수 2 자리지만 값은 PLC 값 그대로.
- Status 는 코드(`Status`) · 이름(`StatusName`) · 뜻(`StatusText`) 세 열.
- SKU `DiagFlags` 는 비트 합 숫자 + `DiagText`(켜진 비트 이름). 예비 Data 칸은 내보내지 않는다.
- 공통 열: Seq, TimeStamp, (All 만 Kind), Status, StatusName, StatusText, WorkId, TaskId, TaskType,
  CellId, Code, Cmd.Count, Cmd.InnerDiameter, Cmd.OuterDiameter, Cmd.Height, Cmd.X/Y/Z/G, Cell.Z, Cmd.ZRel, Flags.
  끝에 Delta.InnerDia · Height · Z · Offset · Count.

## 종류별 Data 열

| 시트 | 열 (Data 번호) |
|---|---|
| Item | InnerDia 1 · UpperBeadHeight 2 · TireHeight 3 · Torq_InnerDia 7 · Torq_Factor 8 · In.InnerDia/UpperBeadHeight/TireHeight/OffsetX/OffsetY 10~14 · Out.같은 5 개 15~19 |
| Sku | Stack[1..7].LowerBead/UpperBead 1~14 · DiagFlags 15 (+DiagText) · LayerOffsetMax 16 · StackCount 17 · EachHeight 18 · StackHeight 19 |
| Floor | FloorHeight 1 · Z.Position 2 · FLD.Distance 3 |
| Pick | LastBeadPos-Target 1 · PickZTarget 2 · Valid 3 · FitError 4 · InnerDia 5 · OffsetX 6 · OffsetY 7 · UpperBeadHeight 8 · TireHeight 9 |
| Manual | LaserInnerDia 1 · TorqueInnerDia 2 · TireHeight 3 · OffsetX 4 · OffsetY 5 · UpperBeadHeight 6 · FitError 7 |

## 판 올리기

정의는 `apps/gr-console/src/measure/xlsx.rs` 한 곳이고 화면 상세의 `apps/gr-web/src/lib/meas/dataFields.ts` 와 같아야 한다.
PLC `MeasureLogAdd` 의 Data 칸이 바뀌면 둘 다 고친다. 열을 빼거나 뜻을 바꾸면 `FORMAT` 을 `/2` 로 올린다.
끝에 열을 더하는 것은 판을 올리지 않는다(기존 수식·피벗이 깨지지 않는다).
