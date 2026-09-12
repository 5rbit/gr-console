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
 * 규칙 한 개.
 * - `why` 위반 메시지에 그대로 나간다(무엇이 깨지는지 말한다 — "스타일 위반"은 아무것도 안 가르친다).
 * - `test` 줄을 받아 위반 문자열 배열을 돌려준다.
 * - `only` 이 경로 접두에서만 본다(비면 전부).
 * - `skip` 이 경로 접두는 보지 않는다.
 */
const RULES = [
  {
    id: 'no-raw-color',
    why: '색값을 직접 쓰지 않는다 — `tokens.css`의 토큰(시맨틱 우선)으로 부른다. 팔레트가 바뀌면 이 자리만 남는다',
    ext: ['.tsx', '.ts'],
    skip: ['src/tokens.css', 'src/lib/robots.ts', 'src/lib/ui/viz/'],
    test: (l) => [
      ...l.matchAll(/#[0-9a-fA-F]{3,8}\b/g),
      ...l.matchAll(/\b(?:rgba?|hsla?)\(/g),
    ].map((m) => m[0]),
  },
  {
    id: 'no-opacity-surface',
    why: '배경에 투명도를 섞어 새 면(5층)을 만들지 않는다 — 면은 4층(app·panel·inset·raised)이고 스케일 값으로 고른다',
    ext: ['.tsx'],
    // 오버레이 백드롭(`bg-black/30`)은 면이 아니라 가림막이라 예외다.
    test: (l) =>
      [...l.matchAll(/\bbg-(?!black\/)[a-z]+-\d{2,3}\/\d{1,3}\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-arbitrary-value',
    why: '임의값(`text-[11px]`·`h-[7px]`)을 쓰지 않는다 — 크기·간격·라운딩은 토큰 스케일에서 고른다',
    ext: ['.tsx'],
    test: (l) =>
      [
        ...l.matchAll(
          /\b(?:text|h|w|min-h|max-h|min-w|max-w|p|px|py|pt|pb|pl|pr|m|mx|my|mt|mb|ml|mr|gap|rounded|top|bottom|left|right|z|shadow)-\[[^\]]+\]/g,
        ),
      ].map((m) => m[0]),
  },
  {
    id: 'no-control-height',
    why: '컨트롤 높이를 숫자로 박으면 전역 밀도가 그 컴포넌트만 건너뛴다 — `h-control-sm/md`·`h-screen-header`·`h-menubar`를 쓴다',
    ext: ['.tsx'],
    // 킷과 셸 크롬에만 건다(화면 내부는 기준선으로 잠근다).
    only: ['src/lib/ui/', 'src/components/workspace/', 'src/components/panes/', 'src/App.tsx',
      'src/components/StatusBar.tsx', 'src/components/Sidebar.tsx'],
    test: (l) => [...l.matchAll(/\b(?:min-)?h-(?:7|8|9)\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-font-bold',
    why: '굵기는 400·500·600뿐이다 — 12px에서 700은 굵은 게 아니라 뭉갠 것이다',
    ext: ['.tsx'],
    test: (l) => [...l.matchAll(/\bfont-(?:bold|extrabold|black)\b/g)].map((m) => m[0]),
  },
  {
    id: 'no-decoration',
    why: '그라디언트·블러·이모지는 쓰지 않는다 — 값이 알람인 화면에서 장식은 알람과 경쟁한다',
    ext: ['.tsx', '.ts', '.css'],
    test: (l) => [
      ...l.matchAll(/\bbg-gradient-|\bbackdrop-blur\b|\bblur-(?:sm|md|lg|xl)\b|linear-gradient\(/g),
      ...l.matchAll(/[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/gu),
    ].map((m) => m[0]),
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
        if (two === '*/') { inBlock = false; i += 2 } else i += 1
        out += ' '
        continue
      }
      if (q) {
        out += raw[i]
        if (raw[i] === '\\') { out += raw[i + 1] ?? ''; i += 2; continue }
        if (raw[i] === q) q = null
        i += 1
        continue
      }
      if (raw[i] === '\'' || raw[i] === '"' || raw[i] === '`') { q = raw[i]; out += raw[i]; i += 1; continue }
      if (two === '/*') { inBlock = true; i += 2; out += '  '; continue }
      if (two === '//') { out += ' '.repeat(raw.length - i); break }
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
/** `design-lint-allow: <rule> — 이유` 가 이 줄이나 윗줄에 있나(이유 없는 예외는 안 받는다). */
function allowed(rule, lines, i) {
  const re = new RegExp(`design-lint-allow:\\s*${rule}\\b[^\\n]*—`)
  return re.test(lines[i] ?? '') || re.test(lines[i - 1] ?? '')
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
      const hits = rule.test(line)
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
  console.error(`기준선 파일이 없다: ${relative(ROOT, BASELINE)} — \`node tools/design-lint.mjs --update\``)
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
  console.error('  // design-lint-allow: no-raw-color — 파형 채널 색표는 VSCode 확장과 공유하는 계약이다\n')
  process.exit(1)
}

if (drops.length > 0) {
  console.log('디자인 린트 통과 — 위반이 줄었다. 기준선을 낮춰 되돌아가지 못하게 잠근다:')
  for (const d of drops) console.log(`  ${d.rule}  ${d.path}  (${d.b} → ${d.n})`)
  console.log('  node tools/design-lint.mjs --update')
} else {
  console.log('디자인 린트 통과 — 새 위반 없음.')
}
