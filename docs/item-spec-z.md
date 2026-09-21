# 화물 규격과 작업 Z (집는 높이 · 눌림양)

콘솔이 셀 대상 PICK / DROP / MEASURE 의 Z 를 정하는 규칙. 계산은 `apps/gr-console/src/registry/spec.rs`
(`stack_z_with` · `curve_of` · `grip_point` · `pick_z_at`, 순수 함수 + 테스트)와 `stock/mod.rs`
(`pressed_height`), 적용은 `issue/mod.rs`(compose) · `stock/routes.rs`(`/api/stock/z`) · `pallet/routes.rs`
(단별 계획 Z). 화면은 **화물 규격** 페이지(Spec · Beads 탭)와 작업 발행의 Z 블록.

프런트 거울은 `apps/gr-web/src/lib/items/levelsModel.ts`(`pickPoint`)와 `lib/task/plan.ts`(순차 계획 표).
규칙이 바뀌면 두 곳을 함께 고친다.

## 값이 어디 있나

| 값 | 어디 | 기본값 |
|---|---|---|
| `Height` · `UpperBidHeight` · `LowerBidHeight` | PLC 와이어 `LGR_Stock_Item`(품목 필드) | — |
| `PickBeadOffset` | 콘솔 `ItemSpec.pick_bead_offset` (spec_json) | **30 mm** (빈칸) |
| `Compression`(눌림양) | 콘솔 `ItemSpec.compression` | **0 mm** (빈칸 = 보정 없음) |
| `CompressionSource` | 콘솔 `ItemSpec.compression_source` | 빈 메모 |
| **비드 프로파일** `profiles[]` (스택 크기별 절대 `Level` · `LowerBead` · `UpperBead` · `StackHeight` · `Source`) | 콘솔 `ItemSpec.profiles` — **정본** | 없음(계산값) |
| 하중 곡선 `AboveRow[]` | 프로파일에서 **파생**(`spec::curve_of`) — 저장하지 않는다 | — |
| `AutoApplyMeasured` | 콘솔 `ItemSpec.auto_apply_measured` | **켬** (빈칸) |
| `DragInDist` · `DragOutDist` | `Defaults.base`(또는 요청) | **150 mm** |
| 그립 기준 `grip_ref` | `Defaults.grip_ref` 또는 작업 요청의 덮어쓰기 | `mid` |

`DeflectionFactor` 는 PLC 로만 가고 콘솔 계산에 쓰지 않는다 — 눌림은 `Compression` 이 맡는다.

## 눌림은 "위에 얹힌 개수"로 정해진다

`n` 개 스택에서 아래에서부터 `k` 단 타이어 위에는 `above = n − k` 개가 얹혀 있다.

```
눌린 단 높이(j 단)   = max(Height − Compression × (n − j), 1 mm)
눌린 상부 비드(k 단) = max(UpperBead − Compression × above, 1 mm)
집는 높이(그립점)    = max(눌린 상부 비드 − PickBeadOffset, 0)   ← 비드를 **잰** 품목만
                       Height / 2                                ← 아직 안 잰 품목(아래 "처음 재는 품목")
```

작업별로 잡는 타이어와 `above` 는 이렇게 정해진다.

| 작업 | 아래 깔린 수 `below` | 잡는 단 | `above` |
|---|---|---|---|
| PICK / MEASURE (`c` 개) | `n − c` | `below + 1` | `c − 1` (같이 들려 올라갈 타이어) |
| DROP (1개를 `n` 위에) | `n` | `n + 1` | `0` (위에 아무것도 없다) |

그래서 최종 Z 는

```
Z = floor + Σ_{j=1..below} 눌린 단 높이(j) + 그립점
```

같은 타이어라도 **혼자 있을 때와 스택 아래에 깔렸을 때 집는 높이가 다르다** — 이것이 이번 규칙의 핵심이다.

## 그립 기준(`grip_ref`) — 2026-09-18 개정

| 값 | 화면 이름 | 그립점(타이어 바닥 기준) |
|---|---|---|
| `mid` | 타이어 중간 (H/2) | `Height / 2` (공칭값, 눌림 반영 안 함) ← **기본값** |
| `pick_bead` | bead+offset (− PickBeadOffset) | **잰** 상부 비드 − `PickBeadOffset` |

