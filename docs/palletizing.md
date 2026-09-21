# 팔렛타이징 패턴 (사양서 R4)

사양서 `[GR] HKT TP2 팔렛타이징 사양서 R4 251213.pptx`의 팔렛 패턴을 콘솔 데이터로 옮기고, 그 데이터로 슬롯
위치·드래그 방향을 만들어 작업 명령(compose)·계획·시나리오에 쓰는 방법을 적는다. 필드 이름은 데이터 이름 그대로
(영문) 쓴다.

- 공장 기준본: `apps/gr-console/src/pallet/spec_r4.json` (바이너리에 `include_str!`로 내장). **생성에는 직접 쓰지
  않는다** — 편집 저장소의 첫 채우기·초기화·비교 기준이다(10절).
- 편집 저장소: sqlite `pallet_flow` · `pallet_pattern` (마이그레이션 `0006_console_v2`). 생성기·계획·작업 명령은
  이 표를 읽는다.
- 기준본 생성기: `tools/pallet/gen_spec.mjs` — `tools/pallet/shapes/shapes_slide{2..6}.csv`(`tools/pallet/pptx_shapes.ps1`로
  pptx에서 뽑은 도형 좌표)와 스크립트 안의 순서표 전사본으로 만든다. `node tools/pallet/gen_spec.mjs --check`는
  JSON이 스크립트 결과와 같은지 본다.
- 백엔드: `apps/gr-console/src/pallet/` (`mod.rs` 생성기 · `edit.rs` 편집 검증·비교·가져오기 · `store.rs` 저장소 ·
  `profiles.rs` 품목·로봇·스테이션 팔렛 설정 · `compose.rs` 작업 작성 통합 · `routes.rs` API)
- 화면: 탭 **팔렛 패턴** (`apps/gr-web/src/components/pallet/`, 순수 로직 `lib/pallet/model.ts` · `lib/pallet/editorModel.ts`)

## 1. 패턴

패턴 번호 = 한 단에 놓는 타이어 수. 팔렛은 1600W × 1600D. OuterDiameter로 패턴을 고른다.

| Pattern | OuterDiameter (mm) | 그려진 D | 최소 중심거리 | 그려진 Gap | 도형 무게중심 (X, Y) | 처리 |
|---|---|---|---|---|---|---|
| 2 | 814 ~ 937 | 853.8 | 933.0 | 79.1 | (0.0, 0.0) | 0 으로 맞춤 |
| 3 | 756 ~ 813 | 782.1 | 773.7 | **−8.4** | (42.4, −75.5) | **그대로 둠** |
| 4 | 663 ~ 755 | 782.1 | 793.1 | 11.1 | (−8.1, −1.9) | 0 으로 맞춤 |
| 5 | 601 ~ 662 | 585.4 | 608.0 | 22.6 | (−3.3, −3.1) | 0 으로 맞춤 |
| 6 | 546 ~ 600 | 512.0 | 525.8 | 13.8 | (−9.5, 0.7) | 0 으로 맞춤(경계값) |
| 8 | 521 ~ 545 | 512.0 | 508.9 | **−3.1** | (0.4, −0.2) | 0 으로 맞춤 |
| 9 | ~ 520 | 512.0 | 513.6 | 1.6 | (−3.2, −0.1) | 0 으로 맞춤 |

값은 HP 입고(기계 축, mm) 기준이다. 다른 흐름도 같은 크기이고, 무게중심은 흐름의 회전만큼 돈다(OP 출하·외부입고
P3 = (−42.4, 75.4)). 그려진 Gap이 음수인 P3·P8은 그림에서 원이 겹친다. 그래서 그려진 D가 아니라 최소 중심거리로
정규화한다(2절).

## 2. 정규화

1. 70.87 pt 둥근 사각형 하나 = 1600 mm 팔렛 한 장(1 pt = 22.577 mm). 타원(= 타이어)은 그 사각형 안에 있는 것만 그 패턴에 넣는다.
2. 사각형 중심에서 타원 중심까지를 **기계 축**으로 바꾼다(3절의 슬라이드별 부호).
3. 모든 쌍의 최소 중심거리로 나눠 `u`를 만든다 → 패턴마다 `min |u_i − u_j| = 1.0`.
4. 무게중심이 10 mm 안이면 0으로 맞추고, 아니면 그린 대로 둔다(P3만 해당).
5. 실제 오프셋 = `u × (OuterDiameter + Gap) / MinDistance(u)`, 슬롯 좌표 = 중심 + 오프셋. 사양서 패턴은
   MinDistance(u) = 1이라 `u × (OuterDiameter + Gap)`과 같고, 최소 중심거리가 곧 OuterDiameter + Gap이다.

Gap은 운전자가 생성할 때 넣는다(기본 **50 mm**). 위 표의 그려진 Gap은 참고값이고 기본값으로 쓰지 않는다.

**편집 뒤 정규화 규칙.** 운전자가 슬롯을 옮겨 MinDistance(u)가 1에서 벗어나도 콘솔은 u를 **자동으로 고치지 않고
그대로 저장**한다. 대신 생성기가 오프셋에 `1 / MinDistance(u)`를 곱해, 가장 가까운 두 타이어의 중심거리가 늘
OuterDiameter + Gap이 되게 맞춘다. 그래서 편집으로 모양(비율)은 바뀌어도 겹침·간격 규칙은 그대로다.

