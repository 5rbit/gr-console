#!/usr/bin/env node
// 디자인 시스템 린터 — `docs/DESIGN.md`의 예산을 **기계가 지키게** 한다.
//
// 문서만으로는 안 지켜진다는 것을 이 저장소가 이미 증명했다: 컨트롤 높이 토큰을 쓰라는 주석이
// `tokens.css`에 있었는데도 `h-8` 하드코드가 33곳이었고, 그래서 전역 밀도가 반만 작동했다.
// 규칙이 진짜로 규칙이려면 `npm run check`가 막아야 한다.
//
// **기준선(baseline) 방식이다.** 이미 있는 위반을 한 번에 다 고치는 것은 이 변경의 범위가 아니고,
// 그렇다고 린터를 미루면 새 코드가 계속 늘어난다. 그래서 파일×규칙별 현재 개수를 기준선으로 박아
// 두고 **늘어나면 실패**한다. 줄이면 칭찬하고 기준선을 낮추라고 알려 준다(잠금 효과: 한 번 고친
// 자리는 되돌아갈 수 없다).
//
// 예외는 코드에 적는다: 그 줄이나 바로 윗줄에 `design-lint-allow: <규칙> — 이유`를 쓴다.
// 이유 없는 예외는 통과시키지 않는다(문자열에 ` — `가 있어야 한다).
//
// 사용:
//   node tools/design-lint.mjs            # 검사(기준선 초과면 exit 1)
//   node tools/design-lint.mjs --update   # 기준선 재작성(줄었을 때 잠그는 용도)
//   node tools/design-lint.mjs --list     # 규칙 설명
import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = join(HERE, '..')
const SRC = join(ROOT, 'src')
const BASELINE = join(HERE, 'design-lint.baseline.json')

/**
 * 킷(`src/lib/ui/*.tsx`)이 내보내는 컴포넌트 이름 — `no-kit-copy`가 이것을 과녁으로 쓴다.
 *
 * 이름을 손으로 적어 두지 않는다: 킷이 자라면 목록이 낡고, 낡은 목록은 새 사본을 놓친다.
 */
const KIT_NAMES = (() => {
  const names = new Set()
  for (const f of readdirSync(join(SRC, 'lib', 'ui'))) {
    if (!f.endsWith('.tsx')) continue
    const src = readFileSync(join(SRC, 'lib', 'ui', f), 'utf8')
    for (const m of src.matchAll(/^export function ([A-Z]\w+)/gm)) names.add(m[1])
  }
  return names
})()

/**
 * 규칙 한 개.
 * - `why` 위반 메시지에 그대로 나간다(무엇이 깨지는지 말한다 — "스타일 위반"은 아무것도 안 가르친다).
 * - `test` 줄을 받아 위반 문자열 배열을 돌려준다. 둘째 인자로 `{ lines, i }`(주석을 벗긴 사본과
 *   그 줄 번호)를 받아 **앞 줄을 볼 수 있다** — prettier 가 감싼 prop 은 값이 다음 줄에 있어서
 *   줄 하나만 보면 `emptyHint={` 와 그 문장이 서로를 못 본다.
 * - `only` 이 경로 접두에서만 본다(비면 전부).
 * - `skip` 이 경로 접두는 보지 않는다.
 */