- **기본값은 `mid`** 다 — 새로 깐 콘솔·데모 씨앗·`Defaults::default()` 모두 `mid`.
- **맨 비드(`bead`, 상부 비드를 그대로 잡기)는 없어졌다.** 비드로 잡는 길은 `pick_bead` 하나뿐이고
  화면 이름을 `bead+offset` 으로 바꿔 "비드에서 오프셋만큼 내려 잡는다" 를 드러냈다. 저장되는 값 이름은
  `pick_bead` 그대로다. 남아 있던 `bead` 는 sqlite 마이그레이션 `0006_console_v2` 가 `pick_bead` 로 옮기고
  (`settings.defaults.grip_ref`, `tasks.doc_json.request.grip_ref`), 읽는 자리(`normalize_grip_ref`,
  `Registry::defaults`, 프런트 `normalizeGripRef`)도 `bead` 를 `pick_bead` 로 받는다 — **다시 써 내보내지는 않는다.**

### 처음 재는 품목은 1/2 높이로

`pick_bead` 는 **그 비드를 실제로 잰 값이 있을 때만** 쓴다. 그립점은 이 차례로 정해진다.

1. **그 스택 크기를 통째로 잰 프로파일** — 절대 `UpperBead(n, Level)` 를 환산 없이 그대로
2. 없으면 파생 곡선의 그 `Above` 점, 또는 잰 두 점 사이 **보간** (`interpolated`)
3. 그것도 없으면 **`mid`(Height/2)** — 공칭 `UpperBidHeight − Compression×Above` 로 지어낸 비드는 **쓰지 않는다**

한 번도 안 잰 품목을 공칭 비드로 집으면 실제 비드와 어긋나 헛집을 수 있다. 그래서 첫 측정 전에는
타이어 중간을 잡는다. 이때 audit(`StackZ.grip_ref = "mid"`, `used` 에 `grip=mid (측정 없음, Height/2 = …)`)와
compose 경고(`그립 기준 bead+offset: 측정된 비드가 없어 mid(Height/2 = …) 로 잡습니다`)에 그 사실이 남고,
화면 미리보기도 `grip=mid (측정 없음)` 으로 보인다. 화물 규격 Beads 탭의 `PickZ` 칸에도 `mid` 가 붙는다.

**내려가는 건 그립점뿐이다.** 잡는 타이어 **아래** 타이어들의 높이는 잰 값이 없으면 여전히 스칼라
`Compression` 모형(`max(Height − Compression×above, 1 mm)`)으로 쌓는다. 클램프·경고도 그대로다.

`UpperBidHeight = 0` 인 품목도 마찬가지로 `mid` 다. 작업 발행 화면의 그립 기준 선택(타이어 중간 ·
bead+offset)이 `Defaults.grip_ref` 를 바꾸고, 작업 요청의 `grip_ref` 가 그걸 덮어쓴다.

`/api/stock/z` · compose(`/api/issue/compose`) · 순차 계획 표 · 팔렛 단별 계획 Z 는 모두 같은
`stack_z_with` 를 지나므로 네 곳의 Z 가 같다(프런트 미리보기의 거울은 `plan.ts::planZ` ·
`levelsModel.ts::pickPoint`, 둘 다 프로파일 → 곡선 → mid 차례를 그대로 따른다).

## 정본은 스택 크기별 **절대** 프로파일이다 (2026-09-18)

PLC 의 `measureSKU` 는 스택 하나를 통째로 재서 **셀 바닥 기준 절대** 비드를 준다. 그 값을 잰 **스택 크기
`n` 별로 그대로** 보관한다(`ItemSpec.profiles`, `BeadProfile`). 같은 크기의 스택을 집을 때는 환산도,
아래 타이어 높이 합산도 없이 그 절대값을 그대로 쓴다 — 계산이 끼어들 자리가 없으니 어긋날 자리도 없다.

| 칸 | 뜻 |
|---|---|
| `Stack`(=`count`) | 잰 스택의 단수 — 프로파일의 키 |
| `Level` | 그 스택에서의 단 번호(맨 아래가 1) |
| `LowerBead` · `UpperBead` | **셀 바닥 기준** 절대 비드 높이(mm) |
| `StackHeight` | 그 단 타이어 윗면까지(맨 윗단은 표본의 `TotalHeight`) |
| `Source` | `measured`(측정 그대로) · `manual`(손으로 고침 — 다시 재도 안 덮는다) |
| `SamplePlc` · `SampleSeq` · `At` | 이 프로파일을 채운 SKU 표본 |
| `TotalHeight` · `EachHeight` | 표본이 같이 준 값(검사용) |