- 저장·목록·계획 응답에 MinDistance를 싣는다(`min_distance`, 계획은 `min_distance_norm`과 배율 `scale`).
- 1과 0.001 넘게 다르면 경고만 하고 저장한다. 편집 화면은 생성 결과를 점선 원으로 겹쳐 보인다.
- 편집 화면의 "MinDistance 1" 버튼은 u를 나눠 1로 맞춘다. 생성 결과는 같고 u 값만 사양서 방식이 된다.
- 슬롯이 1개면 MinDistance가 없으므로 배율 1(`u × (OuterDiameter + Gap)`)이다.
- 두 슬롯이 0.01 안에 겹치면(같은 자리) 저장하지 못한다.

## 3. 좌표와 드래그 방향

### 슬라이드 화면 → 기계 축

| 슬라이드 | 화면 오른쪽 | 화면 아래 | 기계 dx, dy |
|---|---|---|---|
| 2, 4, 5, 6 | X− | Y− | −screen_dx, −screen_dy |
| 3 | X+ | Y+ | +screen_dx, +screen_dy |

각 슬라이드에 그려진 축 화살표로 확인했다. 흐름마다 `screen_axes`로 저장하고, 화면의 "사양서 방향" 보기가 이
값을 쓴다.

### DragDir 코드 (PLC FC `DragDelta`)

| DragDir | 변위 | DragType 바이트 | DragDelta X | DragDelta Y |
|---|---|---|---|---|
| 1 | X+ Y− | 0x01 | +Dist/√2 | −Dist/√2 |
| 2 | X+ | 0x02 | +Dist | 0 |
| 3 | X+ Y+ | 0x04 | +Dist/√2 | +Dist/√2 |
| 4 | Y− | 0x08 | 0 | −Dist |
| 5 | Y+ | 0x10 | 0 | +Dist |
| 6 | X− Y− | 0x20 | −Dist/√2 | −Dist/√2 |
| 7 | X− | 0x40 | −Dist | 0 |
| 8 | X− Y+ | 0x80 | −Dist/√2 | +Dist/√2 |

- DragType 바이트는 `1 << (DragDir − 1)`(`LGR_Const_Task_DragType`)이고, 0은 드래그 없음이다.
- 슬라이드의 범례 격자는 그 슬라이드 화면 방향으로 그려져 있고, 숫자가 놓인 칸이 그 코드의 변위 방향이다. 예: 슬라이드 2에서 "1↗"은 왼쪽 아래 칸 = X+ Y−.
- 화살표 글리프는 드래그 동작을 보여 준다. Drag-in은 슬롯 쪽으로 들어오고, Drag-out은 놓은 뒤 빠진다.
- 원 글자 "C#2 3↘"의 숫자와 순서표 "2(p3)"의 숫자는 같은 코드다.
- 화면의 DragDir 격자와 vitest `dirGrid`가 슬라이드 2와 3의 범례를 그대로 재현하는지 검사한다.

## 4. 흐름

| id | 슬라이드 | drag_kind | 작업 | 로봇 · 존 | 비고 |
|---|---|---|---|---|---|
| `HP_IN` | 2 (=4) | in | PICK + Drag-in | GR1(HP) · 입고1(422) / 입고2(391) | |
| `OP_IN` | 2 (=4) | in | PICK + Drag-in | GR2(OP) · 입고1 / 입고2 | |
| `OP_OUT` | 5 | out | DROP + Drag-out | GR1(OP) · 출하1(115) / 출하2(116) / 출하3(116) | P3는 붉은 박스 수정 순서 |
| `OP_EXT_IN` | 6 | in | PICK + Drag-in | GR1(OP) · 외부입고(117) | |
| `IN1_ALT_HP` | 3 | in | PICK + Drag-in | GR1(HP) · 입고1 대안 셀 | P4만. 기계 축으로 HP_IN P4와 같다 |
| `IN1_ALT_OP` | 3 | in | PICK + Drag-in | GR2(OP) · 입고1 대안 셀 | P4만. OP_IN P4와 다르고 OP_IN_S5 P4와 같다 |
| `OP_IN_S5` | 5 (흐린 칸) | in | PICK + Drag-in | ? | `reference: true` — 참고본(7절) |

- 순서표 "순서"는 작업 순서다. `1 → 2(p3) → 3(p1)` = Seq 1은 C#1(드래그 없음), Seq 2는 C#2(DragDir 3), Seq 3은 C#3(DragDir 1).
- 순서표와 원 글자가 다르면 원 글자를 따르고 `notes`에 남긴다.
- 모든 흐름·패턴에서 입고는 첫 Seq가, 출하는 마지막 Seq가 드래그 없음이다(Rust 테스트가 검사한다).
- 점 집합은 흐름끼리 같다. 회전·미러를 허용하면 HP_IN과 잔차 ≤ 0.0004다. OP_OUT·OP_EXT_IN의 P3·P5·P6·P8·P9는 HP_IN을 180° 돌린 모양이다.

## 5. 생성기 · 검사

`generate(flow, pattern | auto, OuterDiameter, Gap, Rotation, MirrorX, MirrorY, center, PalletSize)`는 슬롯마다
`{Seq, Slot, u, OffsetX, OffsetY, X, Y, spec DragDir, DragDir, DragType, overhang}`을 만든다.