const RULES = [
  {
    id: 'no-raw-color',
    why: '색값을 직접 쓰지 않는다 — `tokens.css`의 토큰(시맨틱 우선)으로 부른다. 팔레트가 바뀌면 이 자리만 남는다',
    ext: ['.tsx', '.ts'],
    // 로봇 색 정본이 `robots.ts` 에서 `robotContext.ts`(순수 모듈)로 옮겨 갔다 — 자리가 옮겨졌을 뿐 예외의 뜻은 같다.
    skip: ['src/tokens.css', 'src/lib/robots.ts', 'src/lib/robotContext.ts', 'src/lib/ui/viz/'],
    test: (l) =>
      [...l.matchAll(/#[0-9a-fA-F]{3,8}\b/g), ...l.matchAll(/\b(?:rgba?|hsla?)\(/g)].map(
        (m) => m[0],
      ),
  },
  {
    id: 'no-opacity-surface',
    why: '중립색 배경에 투명도를 섞어 새 면(5층)을 만들지 않는다 — 면은 4층(app·panel·inset·raised)이고 스케일 값으로 고른다',
    ext: ['.tsx'],
    /**
     * **중립색(white·slate·neutral·gray)만 본다.**
     *
     * 첫 판은 색을 가리지 않아서 `dark:bg-amber-500/15` 같은 것까지 46건을 잡았는데, 그건 면이
     * 아니라 **상태 soft 배경**이다(라이트의 `bg-amber-50`에 대응하는 다크 값). 다크 팔레트에
     * soft 토큰이 없어서 생긴 것이고, 그 사실은 `docs/DESIGN.md` 6절이 "알고 쓰는 절충"으로
     * 이미 기록해 뒀다 — 린터가 그걸 매일 다시 보고할 이유가 없다.
     *
     * 겨냥하는 것은 `bg-slate-200/70`·`bg-white/95`처럼 **면을 투명도로 한 층 더 만드는 것**이다
     * (백드롭 `bg-black/…`은 면이 아니라 가림막이라 예외).
     */
    test: (l) =>
      [...l.matchAll(/\bbg-(?:white|slate|neutral|gray)(?:-\d{2,3})?\/\d{1,3}\b/g)].map(
        (m) => m[0],
      ),
  },
  {
    id: 'no-arbitrary-value',
    why: '임의값(`text-[13px]`·`gap-[7px]`)을 쓰지 않는다 — 글자 크기·라운딩·간격은 토큰 스케일에서 고른다',
    ext: ['.tsx'],
    /**
     * **스케일이 실제로 있는 속성만 본다**(글자 크기 · 라운딩 · 간격).
     *
     * 첫 판에서는 폭·높이·위치·z·shadow까지 걸었는데, 남은 위반이 `w-[620px]`(대화상자 폭) ·
     * `max-h-[70vh]`(뷰포트 상한) · `max-w-[calc(100%-6rem)]` 같은 **스케일이 있을 수 없는 것들**
     * 이었다. 그것까지 막으면 `--spacing-dialog-620` 같은 가짜 토큰이 생기거나(더 나쁘다) 예외
     * 주석이 스물이 된다 — 예외가 스물인 린터는 아무도 켜 두지 않는다.
     *
     * 기하가 반복되면 임의값을 뿌리지 말고 **CSS 유틸리티로 모은다**(`app.css`의 `.ds-splitter*`가
     * 그렇게 나왔다). 그건 규칙이 아니라 관용이고, 리뷰가 본다.
     */
    test: (l) =>
      [
        ...l.matchAll(
          /\b(?:text|rounded|gap|gap-x|gap-y|p|px|py|pt|pb|pl|pr|m|mx|my|mt|mb|ml|mr|space-x|space-y)-\[[^\]]+\]/g,
        ),
      ].map((m) => m[0]),
  },
  {
    id: 'no-control-height',
    why: '컨트롤 높이를 숫자로 박으면 전역 밀도가 그 컴포넌트만 건너뛴다 — `h-control-sm/md`·`h-screen-header`·`h-menubar`를 쓴다',
    ext: ['.tsx'],
    // **프런트 전체**로 넓혔다(2026-09-18). 처음에는 킷·셸에만 걸었는데, 밀도가 건너뛰는 자리는
    // 화면 안에도 똑같이 생긴다 — 도구 띠의 `h-8` 하나면 그 띠만 다른 높이로 선다. `h-10`(40px)도
    // 같은 이유로 센다: 머리띠는 `h-screen-header` 가 정한다.
    test: (l) => [...l.matchAll(/\b(?:min-)?h-(?:7|8|9|10)\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-font-bold',
    why: '굵기는 400·500·600뿐이다 — 12px에서 700은 굵은 게 아니라 뭉갠 것이다',
    ext: ['.tsx'],
    test: (l) => [...l.matchAll(/\bfont-(?:bold|extrabold|black)\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-dark-variant',
    why: '`dark:` 짝을 쓰지 않는다 — 색을 시맨틱 토큰으로 부르면 `tokens.css`의 다크 층이 알아서 따라온다',
    ext: ['.tsx', '.ts'],
    // 프런트 전체. 킷·셸·화면 넷이 모두 `dark:` 0개로 정리됐고, 이 규칙은 그 상태가 되돌아가지
    // 못하게 잠그는 자리다(기준선 0 — 하나라도 생기면 실패).
    test: (l) => [...l.matchAll(/\bdark:[a-z-]+/g)].map((m) => m[0]),
  },
  {
    id: 'no-raw-palette',
    why: '원시 색 스케일(`bg-slate-100`·`text-indigo-600`·`border-l-indigo-600`)을 부르지 않는다 — 뜻으로 부른다(`bg-surface-inset`·`text-accent-text`·`border-l-info`)',
    ext: ['.tsx', '.ts'],
    /**
     * **방향·그라디언트 유틸리티까지 센다**(2026-09-18). 첫 판은 `bg|text|border|ring` 넷만 봐서
     * `border-l-indigo-600`(표 행의 종류 표식으로 실제로 쓰이던 모양) · `divide-slate-200` ·
     * `from-sky-500` 이 전부 통과했다. 접두 하나가 빠지면 그 접두로만 색이 새 나간다.
     */
    test: (l) =>
      [
        ...l.matchAll(
          /(?<![\w:/-])(?:hover:|focus-visible:|group-hover:|group-hover\/tab:|active:|peer-checked:|disabled:|placeholder:)*(?:bg|text|border|ring|divide|outline|decoration|shadow|fill|stroke|caret|accent|from|via|to)(?:-(?:x|y|s|e|t|r|b|l))?-(?:slate|neutral|gray|zinc|stone|indigo|emerald|green|amber|yellow|red|rose|orange|sky|blue|violet|pink)-\d{2,3}(?:\/\d{1,3})?\b/g,
        ),
      ].map((m) => m[0]),
  },
  {
    id: 'no-extra-font-size',
    why: '글자 크기는 `3xs·2xs·xs·sm` 넷이다 — `text-base` 이상은 값이 아니라 표제를 만든다(제목은 굵기와 색으로 세운다)',
    ext: ['.tsx'],
    test: (l) =>
      [...l.matchAll(/\btext-(?:base|lg|xl|2xl|3xl|4xl|5xl|6xl|7xl|8xl|9xl)\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-prose',
    why: '화면 본문에 문장을 쌓지 않는다 — 설명은 `?`(HelpTip) · 툴팁 · 빈 상태에 산다. 늘 서 있는 문단은 한 번 읽히고 자리는 매번 먹는다(`docs/DESIGN.md` 4절 ②)',
    ext: ['.tsx'],
    // 설명이 **살아도 되는 곳**: 도움말 모듈과 빈 상태·토스트 문구를 모아 둔 자리.
    skip: ['src/components/pallet/help.ts', 'src/lib/'],
    /**
     * 과녁은 **렌더되는 긴 리터럴**이다: JSX 텍스트 노드(`>…<`)와 긴 문자열 하나.
     *
     * 설명이 사는 자리(`title`·`hint`·`help`·`text`·`body`·`aria-label`·`placeholder`·
     * `emptyHint`·`disabledReason`·`why`·`reason`·`tooltip`)와 토스트·오류 메시지는 뺀다 —
     * 거기서는 문장이 맞는 답이다. 60자는 "한 줄 꼬리표"와 "문단"이 갈리는 자리다.
     *
     * **한글이 든 리터럴만 센다.** 첫 판은 길이만 봐서 60자 넘는 `className` 문자열을 전부 문단으로
     * 세었다(이 저장소에서 가장 긴 리터럴은 언제나 클래스 목록이다). 이 화면의 글은 한국어이므로
     * 한글 음절 하나를 조건에 넣으면 클래스·경로·쿼리가 통째로 빠진다.
     */
    test: (l, ctx) => {
      // 이 줄과 **앞 세 줄**을 함께 본다 — prettier 가 감싼 `emptyHint={`·`toast.ok(`·`const X_HELP =`
      // 는 문장이 다음 줄에 앉는다.
      const near = [l, ctx?.lines?.[ctx.i - 1] ?? '', ctx?.lines?.[ctx.i - 2] ?? '', ctx?.lines?.[ctx.i - 3] ?? ''].join('\n')
      if (
        /\b(?:title|hint|help|text|body|placeholder|label|emptyHint|empty|disabledReason|why|reason|tooltip|aria-label|ariaLabel|meta|message|desc|note)\s*[=:]/.test(
          near,
        )
      )
        return []
      // 설명을 모아 둔 상수(`*_HELP`·`*_HINT`·`*_TEXT`)는 **글이 사는 자리**다.
      if (/\b(?:const|let)\s+\w*(?:HELP|HINT|TEXT|NOTE|MSG|LABEL)\b/.test(near)) return []
      if (/\b(?:toast\.\w+|console\.\w+|new Error|throw )/.test(near)) return []
      const hits = []
      const korean = /[가-힣]/
      // 코드가 섞여 든 매치는 문단이 아니다(따옴표 짝이 코드를 건너뛰며 맞은 것) — 글에는 `{}<>=`
      // 도 백틱도 없다.
      const prose = (s) => korean.test(s) && !/[`{}<>=]/.test(s)
      for (const m of l.matchAll(/>\s*([^<>{}]{60,}?)\s*</g))
        if (prose(m[1])) hits.push(`텍스트 "${m[1].slice(0, 30)}…"`)
      for (const m of l.matchAll(/'([^'\\\n]{60,})'|"([^"\\\n]{60,})"/g)) {
        const s = m[1] ?? m[2]
        if (prose(s)) hits.push(`문자열 "${s.slice(0, 30)}…"`)
      }
      return hits
    },
  },
  {
    id: 'no-panel-paragraph',
    why: '패널 본문에 `<p>`를 두지 않는다 — 문단이 설 자리는 대화상자의 확인 문구와 빈 상태뿐이다. 값 화면의 설명은 `?` 뒤로 접는다',
    ext: ['.tsx'],
    only: ['src/components/'],
    test: (l) => [...l.matchAll(/<p[\s>]/g)].map(() => '<p>'),
  },
  {
    id: 'no-unit-in-cell',
    why: '단위는 값이 아니라 **머리글**에 붙인다(`Z (mm)` + 값 `12.3`) — 1초에 여러 번 갱신되는 열에서 값에 붙은 단위는 폭을 흔들어 눈이 따라가지 못한다(`docs/DESIGN.md` 4절 ③)',
    ext: ['.tsx'],
    /**
     * 과녁은 **JSX 값 자리**의 `{값} mm` 다. 템플릿 문자열 안(`` `… ${v} mm` ``)은 보지 않는다 —
     * 거기는 요약 문구·툴팁·도움말이고, 문장 안의 단위는 열 폭을 흔들지 않는다. 앞쪽 백틱 수가
     * 홀수면 그 매치는 템플릿 안이다.
     */
    test: (l) =>
      [...l.matchAll(/\}\s+(mm|kg|ms|㎜|°C|rpm)\b/g)]
        .filter((m) => (l.slice(0, m.index).split('`').length - 1) % 2 === 0)
        .map((m) => `} ${m[1]}`),
  },
  {
    id: 'no-stat-card-grid',
    why: '통계 카드 격자(값 하나를 보더+면+그림자로 감싼 카드 여럿)를 만들지 않는다 — 화면의 첫 3분의 1을 먹고 값이 서로 다른 x에 선다. `lib/ui/StatRow` 한 줄로.',
    ext: ['.tsx'],
    test: (l) =>
      /\bgrid-cols-(?:[3-9]|1[0-2])\b/.test(l) && /\brounded/.test(l) && /\bborder\b/.test(l)
        ? ['grid-cols-n + rounded + border']
        : [],
  },
  {
    id: 'no-modal-import',
    why: '`lib/ui/Modal`을 새로 부르지 않는다 — 표시용 팝업은 `Dialog`(footer 없이), 입력은 `FormDialog`, 예/아니오는 `ConfirmDialog`다. 셋이면 충분하고 넷째는 닫는 방법만 늘린다',
    ext: ['.tsx', '.ts'],
    skip: ['src/lib/ui/Modal.tsx'],
    test: (l) => (/from ['"][^'"]*ui\/Modal['"]/.test(l) ? ["import 'lib/ui/Modal'"] : []),
  },
  {
    id: 'no-kit-copy',
    why: '화면 폴더가 킷 컴포넌트와 **같은 이름**을 내보내지 않는다 — 두 벌이 되면 한쪽만 고쳐진다(실제로 `Section`·`OverflowMenu`·`InfoRows`가 그랬다). 킷을 고치거나, 다른 이름으로 감싼다',
    ext: ['.tsx'],
    only: ['src/components/'],
    test: (l) => {
      const m = /^export (?:function|const) ([A-Z]\w+)/.exec(l)
      return m && KIT_NAMES.has(m[1]) ? [`export ${m[1]} (킷에 이미 있다)`] : []
    },
  },
  {
    id: 'no-hand-table',
    why: '화면이 `<table>`을 손으로 짜지 않는다 — `DataTable`(읽는 표)·`DataGrid`(고치는 표)가 정렬·좁은 폭 열 접기·빈 상태·행 액션 폭을 이미 안다. 손으로 짠 표는 460px에서 넘친다',
    ext: ['.tsx'],
    only: ['src/components/'],
    test: (l) => [...l.matchAll(/<table[\s>]/g)].map(() => '<table>'),
  },
  {
    id: 'no-vh-in-pane',
    why: '뷰포트 비율 높이(`max-h-[62vh]`)를 패널 안에서 쓰지 않는다 — 도킹 존의 높이는 뷰포트와 무관해서 늘 어긋난다. 남는 높이(`min-h-0 flex-1`)로 잡는다',
    ext: ['.tsx'],
    // 진짜 오버레이(대화상자·명령 팔레트·패널 호스트·메뉴)는 뷰포트가 기준이 맞다.
    skip: [
      'src/lib/ui/Dialog.tsx',
      'src/lib/ui/Modal.tsx',
      'src/components/CommandPalette.tsx',
      'src/components/PanelHost.tsx',
      'src/App.tsx',
    ],
    test: (l) => [...l.matchAll(/max-h-\[\d+vh\]/g)].map((m) => m[0]),
  },
  {
    id: 'no-decoration',
    why: '그라디언트·블러·이모지는 쓰지 않는다 — 값이 알람인 화면에서 장식은 알람과 경쟁한다',
    ext: ['.tsx', '.ts', '.css'],
    test: (l) =>
      [
        ...l.matchAll(
          /\bbg-gradient-|\bbackdrop-blur\b|\bblur-(?:sm|md|lg|xl)\b|linear-gradient\(/g,
        ),
        ...l.matchAll(/[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/gu),
      ].map((m) => m[0]),
  },
  {
    id: 'no-pill',
    why: '알약(`rounded-full` + 가로 패딩)은 쓰지 않는다 — 글자를 담는 면은 4px 캡슐이고, 알약 모양은 점 하나뿐이다',
    ext: ['.tsx'],
    // 같은 줄에 `rounded-full`과 `px-`가 함께 있으면 글자를 담는 알약이다. 점(`h-1.5 w-1.5`)은 패딩이 없다.
    test: (l) => (/\brounded-full\b/.test(l) && /\bpx-\d/.test(l) ? ['rounded-full+px'] : []),
  },
]

/**
 * 주석 내용을 지운 사본(줄 수는 유지) — **린터가 자기 문서를 잡는 일이 없게.**
 *
 * 첫 판에서 `app.css`의 "글자 표식(★·NEW)은 쓰지 않는다"라는 **규칙을 적은 주석**이 이모지 규칙에
 * 걸렸고, `"PICK 셀#101"`이라는 예시 주석이 색값으로 걸렸다. 자기 규칙을 설명하는 문장을 위반으로
 * 세는 린터는 아무도 켜 두지 않는다.
 */
function stripComments(lines) {
  let inBlock = false
  return lines.map((raw) => {
    let out = ''
    let i = 0
    let q = null
    while (i < raw.length) {
      const two = raw.slice(i, i + 2)
      if (inBlock) {
        if (two === '*/') {
          inBlock = false
          i += 2
        } else i += 1
        out += ' '
        continue
      }
      if (q) {
        out += raw[i]
        if (raw[i] === '\\') {
          out += raw[i + 1] ?? ''
          i += 2
          continue
        }
        if (raw[i] === q) q = null
        i += 1
        continue
      }
      if (raw[i] === "'" || raw[i] === '"' || raw[i] === '`') {
        q = raw[i]
        out += raw[i]
        i += 1
        continue
      }
      if (two === '/*') {
        inBlock = true
        i += 2
        out += '  '
        continue
      }
      if (two === '//') {
        out += ' '.repeat(raw.length - i)
        break
      }
      out += raw[i]
      i += 1
    }
    return out
  })
}

// ── 파일 수집 ───────────────────────────────────────────────────────────────
function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name)
    if (statSync(full).isDirectory()) walk(full, out)
    else out.push(full)
  }
  return out
}

const files = walk(SRC)
  .map((f) => relative(ROOT, f).split(sep).join('/'))
  .filter((f) => !f.endsWith('.test.ts') && !f.endsWith('.test.tsx'))
  .sort()

// ── 검사 ────────────────────────────────────────────────────────────────────
/**
 * `design-lint-allow: <rule> — 이유` 가 이 줄이나 **바로 위 주석 블록**에 있나.
 *
 * 윗줄 하나만 보면 안 된다: 이유를 제대로 적으면 주석이 두 줄이 되고, 그러면 표식은 두 줄 위로
 * 밀려 예외가 먹지 않는다(첫 판에서 실제로 그렇게 놓쳤다). 그래서 위로 주석인 줄만 세 줄까지
 * 훑는다 — 코드 줄을 만나면 멈추므로 남의 예외가 이 줄까지 내려오지 않는다.
 * 이유(` — `)가 없는 표식은 예외로 받지 않는다.
 */
function allowed(rule, lines, i) {
  const re = new RegExp(`design-lint-allow:\\s*${rule}\\b[^\\n]*—`)
  if (re.test(lines[i] ?? '')) return true
  for (let k = i - 1; k >= 0 && i - k <= 3; k--) {
    const up = (lines[k] ?? '').trim()
    // JSX 주석(`{/* … */}`)도 주석이다 — 표 안에서는 그것 말고 쓸 수 있는 주석이 없다.
    if (!up.startsWith('//') && !up.startsWith('*') && !up.startsWith('/*') && !up.startsWith('{/*'))
      break
    if (re.test(up)) return true
  }
  return false
}

const found = {} // { "rule": { "path": [ {line, text} ] } }
for (const path of files) {
  const ext = path.slice(path.lastIndexOf('.'))
  const lines = readFileSync(join(ROOT, path), 'utf8').split('\n')
  // 검사는 주석을 벗긴 사본으로, 예외 주석 판정은 원본으로 한다.
  const code = stripComments(lines)
  for (const rule of RULES) {
    if (!rule.ext.includes(ext)) continue
    if (rule.only && !rule.only.some((p) => path.startsWith(p))) continue
    if (rule.skip?.some((p) => path.startsWith(p))) continue
    code.forEach((line, i) => {
      const hits = rule.test(line, { lines: code, i })
      if (hits.length === 0 || allowed(rule.id, lines, i)) return
      ;((found[rule.id] ??= {})[path] ??= []).push({ line: i + 1, text: hits.join(' · ') })
    })
  }
}

const counts = {}
for (const [id, byFile] of Object.entries(found)) {
  counts[id] = {}
  for (const [path, hits] of Object.entries(byFile)) counts[id][path] = hits.length
}

// ── 모드 ────────────────────────────────────────────────────────────────────
const arg = process.argv[2]
if (arg === '--list') {
  for (const r of RULES) console.log(`${r.id}\n  ${r.why}\n`)
  process.exit(0)
}
if (arg === '--update') {
  writeFileSync(BASELINE, JSON.stringify(counts, null, 2) + '\n')
  const total = Object.values(counts).reduce(
    (a, byFile) => a + Object.values(byFile).reduce((x, y) => x + y, 0),
    0,
  )
  console.log(`기준선을 다시 적었다 — 위반 ${total}건(파일 기준).`)
  process.exit(0)
}

let base = {}
try {
  base = JSON.parse(readFileSync(BASELINE, 'utf8'))
} catch {
  console.error(
    `기준선 파일이 없다: ${relative(ROOT, BASELINE)} — \`node tools/design-lint.mjs --update\``,
  )
  process.exit(2)
}

const news = []
const drops = []
for (const rule of RULES) {
  const now = counts[rule.id] ?? {}
  const was = base[rule.id] ?? {}
  for (const [path, n] of Object.entries(now)) {
    const b = was[path] ?? 0
    if (n > b) news.push({ rule, path, n, b })
  }
  for (const [path, b] of Object.entries(was)) {
    const n = now[path] ?? 0
    if (n < b) drops.push({ rule: rule.id, path, n, b })
  }
}

if (news.length > 0) {
  console.error('\n✗ 디자인 시스템 위반이 늘었다 (docs/DESIGN.md 5절 예산)\n')
  for (const { rule, path, n, b } of news) {
    console.error(`  ${rule.id}  ${path}  (${b} → ${n})`)
    console.error(`    ${rule.why}`)
    for (const h of found[rule.id][path].slice(0, 6))
      console.error(`    ${path}:${h.line}  ${h.text}`)
    console.error('')
  }
  console.error('고치거나, 정말 필요하면 그 줄에 이유를 적어 예외로 둔다:')
  console.error(
    '  // design-lint-allow: no-raw-color — 파형 채널 색표는 VSCode 확장과 공유하는 계약이다\n',
  )
  process.exit(1)
}

if (drops.length > 0) {
  console.log('디자인 린트 통과 — 위반이 줄었다. 기준선을 낮춰 되돌아가지 못하게 잠근다:')
  for (const d of drops) console.log(`  ${d.rule}  ${d.path}  (${d.b} → ${d.n})`)
  console.log('  node tools/design-lint.mjs --update')
} else {
  console.log('디자인 린트 통과 — 새 위반 없음.')
}