같은 크기를 다시 재면 그 프로파일이 **통째로** 새 값으로 바뀐다(`manual` 줄만 남는다). 크기가 다른
프로파일은 나란히 산다(n = 3 · 5 · 8 …).

### 하중(`Above`) 곡선은 파생값이다

잰 적 없는 크기를 위해서는 여전히 하중 곡선이 필요하다. 눌림은 **그 위에 몇 개가 얹혀 있는가**로 정해지므로
(5 개 스택의 3 단 = 위 2 개, 8 개 스택의 3 단 = 위 5 개), 프로파일에서 `above = n − Level` 을 키로 잡아
타이어 자기 바닥 기준 값으로 옮긴 곡선을 **그때그때 만든다**(`spec::curve_of`, 저장하지 않는다).

| 칸 | 뜻 |
|---|---|
| `Above` | 그 타이어 **위에 얹힌 개수**(키) |
| `LowerBead` · `UpperBead` | **그 타이어 자기 바닥 기준** 비드 높이(mm) |
| `PressedHeight` | 그 하중에서 타이어 하나가 차지하는 높이(mm) |

크기가 다른 프로파일은 곡선의 다른 구간을 채우고, 같은 `Above` 가 겹치면 **새 표본**이 이긴다(손으로 고친
줄은 지지 않는다). `Compression@Level = Height − PressedHeight` 는 화면이 보여 주는 파생값이다.

되짚는 식(`spec::stack_profile`):

```
pitch(k)  = LowerBead(k+1) − LowerBead(k)      (하부 비드가 타이어 바닥을 가장 곧게 말해 준다;
                                                없으면 상부 비드 차이)
pitch(n)  = TotalHeight − Σ_{k<n} pitch(k)     (맨 윗단; TotalHeight 가 없으면 EachHeight)
bottom(k) = Σ_{j<k} pitch(j)                   (맨 아래 타이어 바닥 = 0)
곡선 점: above = n−k, PressedHeight = pitch(k),
        UpperBead = 측정 UpperBead(k) − bottom(k), LowerBead = 측정 LowerBead(k) − bottom(k)
```

측정 검사는 PLC 가 따로 준 두 값이 서로 맞는지로 한다 — `|TotalHeight − EachHeight × TotalCount| > 5 mm`
면 경고(`spec::TOTAL_HEIGHT_TOL`).

## 우선순위 — 집는 Z 를 푸는 차례

잡는 타이어가 **설** 스택 크기는 `size = Level + Above` 다 — PICK 은 `n`, DROP 은 놓고 나면 한 단
높아지므로 `n + 1`.

1. **잰 프로파일 그대로**(`z_source = profile`) — `size` 를 통째로 잰 프로파일에 그 `Level` 이 있으면

   ```
   Z = floor + max(AbsUpperBead(size, Level) − PickBeadOffset, 0)
   ```

   환산도, 아래 타이어 합산도, `Compression` 도 쓰지 않는다 — 절대값이 이미 다 담고 있다.
   audit `used` 에 `grip=pick_bead abs(n=8,L3) 660.9 (GR2 #45)`.
2. **파생 곡선**(`z_source = curve`) — 그 크기를 잰 적이 없으면 곡선으로 환산한다. 잡는 타이어 **아래**
   타이어들은 각자 자기 하중(`above_j = n − j`)의 `PressedHeight` 로 쌓인다.

   ```
   base = Σ_{j=1..below} PressedHeight(above = n − j)
   Z    = floor + base + max(UpperBead(above) − PickBeadOffset, 0)
   ```

   그 `Above` 의 점이 있으면 그대로, 없으면 가장 가까운 아래/위 점 사이를 선형 보간한다(`interpolated`).
   audit `used` 에 `upper_bead[above=2]=202 measured(GR2 #41)` 와 `grip=pick_bead 172.4 converted(measured)`.
3. **대체 모형**(`z_source = computed`) — 잰 게 하나도 없으면 그립점은 `mid`(Height/2)로 내려가고, 아래
   타이어 높이만 스칼라 `Compression` 으로 짓는다:
   `PressedHeight = max(Height − Compression × above, 1)`.

