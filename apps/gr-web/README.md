# gr-web — GR 콘솔 프런트엔드

React 19 + Vite 8 + Tailwind v4 + TypeScript. 라우터·상태 라이브러리 없음(`lib/store.ts`의 `Store` +
`useSyncExternalStore`). sh4w-web의 셸·UI 킷을 옮겨 왔다.

**UI를 만지기 전에 `docs/DESIGN.md`를 읽는다** — 셸 골격·도킹 불변식·데이터 뷰 규칙·자리와 절제
예산·일관성 다섯 축·색·밀도·테마·접근성·체크리스트가 거기 있다. 규칙과 코드가 어긋나면 문서가
진실원이고 코드가 버그다. 셀 수 있는 예산은 `npm run check`의 디자인 린트가 막는다(기준선 방식 —
위반이 늘면 실패한다).

## 실행

```
npm install
npm run dev          # http://localhost:5173 — /api 는 GR_BACKEND(기본 http://127.0.0.1:8090)로 프록시
npm run check        # tsc(app + node) + 디자인 린트 — 이 하나가 통과해야 한다
npm run lint:design  # 디자인 시스템 예산만 검사 (tools/design-lint.mjs, --list 로 규칙 설명)
npm run test:unit    # vitest — src/**/*.test.ts 순수 모듈만(컴포넌트 테스트 없음)
                     # TZ=Asia/Seoul 을 붙인다 — lib/task/state.test.ts 의 endedToday 가 로컬 시간대에 매여 있다
npm run build        # dist/
```

백엔드가 없어도 뜬다 — 목록은 비고 SSE 점은 회색/빨강으로 남는다.

## 구조

셸은 본문이 **두 모양**이다(진실원은 `workspace.enabled` 하나). 단일 화면 모드 = 사이드바 + 화면
하나, 워크스페이스 모드 = 존 넷에 패널 도킹. 두 모양이 **같은 패널 컴포넌트**를 쓴다.

```
src/
  App.tsx                   셸: 메뉴바(그룹 + 보기 메뉴) · 본문 두 모양 · StatusBar · PanelHost · CommandPalette · ?tab= 딥링크 · 단축키 1~9
  main.tsx, app.css, tokens.css
  lib/
    types.ts                백엔드 페이로드 타입(공유 계약)
    api.ts                  REST 클라이언트(getJson/postJson/…/postForm/getBlobUrl) + `api` 객체 + STREAM_URL
    sse.ts, feeds.ts        SseFeed(참조 세기·rAF 코얼레싱) · statusFeed / tasksFeed / runsFeed
    plcs.ts                 /api/plcs 2초 폴 스토어
    tasks.ts                TasksEvent → Map 스토어(list/byState/counts, cancel/complete/resubmit)
    measlog.ts              측정 로그 스냅샷·항목·축 이력(300점) 스토어
    registry.ts             useRegistry<T>(loader) 훅
    nav.ts, tabs.ts         탭 상태·레지스트리 · 한 번 쓰는 신호(taskId/measSeq/measCode/scenarioId)
    workspace/              도킹 레이아웃 — model.ts(순수 연산·불변식) · presets.ts(배치 넷) · store.ts(현재 배치·저장·모드·드래그)
    commands.ts, palette.ts 명령 팔레트 — 명령의 모양·순수 검색 / 열림 상태
    density.ts              전역 표시 밀도(표준·조밀) — `<html data-density>` + app.css 토큰
    gr/const.ts             TASK_TYPE_CODE · MODE_NAME · KIND · STATUS · AXIS · PARAM_LABELS · STATE_LABEL/TONE
    store.ts, poll.ts, congestion.ts, share.ts, utils.ts, keys.ts, clipboard.ts, panels.ts, theme.ts
    ui/                     UI 킷(Button·Input·Select·Switch·StatusDot·StatusBadge·JsonView·DataGrid·Toaster·ScreenHeader·Dialog·DataTable·Section·Pair·InfoChip·HelpTip·…; 모델은 *Model.ts)
  components/
    Sidebar.tsx, StatusBar.tsx, PanelHost.tsx, CommandPalette.tsx
    workspace/              존 격자(WorkspaceShell) · 탭 띠·도킹·레일·창 메뉴(DockZone) · Splitter · paneRegistry(패널 등록)
    panes/                  보조 패널 — RobotsPane · PlcPane · StatusPane (사이드바와 도킹 모드가 함께 쓴다)
    shared/                 TaskParamFields · TargetPicker · ItemPicker
    task/TaskIssue.tsx      작업 명령      (스텁)
    taskmgr/TaskManager.tsx Task 관리      (스텁)
    measure/MeasureMonitor.tsx 측정 모니터 (스텁)
    scenario/ScenarioPage.tsx 시나리오     (스텁)
```

## UI 킷에 새로 든 것 (2026-09-18 — 조작·요소 줄이기)