- 변환 순서: MirrorX(X → −X)와 MirrorY(Y → −Y)를 먼저 하고, Rotation(0/90/180/270, X+에서 Y+ 쪽으로)을 한다. 오프셋과 DragDir을 **같은 변환**으로 돌린다.
- 16가지 변환 × 8방향이 DragDelta 결과까지 일치하는지 Rust와 vitest가 같은 표로 검사한다.
- 팔렛 밖(`|Offset| + OuterDiameter/2 > PalletSize/2`, 축마다)이면 경고다.
- 최소 중심거리 < OuterDiameter(Gap < 0)이면 오류다.
- Pattern을 직접 골랐는데 OuterDiameter가 범위 밖이면 경고다. 참고본 흐름도 경고를 낸다.
- **auto 패턴 선택은 흐름마다 그 흐름의 패턴 OdMin/OdMax로** 한다(편집으로 흐름마다 달라질 수 있다). "OdMin 이상인
  가장 큰 패턴"을 고르고, 그 OdMax를 넘으면 다음 패턴과의 틈이 1 mm 이하(정수 경계 813 / 814)일 때만 그 패턴으로
  본다. 더 넓은 틈에 든 외경은 `no pattern for OD …` 400이다.
- **GR2 isValidTaskArea 선검사**는 PLC와 같은 식이다.
  - 기대 중심 = `Info.Position + RotateType별 ±OuterDiameter/2`.
  - 마진 = 스테이션 TaskType ≠ 0이면 650, 아니면 300.
  - 축마다 `|X − 기대| < 마진`이어야 통과하고, 아니면 411(X)·412(Y) 거부를 예고한다.
  - 판정은 GR2가 한다.
- 단별 Z는 기존 스택 규칙(`stack_z_with`, 품목 비드 프로파일 포함)을 그대로 쓴다. 단 L의 스택 개수는 DROP이면 `L − 1`, PICK/MEASURE면 `L − 1 + count`다.

## 6. 팔렛 설정 귀속 · API

**팔렛 패턴은 품목에 속한다**(결정 2026-09-21). 설정은 셋으로 나뉜다(마이그레이션 `0007_pallet_by_item`).

| 어디 | 표 | 무엇 | 이유 |
|---|---|---|---|
| 품목 | `pallet_item` | 입고/출하 Flow · Pattern · Gap · 배치 Rotation/Mirror · PalletSize · Note | 패턴은 제품이 정한다. 위치 좌표는 GR1·GR2 공통 |
| 로봇 | `pallet_robot` | 드래그 방향 Rotation/Mirror | 헤드 방향이 로봇마다 달라 드래그 인/아웃 방향만 다르다 |
| 스테이션 | `pallet_station` | 팔렛 스테이션 여부(`enabled`) · Note | 어느 스테이션이 팔렛인가 |

옛 스테이션별 `pallet_profile`은 지웠다(운용 DB에 행이 없었다). 켜짐 여부만 `pallet_station`으로 옮긴다.

현장에 팔렛 스테이션은 **아직 없다**. 운전자가 팔렛 패턴 화면에서 스테이션을 팔렛 스테이션으로 켜야 그 스테이션에
슬롯이 쓰인다. 그 전에는 모든 스테이션이 기존 동작 그대로다.

`pallet_item` 필드:

| 필드 | 기본값 | 범위 |
|---|---|---|
| `code` | — | 등록된 품목 |
| `flow_in` | 없음 | PICK/MEASURE 에 쓰는 흐름. 없으면 `flow_out` |
| `flow_out` | 없음 | DROP 에 쓰는 흐름. 없으면 `flow_in` (둘 중 하나는 있어야 한다) |
| `pattern` | 없음 = OuterDiameter 자동 | 두 흐름 모두에 있는 번호 |
| `gap` | 50 | 0..500 mm |
| `rotation` | 0 | 0/90/180/270 |
| `mirror_x` · `mirror_y` | false | |
| `pallet_size` | 1600 | 500..4000 mm |
| `note` | 빈 문자열 | |

`pallet_robot` 필드: `robot`(설정된 로봇 id), `rotation`(0/90/180/270), `mirror_x`, `mirror_y`, `note`.
저장 안 한 로봇은 변환 없음이다. 슬롯의 최종 DragDir = 로봇 변환(품목 배치 변환(사양 DragDir))이고, 좌표에는
로봇 변환을 걸지 않는다.

| API | 설명 |
|---|---|
| `GET /api/pallet/spec` | 편집 저장소를 사양 JSON 모양으로(`seed: false`). `?seed=1`이면 내장 사양서 R4(`seed: true`) |
| `GET /api/pallet/items` · `PUT /api/pallet/items` · `DELETE /api/pallet/items/{code}` | 품목 팔렛 패턴. 품목 미등록이면 404, 검증 실패면 400 |
| `GET /api/pallet/robots` · `PUT /api/pallet/robots` | 로봇 드래그 방향. GET 은 설정된 로봇마다 한 줄(`name`, `plc` 포함) |
| `GET /api/pallet/stations` · `PUT /api/pallet/stations` · `DELETE /api/pallet/stations/{station}` | 팔렛 스테이션 여부. 스테이션 미등록이면 404 |
| `GET /api/pallet/plan` | 미리보기: 슬롯 + 단별 Z + 경고 + 선검사 |
| 패턴 편집 API | 10절 |

품목 저장은 Flow가 편집 저장소에 있어야 한다(없으면 400).

`/api/pallet/plan`의 쿼리는 `station`, `center_x`, `center_y`, `floor_z`, `item_code`, `od`, `gap`, `flow`,
`pattern`(숫자/auto), `rotation`, `mirror_x`, `mirror_y`, `pallet_size`, `levels`, `type`, `grip`, `robot`이다.