그립 비드의 출처는 `bead_source`(`measured` · `manual` · `interpolated` · `computed`)로 따로 남는다.

제한(클램프)이 걸리면 — 눌림이 `Height` 를 다 먹거나 `PickBeadOffset` 이 비드보다 클 때 — compose 경고에
`Z: …` 로 올라오고 `/api/stock/z` 의 `warnings` 에도 실린다.

## 측정 표본 이력 · 자동 반영

MEASLOG SKU 항목은 들어오는 즉시 **비드 표본**으로 적재된다(`item_bead_samples`, migration
`0006_console_v2`, 품목·PLC 당 최근 20 개). 품목 스위치 `AutoApplyMeasured`(기본 켬)가 켜져 있으면 검증을
통과한 표본이 **그 스택 크기의 프로파일**까지 자동으로 갱신한다. 켜고 끄는 다른 스위치는 없다 — 재면 바로
그 크기의 픽이 bead+offset 을 쓴다.

거부 규칙(사유가 그대로 화면에 뜬다): 측정 `Status ≥ 3` · `MeasureSku Status ≥ 3` · `DiagFlags ≠ 0` ·
`TotalCount = 0` 또는 20 초과 · 단별 비드 없음 · `UpperBead ≤ LowerBead` · 단이 올라가는데 비드가 안 올라감 ·
0 … 5000 mm 밖의 값. **손으로 고친 줄(`Source = manual`)은 절대 덮지 않고** 건너뛴 단을 알린다.

| API | 하는 일 |
|---|---|
| `GET /api/items/{code}/levels?robot=&preview=n` | 스택 `n` 의 단별 보기(잰 크기면 절대값 그대로, `from_profile`) + 표본 이력 |
| `GET /api/items/{code}/bead-samples?robot=&limit=&all=` | 표본 이력(최신 순, `applied` · `reason`) |
| `POST /api/items/{code}/bead-samples/{seq}/apply?robot=` | 옛 표본을 손으로 반영 |
| `POST /api/items/{code}/levels/apply-measured?robot=&fields=` | 옛 이름 그대로 — 같은 코드로 위임 |

스칼라 `Compression` 은 여전히 되짚어 저장하지만(`suggestion`), 이제는 **잰 적 없는 크기를 위한
대체값**일 뿐이다.

- `beads`: 단별 상부 비드를 위 개수에 대해 원점을 지나는 최소제곱으로 회귀.
- `each_height`: `C = 2 × (Height − EachHeight) / (count − 1)` — 단별 비드가 없을 때의 대안.

화면은 **화물 규격 › Beads** 탭: 표는 **스택 크기**로 매겨진다. `Stack n` 칸(과 잰 크기 칩 `n=3 · n=5 ·
n=8`)으로 크기를 고르면 그 크기의 `Level 1..n` 이 `AbsLowerBead` · `AbsUpperBead` · `StackHeight` ·
`Above` · `Compression@Level` · `Source` · `SampleSeq` · `PickZ` 로 펼쳐진다. 잰 적 없는 크기는 곡선에서
지어낸 값이 흐리게 보이고(고치면 그 값이 손입력으로 저장된다), 칸을 고치면 그 줄이 `manual` 이 된다.
아래 **Measured** 구역이 표본 이력(Time · Robot · TotalCount · EachHeight · Status · Applied/Rejected 사유 +
Apply 버튼)이다. 옆모습 그림은 고른 스택 크기를 그린다 — 잰 크기면 저장된 절대값 그대로다.

## Excel (2026-09-21 — 품목 추가의 두 번째 길)

화면: **화물 규격 › `추가 ▾`** — 왼쪽 `추가` 는 폼 하나(직접 입력), `▾` 는 `직접 입력… · Excel 가져오기… ·
양식 받기 · Excel 내보내기`. 빈 표에도 `직접 입력` · `Excel 가져오기` 버튼이 선다. 가져오기 창은 파일 고르기와
**끌어다 놓기**를 다 받고, dry-run 결과를 줄 결과 넷(추가 · 갱신 · 동일 · 건너뜀)과 문제 줄 표(Row · Sheet ·
Message)로 보인 뒤에야 `적용` 을 준다. 쓸 줄이 없으면 `적용` 은 막히고 이유를 말한다(모두 같은 값 / 모든 줄
오류 / 적용할 행 없음). 적용 뒤 토스트는 `품목 12건 추가 · 3건 갱신`.

