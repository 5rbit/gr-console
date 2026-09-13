---
name: gr-design
description: GR 콘솔 프런트엔드(apps/gr-web)의 디자인·UX 규칙. gr-web의 화면·패널·도킹·레이아웃·탭·명령 팔레트·사이드바·상태바·표·차트·색·상태 배지·밀도·테마·빈 상태·단축키·접근성을 만들거나 고칠 때 사용한다. React/Tailwind 컴포넌트를 추가하거나 UI를 재배치하기 전에 읽는다. Use when creating or changing any gr-web UI — screens, panes, docking zones, layouts, tables, colors, status badges, density, theme, empty states, keyboard shortcuts.
---

# GR 콘솔 UI 작업

**규칙 본문은 `docs/DESIGN.md`에 있다. 코드를 고치기 전에 그 파일을 읽는다.**

여기에 규칙을 복제하지 않는다 — 두 벌은 반드시 갈리고, 갈린 순간 에이전트가 앱과 다른 규칙을 보게
된다(`src/tokens.css`의 주석이 이미 그 교훈을 적고 있다: 같은 토큰이 두 파일에 있었고 한쪽만
고쳐졌다).

## 순서

1. `docs/DESIGN.md`를 읽는다(10절 체크리스트까지).
2. 고칠 자리가 이미 있는 것으로 되는지 본다 — `src/lib/ui/`(UI 킷) · `src/lib/workspace/`(레이아웃
   순수 모델) · `src/components/workspace/paneRegistry.tsx`(패널 등록) · `src/lib/commands.ts`(명령).
   새 컴포넌트를 만들기 전에 **킷에 같은 일을 하는 것이 있는지** 먼저 확인한다.
3. `apps/gr-web/README.md`의 소유 규칙을 지킨다 — `lib/types.ts`·`lib/api.ts`·`App.tsx`·
   `paneRegistry.tsx`는 리드 소유다. 필요한 타입·엔드포인트·패널 등록이 없으면 리드에게 요청한다.
4. 고친 뒤 `docs/DESIGN.md` 10절 체크리스트로 자기 diff를 훑는다.
5. 검증: `cd apps/gr-web && npm run check && TZ=Asia/Seoul npm run test:unit && npm run build`.
   `check`에 **디자인 린트**가 들어 있다 — 색값 하드코드·임의값(`text-[13px]`)·투명도 면·굵기 700·
   그라디언트/블러/이모지·킷의 `h-8`을 막는다. 규칙 설명은 `npm run lint:design -- --list`.
   기준선(`tools/design-lint.baseline.json`)을 **올려서 통과시키지 않는다** — 고치거나, 그 줄에
   `// design-lint-allow: <규칙> — 이유`를 적는다(이유 없는 예외는 통과하지 않는다).
   (`TZ`가 필요한 이유는 `docs/DESIGN.md`와 플랜에 적혀 있다 — `state.test.ts`가 로컬 시간대에
   매여 있다.)

## 가장 자주 어기는 것 넷

- 한 줄에 조작과 값을 섞는다 → `Toolbar`(조작)와 표·`FieldList`·`ScreenHeader`(표시)를 가른다.
- 새 조작을 메뉴에만 넣는다 → **메뉴에 있는 것은 명령 팔레트에도 있다**.
- 색값이나 `dark:` 변형을 화면 안에 새로 쓴다 → 시맨틱 토큰(`bg-surface-panel` 류)을 쓴다.
  `dark:`는 셸 크롬만 쓴다(`docs/DESIGN.md` 6절의 절충을 먼저 읽는다).
- 비활성 버튼을 회색으로 침묵시킨다 → `disabled`에 **사유 문자열**을 넣는다.