- 값의 우선순위는 **쿼리 → 품목 팔렛 설정 → 기본값**이고, 각 값의 출처가 응답 `sources`에 실린다.
  흐름은 `type`이 있으면 그 작업의 흐름(`flow_for`), 없으면 `flow_in` → `flow_out` 순이다.
- 드래그 방향 보정은 `robot`(없으면 첫 로봇)의 `pallet_robot` 설정이다. 응답 `robot_dir`, 슬롯 `layout_drag_dir`(로봇 보정 전).
- 중심은 `station`이 있으면 그 Info.Position XY이고 Floor는 Info.Position Z다. 없으면 `center_x/center_y`(기본 0, 0)이고 Floor는 `floor_z`(없으면 Z 계산 안 함)다.
- `od`가 없으면 품목의 OuterDiameter를 쓴다.
- `grip`(그립 기준)은 `mid`(기본, Height/2)와 `pick_bead`(= bead+offset, 잰 상부 비드 − PickBeadOffset)뿐이다.
  옛 `bead`는 없어졌고 `pick_bead`로 읽는다. 단별 Z는 compose·`/api/stock/z`와 같은 `stack_z_with`를 지나므로
  **한 번도 안 잰 품목은 `pick_bead`를 시켜도 그립점이 `mid`로 내려간다**(`docs/item-spec-z.md`).

## 7. 작업 명령(compose) 동작

`TaskRequest.pallet = {seq, level}`(1-based, level 기본 1) 또는 `{auto: true}`.

1. 대상이 **팔렛 스테이션**(켜짐)이어야 한다. 셀 대상이거나 팔렛 스테이션이 아니면 400이다. 품목(OuterDiameter)이 없거나 품목 팔렛 설정이 없어도 400이다.
2. 흐름은 품목의 `flow_for(작업)`(DROP = FlowOut 먼저, 그 밖 = FlowIn 먼저)이고, 패턴은 품목 Pattern(없으면 OuterDiameter 자동)이다. 품목의 Gap·Rotation·Mirror·PalletSize를 쓰고, DragDir에는 대상 로봇(`req.robot`)의 방향 보정을 더 건다.
3. `auto`는 스테이션 재고 개수 n으로 고른다. 미리보기의 `?stock=`이 있으면 그 값이다. 한 단은 패턴 슬롯 수 k다.
   - DROP: 아래 단부터 채운다. level = ⌊n/k⌋+1, seq = n mod k + 1.
   - PICK/MEASURE: 맨 윗단을 Seq 순으로 비운다. n = 0이면 400이다.
4. Position X, Y는 슬롯 X, Y(중심 = Info.Position)다. Z·G는 기존 규칙이고, Z는 단에서 만든 스택 개수로 계산한다. `position_override`가 있으면 그것을 쓰고 경고한다.
5. 드래그는 흐름과 작업이 맞을 때만 넣는다.
   - 입고 흐름 + PICK이면 `UseDragIn = true`, `DragInDir = DragType 바이트`다.
   - 출하 흐름 + DROP이면 `UseDragOut = true`, `DragOutDir = DragType 바이트`다.
   - DragDir 0인 슬롯은 해당 드래그를 **끈다**(기본값에 켜져 있어도).
   - Dist·Height는 기본값(`작업 명령 › 기본값`)과 `params`를 그대로 쓴다. 0이면 경고한다.
   - 흐름과 작업이 어긋나면(예: 출하 흐름에 PICK) 자리만 쓰고 드래그는 넣지 않으며, 경고한다.
6. **스테이션 보정(컨베이어 트래킹)은 팔렛 스테이션에 쓰지 않는다.** 작성과 제출 재계산 모두 건너뛰고, 감사 기록에 `station_offset_skipped: true`를 남긴다. 트래킹 보정이 하던 GR2 영역 선검사는 팔렛 쪽이 대신한다(`area`).
7. 결과 `Composed.pallet`과 원장 `LedgerEntry.pallet`에 같은 감사 블록을 남긴다. 필드는 `item_code`, `robot`, `dir_rotation`, `dir_mirror_x/y`, `flow`, `pattern`, `od`, `gap`, `pitch`, `rotation`, `mirror_x/y`, `pallet_size`, `seq`, `level`, `slot`, `auto`, `stock_used`, `offset`, `xy`, `spec_drag_dir`, `layout_drag_dir`, `drag_dir`, `drag_type`, `drag_applied`(in/out/none), `station_offset_skipped`, `area`, `warnings`, `flow_updated_at`, `pattern_updated_at`다. auto로 고른 seq/level은 이 기록으로 고정되고, 재제출은 기록을 그대로 쓴다.
   - `flow_updated_at` · `pattern_updated_at`은 슬롯을 만든 흐름·패턴의 편집 시각이다. 나중에 패턴을 고쳐도 이 작업이 어느 판으로 만들어졌는지 원장에서 알 수 있다. 이 필드가 없는 예전 기록은 빈 문자열로 읽는다.
   - `spec_drag_dir`은 이름과 달리 **저장소 패턴**의 변환 전 방향이다(편집했으면 편집값). 사양서 원래 값은 비교(10절)로 본다.