| API | 하는 일 |
|---|---|
| `GET /api/items/template.xlsx` | **빈 양식** — `Items` · `ItemBeadProfile` 머리글만(자료 줄·예시 줄·설명 줄 없음). 받은 그대로 올리면 아무것도 안 바뀐다 |
| `GET /api/items/export.xlsx` | 지금 품목 전부 — 같은 두 시트(양식과 머리글이 같다) |
| `POST /api/items/import-file?dry_run=1\|0` | 두 시트를 한 번에. 응답 `added · updated · unchanged · skipped`(+ 옛 이름 `imported` = added) · `errors[{row, sheet, message}]` · `counts` |

`/api/registry/import-file`(셀 · 스테이션 · 품목 세 표)은 그대로이고 같은 응답 모양을 쓴다.

**`Items` 시트** — 한 줄 = 품목 하나. 머리글 이름으로 열을 찾는다(대소문자·공백 무시, 한국어 별칭 `코드`·`품명` 등도 받음).

| 열 | 필수 | 비우면 | 열 자체가 없으면 |
|---|---|---|---|
| `Code` | **필수**(0·빈칸이면 그 줄 거부) | — | 시트 전체 거부 |
| `Name` | **채울 것**(검사는 안 한다) | 이름 없는 품목 | 이름 없는 품목 |
| `Count` | | 1 | 1 |
| `InnerDiameter` · `OuterDiameter` · `LowerBeadHeight` · `UpperBeadHeight` · `Height` · `DeflectionFactor` | | 0(= 미입력) | 0 |
| `StackMax` · `PalletMax` | | 0(= 제한 없음) | **저장된 값 유지**(새 품목은 0) |
| `WeightKg` · `PickBeadOffset` · `Compression` | | 없음(기본: PickBeadOffset 30, Compression 0) | **저장된 값 유지** |
| `Note` | | 빈 칸 | 빈 칸 |

`Name` 을 필수로 막지 않는 이유: `Code,StackMax` 두 열만 든 파일로 기존 품목의 한 값만 고치는 길이 있다 —
이름을 요구하면 그 길이 막힌다.

**`ItemBeadProfile` 시트(선택)** — 한 줄 = 잰 스택 하나의 한 단(`Code · Stack · Level · LowerBead · UpperBead ·
StackHeight · Source · SamplePlc · SampleSeq · TotalHeight · EachHeight · At`, 키는 `Code + Stack + Level`, 필수도 그 셋).
값은 모두 **셀 바닥 기준 절대값**이고 `Source` 를 비우면 `manual` 이다. 줄이 하나라도 있으면 `Items` 시트에 있는
모든 코드에 대해 이 시트가 정본이다(그 코드의 줄이 없으면 프로파일 없음). `Items` 에 없는 코드의 줄은 이미
등록된 품목의 프로파일만 고친다(없는 코드면 그 줄은 건너뜀). **머리글만 있는 시트는 없는 시트로 본다** — 양식을
받아 `Items` 만 채워 올려도 이미 잰 비드가 지워지지 않는다(비드를 비우는 일은 화면의 Beads 탭). 하중 곡선은
콘솔이 파생하므로 시트로 오가지 않는다. 한 파일로 품목과 그 단별 비드를 같이 넣을 수 있다.

**오류와 적용 규칙** — 오류는 줄마다 하나씩 `Items row 5: count must be >= 1` · `ItemBeadProfile row 7: Code 9999: …`
처럼 시트와 줄을 앞세운다(csv 는 `row 5: …`). 저장된 값과 합쳐야 드러나는 오류(저장된 StackMax 보다 깊은 단
등)도 줄을 짚는다. **틀린 줄만 건너뛰고 나머지는 적용한다**(전부 아니면 전무가 아니다) — dry-run 과 실제 적용이
같은 규칙으로 세므로 미리보기의 `추가 · 갱신` 이 곧 적용될 수다. `skipped` = `errors` 의 수.

## 검증

