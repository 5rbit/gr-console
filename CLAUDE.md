# gr-console — 에이전트 안내

겐트리 로봇(GR2) 엔지니어링 테스트 콘솔. Rust(axum) 백엔드 + React 프런트의 워크스페이스 하나다.

```
apps/gr-console/   백엔드(axum) — PLC S7 읽기/쓰기 · OPC UA 명령 · SSE · SQLite · 웹 서빙(패키지는 웹·계약 내장)
apps/gr-web/       프런트(React 19 + Vite + Tailwind v4) — 셸 · 화면 넷 · 도킹 워크스페이스
crates/            공유 크레이트(PLC 레이아웃·계약)
tools/gr-contract/ TIA 소스 → PLC 레이아웃 생성기
tools/package.*    배포 패키지 스크립트(cargo feature `embed` — README "패키지 만들기")
plc/contract/      TIA 소스(진실원)
docs/              DESIGN.md(UI 규칙) · ui-ux-plan.md(결정 기록) · screenshots/
```

## UI를 만지면 `docs/DESIGN.md`를 먼저 읽는다

프런트의 디자인·UX 규칙은 전부 거기 있다(셸 골격 · 도킹 불변식 · 데이터 뷰 규칙 · 자리와 절제 예산 ·
일관성 다섯 축 · 색·밀도·테마 · 접근성 · 체크리스트). 규칙과 코드가 어긋나면 **문서가 진실원이고
코드가 버그다**. `.claude/skills/gr-design`이 UI 작업에서 이 문서를 가리킨다.

예산 중 **셀 수 있는 것은 `npm run check`가 막는다**(색값 하드코드 · 임의값 · 투명도 면 · 굵기 700 ·
그라디언트/블러/이모지 · 알약 모양 · 킷의 컨트롤 높이 · **`dark:` 짝과 원시 색 스케일**). 기준선 방식이고
지금 기준선은 **0**이라 새 위반 하나가 곧 실패다 — 기준선을 올리는 커밋은 리뷰에서 막는다.

색은 **뜻으로 부른다**: `bg-surface-panel` · `text-content-muted` · `bg-warn-soft` ·
`text-accent-text`. 기본은 화이트톤이고 다크는 `tokens.css`의 `[data-theme='dark']` 한 블록이 든다 —
`dark:` 짝을 손으로 달 일이 없다.

## 자주 쓰는 명령

```
just                      # 레시피 목록
cargo check --workspace   # 백엔드 타입 검사
cargo clippy --workspace
cargo fmt

cd apps/gr-web
npm install
npm run dev               # http://localhost:5173 — /api 는 GR_BACKEND(기본 127.0.0.1:8090)로 프록시
npm run check             # tsc(app + node) + 디자인 린트
npm run lint:design       # 디자인 시스템 예산 검사(tools/design-lint.mjs)
TZ=Asia/Seoul npm run test:unit   # vitest — 순수 모듈만
npm run build

tools/package.sh          # 배포 패키지(실행 파일 하나 + 설정 + 안내문 → dist/*.zip). Windows: just package
```

`TZ=Asia/Seoul`: `apps/gr-web/src/lib/task/state.test.ts`의 `endedToday`가 로컬 시간대에 매여 있다
(UTC 환경에서 1건 실패한다). 이 프로젝트의 현장 시간대가 KST다.

## 규약

- 프런트 규약·구조·**소유 규칙**은 `apps/gr-web/README.md`. `lib/types.ts`·`lib/api.ts`·`App.tsx`·
  `components/workspace/paneRegistry.tsx`는 리드 소유 — 직접 고치지 말고 요청한다.
- 백엔드 슬라이스는 자기 모듈 밖을 건드리지 않고, 필요한 것은 각 모듈의 `LEAD_REQUESTS.md`에 적는다.
- 주석은 **왜**를 적는다(무엇을 하는지는 코드가 말한다). 한국어, UTF-8.
- 프런트: 세미콜론 없음 · 작은따옴표 · printWidth 100(`.prettierrc.json`). 테스트는 DOM 없는 순수
  모듈만(`*.test.ts`) — 컴포넌트 테스트는 두지 않는다.
- Windows(NTFS) 대소문자 주의: 컴포넌트 `Foo.tsx` 옆에 `foo.ts`를 만들지 않는다(모델은 `*Model.ts`).