화면에서 **보이는 요소와 누르는 횟수를 줄이려고** 다섯이 들어왔다. 전부 **덧붙이기**라 기존 컴포넌트의
prop·동작은 그대로다. 화면이 각자 만들던 폼 팝업·`⋯` 메뉴·통계 카드 격자를 여기로 모은다.

| 것 | 파일 | 쓰는 자리 |
| --- | --- | --- |
| `Dialog` · `FormDialog` · `useUnsavedGuard` | `lib/ui/Dialog.tsx`(+`dialogModel.ts`) | **입력·편집을 팝업으로** 옮기는 자리. 열리자마자 본문 첫 입력에 포커스 · Enter 제출 · Escape 취소 · 입력이 남아 있으면 바닥 띠가 한 번 되묻는다(두 번째 모달을 겹치지 않는다). 폭은 `sm/md/lg/xl` 넷 |
| `OverflowMenu` | `lib/ui/OverflowMenu.tsx` | 도구 띠·머리띠 **오른쪽 끝**의 `⋯`. 가끔 쓰는 조작을 걷어 낸다. 우클릭 도구 상자(`lib/ui/menu.ts`)를 그대로 열어 우클릭과 같은 모양이 뜬다. 표의 행에는 두지 않는다 |
| `StatRow` | `lib/ui/StatRow.tsx` | 화면 머리의 **숫자 한 줄**. 통계 카드 격자(카드 하나에 값 하나)를 대신한다 — 면을 만들지 않아 예산을 쓰지 않고, 값이 한 축에 서서 세로로 훑힌다 |
| `Field` | `lib/ui/Field.tsx` | 라벨+컨트롤+힌트 **간격 한 벌**. `Input`·`Select` 밖의 컨트롤(스위치·세그먼트·읽기 값)에 라벨을 붙일 때. `inline`이면 왼쪽 라벨 + 오른쪽 컨트롤 |
| `DataTable`의 `density`·`stickyHeader`·`zebra`·`className` | `lib/ui/DataTable.tsx` | 긴 표를 조밀하게(세로 여백만 줄인다 — 글자 크기는 그대로), 스크롤 상자 안에서 머리글 고정, 열이 여덟을 넘을 때만 줄무늬. `className`은 스크롤 상자에 붙는다(`max-h-96 overflow-y-auto`) |

### 마무리 판에서 더 든 것 (2026-09-18 — 화면 넷이 각자 만들던 것을 킷으로)

화면 정리 네 판이 끝나고 **같은 물건을 네 번 만든 자리**가 드러났다. 아래는 그것을 킷으로 올린 것과,
킷이 못 받아 줘서 화면이 우회하던 자리를 받아 준 prop 들이다. 전부 덧붙이기다(기존 호출부는 그대로).

| 것 | 파일 | 쓰는 자리 |
| --- | --- | --- |
| `Pairs` · `InfoRows` | `lib/ui/Pair.tsx` | **라벨+값 짝**. `Pairs`는 한 줄 띠(요약·대화상자 머리·캔버스 눈금), `InfoRows`는 두 열 표(팝오버 안). 팔렛이 손으로 짜던 `<dl>` 넷과 화물 규격의 사본을 대신한다 |
| `InfoChip` · `ChipPopover` | `lib/ui/InfoChip.tsx` | 값 한 조각을 띠에 세우고(`InfoChip`), 전체 표는 눌러서 연다(`ChipPopover`). 설명 문단을 걷어 낸 자리에 서는 것 |
| `Section` | `lib/ui/Section.tsx` | 카드 **안**의 소제목(제목 · `?` · 오른쪽 값 하나). 카드를 쪼개지 않고 1px 선으로 가른다. 측정 화면의 사본을 올린 것 |
| `useDismiss` | `lib/ui/useDismiss.ts` | 바깥 클릭·Escape 로 닫기. `IconPopover`·`HelpTip`·`ChipPopover`가 같은 규칙을 쓴다(전에는 두 벌이 `mousedown`/`click`으로 갈려 있었다) |
| `HelpTip`의 `sections`·`children` | `lib/ui/HelpTip.tsx` | `?` 안에 **소제목+본문 여러 칸**(공식·키 풀이·한계)이나 표를 넣는다. 이게 없어서 화면이 제 `HelpDot`을 만들었다 |
| `Column.label: ReactNode` · `Column.name` · `Column.help` | `lib/ui/table.ts` | 머리글에 노드를 세우고, 열 옆에 `?`를 단다. 읽어 주는 이름은 `name`(없으면 글자 라벨/`key`) |
| `DataTable`의 `emptyDense`·`rowDetail` | `lib/ui/DataTable.tsx` | 반 높이 카드의 **한 줄 빈 상태**, 그리고 행을 펼쳤을 때의 자세히(미리보기·PLC 구조). 액션 열은 언제나 `w-px whitespace-nowrap`이라 행 버튼이 0 폭으로 접히지 않는다 |
| `EmptyState`의 `compact` | `lib/ui/EmptyState.tsx` | `min-h-60` 없이 한 줄로 — 좁은 패널과 `<td colSpan>` 안 |
| `Input`의 `dense` | `lib/ui/Input.tsx` | 표 셀 안의 입력을 `Select dense`와 **같은 높이**(28px)로 |
| `OverflowMenu`의 `disabledReason` · `MenuItem.testid` | `lib/ui/OverflowMenu.tsx`·`menu.ts` | 잠근 `⋯`는 이유를 말한다(4절 ⑥). 항목의 testid 는 라벨 문구가 바뀌어도 스모크가 안 깨지게 |
| `StatItem`의 `help` | `lib/ui/StatRow.tsx` | 머리 숫자 옆 `?` — 네이티브 `title`은 지연이 길고 줄바꿈이 안 된다 |
| `Dialog`의 `closeLabel` | `lib/ui/Dialog.tsx` | **바로 적용되는 설정 팝업**의 바닥 띠(닫기 하나). "적용/취소"가 거짓말인 자리 |
| `ItemFields` · `FormErrors` | `components/task/forms.tsx` | 품목 입력 칸을 대화상자 껍데기에서 뗀 것 — 재고 팝업이 **모달 위 모달** 대신 같은 상자의 다음 단계로 쓴다 |