| 규칙 | 결과 |
|---|---|
| `PickBeadOffset` ≥ 0, `Compression` ≥ 0, 유한수 | 어기면 400 |
| `PickBeadOffset` ≤ `Height` | 어기면 400 |
| `Compression` < `Height` | 어기면 400 |
| `PickBeadOffset` > `Height/2` | 경고(그립점이 타이어 아래쪽) |
| 눌림 × (StackMax−1) 이 `Height` 를 다 먹음 | 경고(단 높이를 1 mm 로 제한) |
| 프로파일 `Stack` 1..20, 중복 없음 | 어기면 400 |
| 프로파일 `Level` 1..`Stack`, 중복 없음 | 어기면 400 |
| 프로파일 `UpperBead > LowerBead`, 절대값 ≤ `Height × 1.5 × Level` | 어기면 400 |
| `Source` 가 `measured`·`manual` 이 아님 | 어기면 400 |
| 단이 올라가는데 절대 비드가 내려감 | 경고 |
| 하중이 커지는데 파생 곡선의 비드가 커짐 | 경고 |
| 잰 `TotalHeight` 와 `EachHeight × Stack`(또는 맨 윗단 `StackHeight`)이 5 mm 넘게 어긋남 | 경고 |

## 셀 바닥 Z 는 음수일 수 있다 — 막지 않고 경고한다 (2026-09-21)

`Cell.Position["Z"]`(바닥 Z)는 **바닥 평탄도 보정**이다. 명령 Z 는 어차피 `바닥 + 스택 + 그립` 으로 나가므로,
바닥이 기준면보다 낮은 셀은 음수·0 이 **맞는 값**이다 — 현장 CELL 표의 셀 301..305 가 −8.8 … −23.5 로 들어 있다.

그래서 콘솔은 바닥 Z ≤ 0 을 **저장·편집·가져오기·PLC 쓰기 어디서도 막지 않고**, 대신 한 자리마다 한 번씩 경고만 낸다.
막는 쪽은 데이터가 아니라 **작업**이다: GR2 `isValidTaskData`(`siemens/export/GR2_PLC/blocks/isValidTaskData.scl`, 79~83행)가
`Task.Cell.Position["Z"] <= 0` 인 작업을 `INVALID_CELL_POSZ`(305)로 돌려보낸다.

**검사 범위 주의(PLC 원문 확인 2026-09-21)** — 그 검사는 `IF #Task.Cell.Id > 2000` 안에 들어 있다.
`Task.Cell` 은 스테이션 대상일 때 스테이션 `Info` 가 들어가므로, 실제로 거부되는 것은 **스테이션 대상 작업**이고
셀 대상(id 1..1000)은 바닥 Z 가 음수여도 GR2 가 거부하지 않는다. 콘솔 경고는 두 경우에 모두 뜨고 문구에
`(검사는 Cell.Id > 2000 대상)` 을 달아 둔다.

| 값 | 콘솔 | PLC 작업 |
|---|---|---|
| 바닥 Z > 0 | 통과 | 통과 |
| 바닥 Z ≤ 0, 셀 대상(id ≤ 1000) | **경고**(저장됨) | 지금은 통과(검사가 `Cell.Id > 2000` 에만 걸린다) |
| 바닥 Z ≤ 0, 스테이션 대상(id > 2000) | **경고**(저장됨) | `INVALID_CELL_POSZ` 로 거부 |
| 바닥 Z 가 NaN/∞ | 400 (계산이 깨진다) | — |
| 셀 X/Y < 0 | 400 | — |

스테이션 레지스트리 자체는 그대로 `X/Y/Z > 0` 을 요구한다(PLC `isValid_Station_Parameter`) — 이번 완화는 셀 표에만 해당한다.

경고가 나오는 자리 — 값이 지나가는 길목마다 한 번씩:

| 자리 | 채널 |
|---|---|
| `POST/PUT /api/cells[/id]`, `GET /api/cells` | 셀 JSON 의 `warnings[]` (`registry::routes::cell_view`) |
| `POST /api/cells/bulk` (그리드 적용·레이아웃 생성, 모드는 `docs/layout-apply.md`) | 응답 `warnings[]` → 토스트 |
| `POST /api/cells/import`(PLC→로컬) · `import-file`(Excel/CSV, dry-run 포함) | 요약 `warnings[]` → 미리보기 목록 · 토스트 |
| `POST /api/cells/push`(로컬→PLC) | `PushResult.warnings[]` → 토스트 |
| `POST /api/issue/compose` | compose `warnings[]` → ComposeCard · 계획 미리보기 |
| 셀 그리드 편집기 | Z 칸이 경고 색 + 말풍선, 툴바 `· 경고 n` (오류 수에는 안 들어가고 **적용은 된다**) |
| 셀 추가·편집 폼 · 레이아웃 생성 규칙 | 경고 목록(저장·생성 버튼은 그대로) |