8. 팔렛 스테이션인데 요청에 `pallet`가 없으면, Info.Position 중심으로 작성하고(트래킹 보정 없이) 경고한다.
9. 시나리오 스텝도 `pallet`를 싣는다. JSON에만 실리고 CSV 열은 없다. 러너가 그대로 `TaskRequest.pallet`로 넘긴다.

## 8. 화면 (팔렛 패턴 탭)

- **왼쪽 입력**
  - Station(또는 수동 Center X/Y/Floor Z) + "팔렛 스테이션" 스위치(누르면 바로 저장).
  - Item(또는 OuterDiameter). 품목을 고르면 품목 팔렛 패턴(FlowIn · FlowOut · Pattern · Gap · PalletSize · Rotation · MirrorX/Y · Note)이 채워지고 "품목 패턴 저장"으로 저장한다. 미리보기 In/Out 으로 어느 흐름을 펼칠지 고른다.
  - 로봇 DragDir: 선택된 로봇의 방향 보정 한 줄 + "설정" 대화상자(로봇마다 Rotation · MirrorX/Y, 바꾸면 바로 저장).
- **가운데 그림**
  - 팔렛과 타이어를 실제 크기로 그리고, 각 원에 Slot · Seq · DragDir을 표시한다.
  - 드래그 화살표 길이는 기본값 DragInDist/DragOutDist(기본 150 mm)이고, 0이면 그림에도 150을 쓴다.
  - 팔렛 밖·GR2 거부 예상 슬롯은 경고색, 겹침은 위험색이다.
  - 보기는 "사양서 방향"(흐름의 슬라이드 방향)과 "기계 축"(X+ 오른쪽 · Y+ 위) 둘이다.
  - 그 보기 방향의 DragDir 코드 격자를 같이 보인다.
- **오른쪽 표**
  - 열은 Seq, Slot, OffsetX, OffsetY, X, Y, DragDir, DragType(hex), Z L1..Ln이다.
  - "계획에 추가"는 단 순서(출하 = 아래부터, 입고 = 위부터) × Seq 순서로 `pallet: {seq, level}`을 실은 스테이션 스텝을 작업 명령 계획 끝에 붙인다(`lib/task/planInbox.ts`, 되돌리기 한 칸).
  - "시나리오로 내보내기"는 같은 스텝으로 시나리오를 가져오기 형식(JSON)으로 만든다.
  - "CSV 복사"도 있다.
  - 두 내보내기는 팔렛 스테이션·품목 팔렛 패턴이 있고, 입력이 저장값과 같고, 미리보기 흐름이 그 작업의 흐름과 같고, OuterDiameter를 직접 넣지 않았을 때만 켜진다. 작업 명령은 저장된 품목 패턴으로 다시 계산하므로, 화면과 제출이 갈리지 않게 막는다.
- **패턴 편집**(헤더의 "패턴 편집" 단추) — 켜면 생성 보기 대신 편집 화면이 나오고, 끄면 생성 보기가 그대로 돌아온다. 저장하면 생성 보기의 사양·계획을 다시 읽는다.
  - **흐름 목록**: 새 흐름 · 복사 · 이름·설정(id 바꾸기 포함) · 사양서로 되돌리기 · 삭제. 배지 `spec`(사양서와 같음) · `modified`(고침) · `custom`(사양서 기준 없음). 품목이 쓰는 흐름은 삭제 단추가 꺼지고 품목 코드를 보여 준다.
  - **패턴 목록**: Pattern · OdMin · OdMax · Slots · MinDistance. 추가 · 복제(새 번호, OD 범위는 기존 최대 위로 두니 고쳐 넣는다) · 이 패턴만 되돌리기 · 삭제.
  - **그림**: 원을 끌어 옮긴다(Snap off / 10 mm / 0.05). 두 번 누르거나 D 키로 DragDir 순환, 방향키로 스냅 단위 이동. "Seq 재정렬"을 켜고 작업 순서대로 누르면 Seq가 바뀐다. MirrorX · MirrorY · 90° · 180°는 오프셋과 DragDir를 같은 변환으로 바꾼다(생성기와 같은 변환 코드). 그림은 편집 좌표 `u × (Preview OD + Gap)`이고, MinDistance ≠ 1이면 생성 결과를 점선 원으로 겹친다.
  - **슬롯 표**: 행 끌기로 Seq 바꾸기, Slot 이름, OffsetX/OffsetY(Units `norm` 또는 `mm` — mm 입력은 미리보기 pitch로 u를 다시 계산), DragDir, DragType(hex), 슬롯 삭제. 저장값과 다른 행은 경고색.
  - **DragDir 격자**: 고른 슬롯의 방향을 3×3 칸에서 고른다. 칸 배치는 현재 보기 방향의 사양서 범례와 같고, 가운데는 0(드래그 없음), 칸마다 코드와 DragType 바이트를 적는다.
  - **실시간 검사**: 백엔드와 같은 검증(오류면 저장 단추가 꺼진다), 겹침(편집 좌표 중심거리 < OuterDiameter), 팔렛 밖(생성 결과 기준).
  - **저장 · 취소**: 한 번에 패턴 하나. 저장하지 않은 변경이 있으면 다른 흐름·패턴으로 가거나 편집을 끌 때 묻고, 브라우저를 닫을 때도 막는다.
  - **사양서 비교**: 바뀐 패턴마다 사양서/현재 그림을 나란히 두고 바뀐 행(필드·슬롯)을 표로 보인다.
  - **JSON**: 파일로 받기 · 복사 · 가져오기(미리보기 dry-run이 통과해야 적용).
  - 순수 로직(단위 변환·끌기·Seq 재정렬·검증·비교)은 `lib/pallet/editorModel.ts`이고 vitest로 검사한다.