쓰는 규칙 셋(`docs/DESIGN.md` 3·4·5절의 연장):

- **주 조작은 한 번에 닿는다.** 화면의 주 조작은 띠에 하나(accent 하나 예산), 나머지는 `⋯`나 대화상자로.
  같은 값을 다시 보여 주기만 하는 중간 토글은 두지 않는다.
- **팝업은 갇히지 않는다.** `FormDialog`는 Enter 제출·Escape 취소가 기본이고, 저장 안 한 입력을
  조용히 버리지 않는다. 읽기용 팝업은 `Dialog`에 `footer` 없이 쓴다.
- **머리 숫자는 화면당 한 줄.** 하위 탭이 있어도 띠는 하나이고 자리가 바뀌지 않는다(측정 모니터가 그 예:
  `components/measure/MeasureMonitorModel.ts`의 `bandItems`가 순수 함수로 그 줄을 만든다).


## 소유 규칙

- `components/<page>/` — 화면별 담당 에이전트가 소유한다. 루트 컴포넌트는 **default export** 하나이고
  `App.tsx`의 `Screen` 스위치가 그것만 가리킨다. 화면 안의 하위 컴포넌트·훅은 그 폴더 안에 둔다.
- `lib/types.ts`, `lib/api.ts`, `App.tsx`, `components/workspace/paneRegistry.tsx` — **리드 소유**.
  필요한 타입·엔드포인트·패널 등록이 없으면 리드에게 요청한다(직접 고치지 않는다). `lib/*.ts` 스토어와
  `components/shared/`도 공용이라 바꾸면 알린다.
- 패널을 추가하는 절차와 도킹 불변식은 `docs/DESIGN.md` 2절에 있다. 패널 몸은 **자기 머리띠를 그리지
  않는다**(탭·닫기·최대화는 껍데기가 그린다).
- `lib/ui/` — sh4w-web과 같은 킷. 화면 전용 변형은 킷을 고치지 말고 화면 폴더에서 감싼다.

## 규약

- 파일 인코딩 UTF-8, 세미콜론 없음, 작은따옴표, printWidth 100 (`.prettierrc.json`).
- 컴포넌트 밖 상태는 `Store`를 상속하고 `notify()`로 알린다. 화면은 `useStore(store)`로 읽는다.
- 폴링은 `visibleInterval`(숨은 탭 스킵·혼잡 감속), 스트림은 `SseFeed`(`start()`가 해제 함수를 돌려준다).
- 테스트는 DOM 무관 순수 모듈만(`*.test.ts`). 컴포넌트 테스트는 두지 않는다.

## Windows 주의 — 파일 이름 대소문자

NTFS는 대소문자를 구분하지 않아 `Foo.tsx` 옆에 `foo.ts`가 있으면 `import … from './Foo'`가 `foo.ts`로
풀린다(tsc·rolldown 둘 다). 그래서 sh4w-web의 `jsonView.ts`·`fieldList.ts`·`opsPanel.ts`·`screenHeader.ts`는
여기서 `*Model.ts`로 이름을 바꿨다. **컴포넌트(`Foo.tsx`)와 대소문자만 다른 `.ts`를 같은 폴더에 만들지 않는다.**

## npm install이 `edgesOut` 오류로 실패하면

npm 10.9 + vitest 4.1의 peer 해석 버그다. `package-lock.json`이 있으면 재현되지 않는다 — 지우지 말 것.
지웠다면 sh4w-web의 lockfile을 복사해 넣고 `npm install`을 다시 돌린다(초과 패키지는 npm이 걷어 낸다).