정본 문구는 백엔드 `registry::xlsx::floor_z_warning`, 프런트 `lib/task/registryGrid.ts::CELL_POSZ_WARNING`.

## PLC `p973` (Pick_BeadZOffset) — 콘솔이 목표를 정하고, PLC 는 다듬기만 한다

**로봇은 주는 대로 간다.** 콘솔이 명령한 Z 가 목표이고, `p973` 은 그 위에서 GR2 가 **추가로** 하는 하강 중
보정일 뿐이다 — 콘솔 `PickBeadOffset`(기본 30 mm)과 맞출 값이 아니다.

GR2 `PL_Task_V2` 는 PICK 하강 중 레이저가 본 비드 높이 − `p973`(`Task.Pick_BeadZOffset`,
`DEF_PICK_BEAD_Z_OFFSET := 20.0`) 을 목표 Z 로 래치할 수 있다. 조건은 둘 다 맞을 때뿐이다.

- 그 값이 **명령 Z 보다 위**일 것
- 명령 Z 로부터 `Item.Height × 0.3` 안일 것

즉 콘솔이 더 아래(더 깊이)를 명령하면 PLC 가 조금 올려 잡고, 콘솔이 더 위를 명령하면 PLC 는 가만히 둔다.
콘솔 `PickBeadOffset` 은 **명령값**이므로 그대로 두고 관리한다.

원문: `siemens/export/GR2_PLC/blocks/005. PL/Task_V2/PL_Task_V2.scl`(`DEF_PICK_BEAD_Z_OFFSET`),
PARA 슬롯 `p973`.

## 드래그 거리 기본값 — 150 mm (2026-09-18)

`DragInDist` · `DragOutDist` 를 요청도 기본값도 안 정했으면 compose 가 **150 mm**(`gr_proto::DEFAULT_DRAG_DIST`)
를 넣는다 — `issue::compose_from` 과 팔렛 `pallet::compose` 가 같은 값을 쓰므로 옛 "Dist = 0 경고" 는 더 이상
나오지 않는다. 저장된 기본값의 0 은 migration `0006_console_v2` 가 150 으로 옮긴다. `DragInHeight` · `DragOutHeight` 는
그대로 0 이면 지정 없음이다.

## MOVE 방식 — `params.move_mode` (2026-09-21)

| move_mode | Z | Avoid | 품목·수량 | GR2 동작 |
|---|---|---|---|---|
| `top` (화면 기본) | 9999 | 끔 | 불필요 | Z HomePos 유지 → 대상 XY 로 이동 → 끝 (하강·그립 없음) |
| `avoid` | 9999 | 켬 | 불필요 | 상단에서 **X 만** 이동, Y 는 지금 위치 유지 (회피) |
| `stack` (요청에 없을 때) | 바닥 + 스택 높이 + `move_clearance`(기본 500) | 끔 | 선택(없으면 셀 재고 품목) | 그 Z 까지 내려갔다 올라와 끝 |

근거(GR2_PLC): `isValidTaskData` 34행(MOVE ∧ Z=9999 는 Z 범위 검사 면제) · 38행(MOVE 는 G 범위 면제) ·
85행(MOVE 는 ItemCode 0 허용), `isValidTaskArea` 83행(Avoid 또는 MOVE ∧ Z=9999 면 영역 검사 전체 면제),
`PL_Task_V2` 299행 `isTaskMove` → 300 스텝 XY 이동 뒤 999(937·958행), Avoid 면 Y = 현재 위치(859–865행).
`stack` 에서 하강 MOVE 는 400 스텝 FLD 검사(6006)를 받으므로 여유를 너무 작게 잡지 않는다.
요청에 모드가 없으면 예전과 같은 `stack` 이고, 빈 셀이면 옛 "바닥 + 500" 과 값이 같다(재고가 있으면 그 위로 올라간다).
`move_mode` · `move_clearance` 는 `TaskParams` 밖의 키라 compose 가 원본 `params` JSON 에서 읽는다(기본값 `by.MOVE.<kind>` 도 가능).