## 9. 사양서 불일치 · 판단 기록

1. **Pattern 5 범위**: 순서표는 "601 ~ 622", 크기 표는 "601 ~ 662"다. 662를 쓴다(623..662가 빈 구간이 되지 않게).
2. **OP 출하 P3**: 슬라이드 5 붉은 박스 "3(p7) → 2(p6) → 1"을 쓴다. 원래 줄 "2(p6) → 3(p7) → 1"은 슬라이드 5 윗줄과 슬라이드 6 흐린 칸에 남아 있다.
3. **OP 외부입고 P3** "1 → 3(p7) → 2(p6)"는 수정 **전** OP 출하 순서를 뒤집은 것이다. 나머지 패턴은 외부입고 = 출하 역순인데, P3만 수정이 반영되지 않았다(그대로 둠, 확인 필요).
4. **슬라이드 2 OP 입고 P4** "3 (45)": 슬라이드 4는 "3 (p4)", 원 글자는 C#3 "4↑"이므로 → **DragDir 4**(p5가 아니다).
5. **슬라이드 2 OP 입고 P3**에는 둘째 줄 "2(p8) → 1(p6)"(C#3 없음)이 더 있다. 슬라이드 4는 3개짜리 한 줄뿐이라 그것을 쓴다.
6. **OP 입고 P6** "1(7)"은 p가 빠진 오기다. 원 글자 C#1 "←7" → DragDir 7.
7. **OP 출하 P5** "… 3(p7) → 2 → 1"에는 2의 방향이 없다. 원 글자 C#2 "4↓"와 외부입고 P5 "2(p4)"에 따라 → DragDir 4.
8. **슬라이드 6 흐린 OP 출하 P8**은 C#4/C#5 위치가 서로 바뀌어 있고 순서도 "6 → 4 → 5"다. 슬라이드 5(원본)를 쓴다.
9. **슬라이드 5 흐린 "OP 입고"**는 슬라이드 2/4 OP 입고와 슬롯 위치는 같지만 P4..P9의 순서·방향이 다르다(아래 줄부터 한 줄씩). 그 P4는 슬라이드 3 대안(IN1_ALT_OP)과 기계 축으로 똑같다. 어느 쪽이 현행인지 몰라 `OP_IN_S5`(reference)로 따로 넣었다.
10. 슬라이드 3 대안 **IN1_ALT_HP**는 화면 방향만 다르고 기계 축으로 HP_IN P4와 똑같다.
11. **그려진 Gap 음수**: P3 −8.4 mm, P8 −3.1 mm(원이 겹쳐 그려짐). 그래서 그려진 D가 아니라 최소 중심거리로 정규화했다.
12. P3만 무게중심이 (42.4, −75.5) mm 치우쳐 있어 그대로 두었다(10 mm 규칙). P6은 9.5 mm라 0으로 맞췄다.
13. **안쪽을 향하는 드래그** 11건: 변위가 패턴 무게중심 쪽 성분을 가진 것이다. 사양서를 그대로 두고 테스트로 막지 않았다.
    - HP_IN P8 C#3(2)
    - OP_IN P5 C#3(7), P6 C#4(7), P8 C#6(7)
    - OP_IN_S5 P5 C#3(7), P6 C#4(7), P8 C#6(7)
    - OP_OUT P6 C#2(7), P8 C#3(7)
    - OP_EXT_IN P6 C#2(7), P8 C#3(7)
14. 표지는 "Rev2 2025.07.02"인데 파일 이름은 R4 251213이다. 슬라이드 2와 4는 도형 좌표가 완전히 같다(존 이름·보관량만 다름).
15. PLC `isValidTaskArea` 관찰(수정하지 않음):
    - 마진은 `STATION.Station[#StationNo].TaskType`을 보는데, 나머지는 `Station[#idx]`를 본다.
    - 함수 이름과 달리 invalidCode ≠ 0일 때 TRUE를 돌려준다.

### 운영자가 편집으로 결정

사양서만으로 정할 수 없는 것은 콘솔이 **사양서 그대로 넣어 두고, 현장에서 운영자가 패턴 편집으로 정한다**(10절).
고친 뒤에도 비교 창에서 사양서 원래 값이 보이고, 흐름·패턴 단위로 언제든 되돌릴 수 있다.

- **OP 입고 현행본**(위 5·9번) — 슬라이드 2/4(`OP_IN`)인가, 슬라이드 5 흐린 칸(`OP_IN_S5` = 슬라이드 3 대안)인가.
  현장 판단에 따라 품목 FlowIn을 `OP_IN_S5`로 두거나, `OP_IN`의 P4..P9 순서·DragDir를 고친다.
  `OP_IN_S5`를 현행으로 쓰면 이름·설정에서 reference 표시를 끄는 대신 복사본(예: `OP_IN_SITE`)을 만들어 쓰는 것을 권한다.
- **외부입고 P3 순서**(위 3번) — 출하 수정본에 맞춰 "1 → 2(p6) → 3(p7)"로 뒤집어야 하면 `OP_EXT_IN` Pattern 3에서
  Seq만 바꾼다("Seq 재정렬" 또는 표에서 행 끌기).
- **Pattern 5 범위**(위 1번) — 순서표의 622가 맞으면 해당 흐름 P5 OdMax를 622로 고친다. 623..662는 빈 구간이 되어
  계획·작업 명령이 `no pattern for OD`로 거부한다(흐름 목록 경고에 표시).
- **오기·원 글자와 순서표 불일치**(위 4·6·7·8번) — 콘솔은 원 글자를 따랐다. 다르게 확인되면 해당 슬롯 DragDir를 고친다.

## 10. 편집 저장소 · 편집 API

### 저장 구조

마이그레이션 `0006_console_v2`.

| 표 | 필드 |
|---|---|
| `pallet_flow` | `id`(PK) · `name` · `drag_kind`(in/out) · `source`(`spec_r4` / `custom` / `import`) · `note` · `updated_at` · `based_on`(사양서 흐름 id 또는 NULL) · `meta_json`(source_slide · also_slides · reference · robot · zone · screen_axes · notes) · `sort` |
| `pallet_pattern` | `flow_id` + `pattern`(PK) · `od_min` · `od_max` · `slots_json` · `note` · `updated_at` · `meta_json`(order_text · drawn · notes) |

- `slots_json` = `[{"slot": "C#n", "seq": 1, "u": [x, y], "drag_dir": 0..8}]`. `u`는 기계 축 정규화 오프셋이다(2절).
- 생성기·계획·compose·품목 팔렛 설정 검증은 이 표를 메모리 `Library`로 읽어 쓴다. 테스트는 `Library::from_seed`로 sqlite 없이 돈다.

### 채우기 · 초기화 · 비교

- **첫 기동**에 표를 내장 사양서 R4의 7개 흐름(참고본 `OP_IN_S5` 포함)으로 채운다. `source = spec_r4`, `based_on = 자기 id`이고, settings `pallet.seed`에 기록한다. 이후 기동은 채우지 않는다. 흐름을 모두 지워도 다시 채우지 않는다.
- **되돌리기**(`reset`)는 `based_on` 사양서 흐름으로 되돌린다. 패턴 하나만 되돌리거나(`?pattern=`), 흐름 전체(이름·DragKind·메타·모든 패턴, 운영자 note는 둔다)를 되돌린다. `based_on`이 없는 흐름은 400이다.
- **비교**(`diff`)는 `based_on` 사양서 흐름과 패턴별로 비교한다. 패턴 상태(same / changed / added / removed)와 바뀐 필드(`od_min`, `od_max`, `note`), 슬롯별 바뀐 필드(`seq`, `u`, `drag_dir`; u 허용오차 0.00005)를 준다. 흐름 `status`는 `spec` / `modified` / `custom`이다. `name`·`note`만 바뀐 것은 `modified`로 치지 않는다(복사본은 이름이 늘 다르다).
- 흐름 **복사**(`copy_from`)는 원본의 패턴·메타·`based_on`을 그대로 가져가고 `source = custom`이 된다. 고치기 전까지 상태는 `spec`이다.

### 검증 (400)

- Flow id: 앞뒤 공백을 떼고 대문자로 바꾼 뒤 `[A-Z0-9_]{2,32}`여야 한다. 중복이면 409다.
- name 1..80자, note 500자 이하, screen_axes는 right `X+`/`X-`, down `Y+`/`Y-`여야 한다.
- 슬롯 1..20개, Seq는 1..n 순열, DragDir 0..8, Slot 이름 1..16자이고 한 패턴 안에서 중복이 없어야 한다. |u| ≤ 20.
- OdMin < OdMax(0..3000 mm)이고, **한 흐름 안에서 OD 범위가 겹치면 안 된다**(경계값이 같아도 겹침). 틈은 허용하되 흐름 경고 `no pattern for OD a~b`를 낸다.
- 두 슬롯이 같은 자리(MinDistance < 0.01)면 오류다. MinDistance ≠ 1은 경고만 한다(2절 정규화 규칙).
- 경고(저장은 된다): 입고 흐름의 Seq 1, 출하 흐름의 마지막 Seq에 DragDir ≠ 0(사양서 전 표에서 드래그 없음).
- 검증은 한 트랜잭션 안에서 흐름 전체로 하고, 실패하면 아무것도 쓰지 않는다. 편집 화면(`editorModel.ts`)도 같은 규칙으로 저장 전에 보여 준다.

### API

| API | 설명 |
|---|---|
| `GET /api/pallet/flows` | 흐름 목록 + 패턴(`min_distance`, 비교 `status`) · `status` · `modified` · `used_by`(그 흐름을 쓰는 품목 코드) · `warnings` · `removed_spec_patterns` |
| `POST /api/pallet/flows` | 만들기. `{id, name?, drag_kind, note?}` 또는 `{id, copy_from, name?, note?}` |
| `PUT /api/pallet/flows/{id}` | `{id?, name?, drag_kind?, note?, screen_axes?, reference?}`. id가 다르면 이름 바꾸기이고, 그 흐름을 쓰는 품목의 FlowIn/FlowOut도 같이 바뀐다 |
| `DELETE /api/pallet/flows/{id}` | 품목이 쓰면 **409**(품목 코드 목록) |
| `PUT /api/pallet/flows/{id}/patterns/{pattern}` | 패턴 저장(없으면 만든다). `{od_min, od_max, slots, note?, pattern?}`. 본문 `pattern`이 경로와 다르면 번호를 바꾼다(있는 번호면 409) |
| `DELETE /api/pallet/flows/{id}/patterns/{pattern}` | 패턴 삭제 |
| `POST /api/pallet/flows/{id}/reset[?pattern=]` | 사양서로 되돌리기 → `{flow, diff}` |
| `GET /api/pallet/flows/{id}/diff` | 사양서 비교 |
| `GET /api/pallet/export.json` | 저장소 전체를 사양 JSON 모양으로(첨부 파일) |
| `POST /api/pallet/import?dry_run=1` | 가져오기. 본문 = 사양 JSON |

### 내보내기 · 가져오기 형식

`spec_r4.json`과 같은 모양이다. 내보내기는 흐름마다 `source`, `based_on`, `note`, `updated_at`, 패턴마다 `note`, `updated_at`을 더 싣는다.

```json
{
  "flows": [
    {
      "id": "HP_IN_SITE", "name": "HP 입고 현장", "drag_kind": "in", "based_on": "HP_IN",
      "patterns": [
        { "pattern": 4, "od_min": 663, "od_max": 755, "note": "",
          "slots": [ { "slot": "C#1", "seq": 1, "u": [-0.5, -0.5], "drag_dir": 0 } ] }
      ]
    }
  ]
}
```

- 필수는 `flows[].id`, `flows[].drag_kind`, `patterns[].pattern / od_min / od_max / slots`다. 나머지(`version`, `size_classes`, `drawn`, `order_text`, `notes`, `screen_axes` …)는 없어도 된다.
- **병합**: 흐름 id + 패턴 번호로 합친다. 문서에 없는 흐름·패턴은 그대로 둔다. 같은 내용(OD·슬롯·note)의 패턴은 `unchanged`이고 `updated_at`을 바꾸지 않는다.
- 있는 흐름은 name·note·robot·zone·notes가 비어 있지 않을 때만, screen_axes가 기본값(기계 축)이 아닐 때만 덮어쓰고, drag_kind는 늘 덮어쓴다.
- 새 흐름은 `source = import`이고, `based_on`은 문서 값(사양서 흐름일 때만) 또는 같은 id의 사양서 흐름이다.
- `dry_run=1`은 쓰지 않고 보고서만 준다: `{ok, errors, created, updated, unchanged, flows: [{id, action, patterns: [{pattern, action}], warnings, errors}]}`.
- 실제 적용은 **하나라도 오류가 있으면 전부 거부**(400)하고, 통과하면 한 트랜잭션으로 쓴다.

### 감사

- 흐름 `updated_at`은 그 흐름의 무엇이든(패턴 포함) 바뀔 때, 패턴 `updated_at`은 그 패턴 내용이 바뀔 때만 새로 찍는다.
- 계획 응답은 `flow_source`, `based_on`, `flow_updated_at`, `pattern_updated_at`, `pattern_note`, `min_distance_norm`, `scale`을 싣는다.
- 작업 명령 감사 블록(`Composed.pallet` / `LedgerEntry.pallet`)은 `flow_updated_at`, `pattern_updated_at`을 남긴다(7절).

## 11. 열린 질문

1. **어느 스테이션에 어느 Flow·Rotation·Mirror를 붙이나** — 팔렛 스테이션 자체가 아직 정해지지 않았다. 슬라이드의 존(입고1/2, 출하1~3, 외부입고)과 콘솔 스테이션 id의 대응도 미정이다.
2. ~~**DragInDist / DragOutDist 기본값**~~ — **정해짐(2026-09-18): 150 mm**. 요청도 기본값도 안 정하면 compose 가 `gr_proto::DEFAULT_DRAG_DIST` = 150 을 넣고(팔렛 compose 도 같다), 저장된 기본값의 0 은 migration `0006_console_v2` 가 150 으로 옮긴다 — 옛 "Dist = 0 경고" 는 더 이상 없다. **DragInHeight / DragOutHeight 기본값은 아직 미정**이라 0 이면 지정 없음이다.
3. **GR2 마진** — TaskType 0 스테이션은 마진 300이다. P2~P9 슬롯 대부분이 중심에서 300 mm 넘게 떨어지므로 411/412 거부가 예상된다. 팔렛 스테이션을 TaskType ≠ 0으로 둘지, PLC 검사를 팔렛에 맞게 바꿀지 결정해야 한다. RotateType도 0(±OD/2 이동 없음)이어야 하는지 확인이 필요하다.
4. **단(Level)의 뜻** — 지금은 한 슬롯에 타이어가 한 단씩 쌓이고, 스텝마다 count 1이며, 입고는 맨 윗단부터 집는다고 본다. 여러 개를 한 번에 집는지, 단마다 패턴이 바뀌는지(교차 적재) 확인이 필요하다.
5. **auto 재고** — 스테이션 재고는 완료된 PICK/DROP이 접힌 값뿐이다. `PUT /api/stock`은 셀만 받으므로, 팔렛을 손으로 채웠을 때 개수를 맞출 방법이 필요한지 정해야 한다.
6. **팔렛 중심 = Info.Position** 가정 — 팔렛이 스테이션 중심에 정확히 놓이는지, 기계 축과 팔렛 모서리가 나란한지(Rotation 0/90만으로 충분한지).
7. **패턴 편집 권한** — 지금은 콘솔에 들어온 누구나 패턴을 고칠 수 있다(콘솔은 무인증). 편집 잠금·승인이 필요한지, 고친 패턴을 다른 콘솔로 옮길 때 JSON 가져오기로 충분한지 정해야 한다.
