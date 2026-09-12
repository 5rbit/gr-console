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
    ui/                     UI 킷(Button·Input·Select·Switch·StatusDot·StatusBadge·JsonView·DataGrid·Modal·Toaster·ScreenHeader·…; 모델은 *Model.ts)
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
