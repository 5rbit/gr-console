// 계층 데이터(JSON) 열람 — API 응답·알람 증거·surface dump·계약을 **모양을 미리 정하지 않고** 읽는다.
//
// 세 뷰가 같은 자료를 다르게 편다:
//   트리 — 구조를 본다(어디에 무엇이 들었나)
//   표   — 값을 훑는다(경로 · 값 · 종류 · 추세). 잎만 편다.
//   원문 — 그대로 퍼 간다(붙여넣기·이슈 첨부)
//
// 폴링으로 값이 갈리면 그 줄이 1.2초 물든다 — 1초에 여러 번 갱신되는 화면에서 유일한 질문은
// "무엇이 방금 바뀌었나"이기 때문이다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Copy } from 'lucide-react'
import { Sparkline } from './viz/Sparkline'
import {
  IND,
  ROW,
  typeOf,
  isBranch,
  VAL_CLASS,
  preview,
  matches,
  flatten,
  fmtValue,
  seedOpen,
  expandForFilter,
} from './jsonViewModel'
import type { FlatRow } from './jsonViewModel'
import { cn } from '../utils'

export interface JsonViewProps {
  value: unknown
  /** 트리 뿌리의 이름 — 경로의 첫 마디가 된다(`evidence.tick`). */
  rootLabel?: string
  /** 처음 펼칠 깊이. */
  defaultDepth?: number
  height?: number
  /** 검색어 — 키·값 어느 쪽이 맞아도 걸리고, 맞은 가지는 자동으로 펼쳐진다. */
  filter?: string
  view?: 'tree' | 'table' | 'raw'
  /** 값 변경 하이라이트 — 폴링으로 갱신되는 자리에서만 켠다. */
  highlightChanges?: boolean
  /** 경로를 집는다(복사·Watch 추가). */
  onPickPath?: (path: string, value: unknown) => void
  className?: string
}

interface RowProps {
  k: string
  v: unknown
  path: string
  depth: number
  open: Set<string>
  toggle: (path: string) => void
  q?: string
  onPickPath?: (path: string, value: unknown) => void
  changed?: Set<string>
  last?: boolean
}

// 트리 한 줄 — 자기 자신을 재귀로 부른다(가지면 자식을, 잎이면 값을).
function Row({
  k,
  v,
  path,
  depth,
  open,
  toggle,
  q = '',
  onPickPath,
  changed,
  last = false,
}: RowProps) {
  const [hover, setHover] = useState(false)
  const t = typeOf(v)
  const branch = isBranch(v)
  const expanded = branch && open.has(path)
  const entries = branch ? Object.entries(v as Record<string, unknown>) : []
  const shown = q ? entries.filter(([kk, vv]) => matches(vv, kk, q)) : entries

  // 접힌 가지도 물든다 — 그 안에서 값이 갈렸으면 접혀 있다는 이유로 변화를 숨기면 안 된다.
  const hit =
    !!changed &&
    (changed.has(path) ||
      (branch && !open.has(path) && [...changed].some((p) => p.startsWith(path + '.'))))

  return (
    <>
      <div
        className={cn(
          'flex items-center gap-1 font-mono text-xs whitespace-nowrap tabular-nums',
          branch && 'cursor-pointer font-medium',
          hover ? 'bg-surface-hover' : branch && depth > 0 ? 'bg-surface-inset' : '',
          hit && 'ds-flash',
        )}
        style={{
          height: ROW,
          minHeight: ROW,
          paddingLeft: 4 + depth * IND,
          paddingRight: 6,
        }}
        title={path}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        onClick={() => branch && toggle(path)}
      >
        {branch ? (
          expanded ? (
            <ChevronDown size={12} className="text-content-faint" />
          ) : (
            <ChevronRight size={12} className="text-content-faint" />
          )
        ) : (
          <span className="w-3 flex-none" />
        )}
        <span className="font-medium text-content-secondary">{k}</span>
        <span className="text-content-faint">:</span>
        {branch ? (
          <span className="text-content-faint">
            {expanded ? (t === 'array' ? '[' : '{') : preview(v)}
          </span>
        ) : t === 'string' ? (
          <span className={VAL_CLASS.string}>
            <span className="text-content-faint">&quot;</span>
            {String(v)}
            <span className="text-content-faint">&quot;</span>
          </span>
        ) : (
          <span className={VAL_CLASS[t] ?? ''}>{String(v)}</span>
        )}
        {/* 경로 복사는 **호버할 때만** 뜬다 — 줄마다 상시로 두면 아이콘이 값보다 많아진다. */}
        {hover && onPickPath ? (
          <button
            type="button"
            className="ml-auto inline-flex border-0 bg-transparent p-0 text-content-faint"
            title={`경로 복사 — ${path}`}
            aria-label="경로 복사"
            onClick={(e) => {
              e.stopPropagation()
              onPickPath(path, v)
            }}
          >
            <Copy size={12} />
          </button>
        ) : null}
      </div>

      {expanded ? (
        <div className="relative">
          {/* 세로 안내선 — 마지막 자식의 절반 높이에서 끊어 'ㄴ' 모양이 자연스럽게 닫힌다. */}
          <span
            className="absolute w-px bg-line-strong"
            style={{ top: 0, bottom: ROW / 2, left: 10 + depth * IND }}
          />
          {shown.map(([kk, vv], i) => (
            <Row
              key={path + '.' + kk}
              k={kk}
              v={vv}
              path={path + '.' + kk}
              depth={depth + 1}
              open={open}
              toggle={toggle}
              q={q}
              onPickPath={onPickPath}
              changed={changed}
              last={i === shown.length - 1}
            />
          ))}
          <div
            className="font-mono text-xs text-content-faint"
            style={{ height: ROW, minHeight: ROW, paddingLeft: 4 + depth * IND + 16 }}
          >
            {t === 'array' ? ']' : '}'}
            {last ? '' : ','}
          </div>
        </div>
      ) : null}
    </>
  )
}

export function JsonView({
  value,
  rootLabel = 'root',
  defaultDepth = 2,
  height = 260,
  filter = '',
  view = 'tree',
  highlightChanges = true,
  onPickPath,
  className = '',
}: JsonViewProps) {
  // ── 변경 추적 ───────────────────────────────────────────────────────────
  // 같은 폴에서 숫자 잎의 이력(최근 20 표본)도 모아 표 뷰의 추세선을 그린다.
  const prev = useRef<Map<string, string> | null>(null)
  const hist = useRef<Map<string, number[]>>(new Map())
  const [changed, setChanged] = useState<Set<string>>(() => new Set())

  const flat = useMemo(() => flatten(value, rootLabel, []), [value, rootLabel])

  useEffect(() => {
    if (!highlightChanges) return
    for (const r of flat) {
      if (typeof r.value !== 'number') continue
      const arr = hist.current.get(r.path) ?? []
      if (arr.length === 0 || arr[arr.length - 1] !== r.value) arr.push(r.value)
      if (arr.length > 20) arr.shift()
      hist.current.set(r.path, arr)
    }
    const now = new Map(flat.map((r) => [r.path, JSON.stringify(r.value)]))
    if (prev.current) {
      const hits = new Set<string>()
      now.forEach((v, k) => {
        if (prev.current!.get(k) !== v) hits.add(k)
      })
      if (hits.size) {
        setChanged(hits)
        const t = setTimeout(() => setChanged(new Set()), 1200)
        prev.current = now
        return () => clearTimeout(t)
      }
    }
    prev.current = now
  }, [flat, highlightChanges])

  // ── 펼침 상태 ───────────────────────────────────────────────────────────
  const seed = useMemo(
    () => seedOpen(value, rootLabel, defaultDepth),
    [value, rootLabel, defaultDepth],
  )
  const [open, setOpen] = useState(seed)
  useEffect(() => setOpen(seed), [seed])

  const effective = useMemo(
    () => expandForFilter(open, value, rootLabel, filter),
    [open, value, rootLabel, filter],
  )

  const toggle = (p: string) =>
    setOpen((prevOpen) => {
      const s = new Set(prevOpen)
      if (s.has(p)) s.delete(p)
      else s.add(p)
      return s
    })

  // ── 표 뷰 ───────────────────────────────────────────────────────────────
  const rows = useMemo(
    () =>
      flat.filter(
        (r) =>
          !filter ||
          r.path.toLowerCase().includes(filter.toLowerCase()) ||
          String(r.value).toLowerCase().includes(filter.toLowerCase()),
      ),
    [flat, filter],
  )
  const groupOf = (p: string) => p.slice(0, p.lastIndexOf('.'))

  // 드래그 범위 선택 — 여러 줄을 끌어 고르고 Ctrl/⌘+C로 `경로 : 값`을 그대로 복사한다.
  // (경로 열이 부모/잎 두 조각으로 그려지므로 브라우저 기본 선택에 맡기면 조각난 글자가 붙는다.)
  const [range, setRange] = useState<[number, number] | null>(null)
  const dragging = useRef(false)
  const inRange = (i: number) =>
    !!range && i >= Math.min(range[0], range[1]) && i <= Math.max(range[0], range[1])

  useEffect(() => {
    const up = () => {
      dragging.current = false
    }
    const onCopy = (e: ClipboardEvent) => {
      if (!range) return
      const [a, b] = [Math.min(range[0], range[1]), Math.max(range[0], range[1])]
      const text = rows
        .slice(a, b + 1)
        .map((r: FlatRow) => `${r.path} : ${fmtValue(r.value)}`)
        .join('\n')
      if (!text) return
      e.clipboardData?.setData('text/plain', text)
      e.preventDefault()
    }
    window.addEventListener('pointerup', up)
    document.addEventListener('copy', onCopy)
    return () => {
      window.removeEventListener('pointerup', up)
      document.removeEventListener('copy', onCopy)
    }
  }, [range, rows])

  return (
    <div
      className={cn(
        'overflow-auto rounded-md border border-line-default bg-surface-panel pb-0.5',
        className,
      )}
      style={{ maxHeight: height }}
    >
      {view === 'raw' ? (
        <pre
          className="m-0 p-2 font-mono text-xs whitespace-pre text-content-primary"
          style={{ lineHeight: 1.55 }}
        >
          {JSON.stringify(value, null, 2)}
        </pre>
      ) : view === 'table' ? (
        rows.length === 0 ? (
          <p className="m-0 p-3 text-2xs text-content-faint">값 없음</p>
        ) : (
          <table className="w-full border-collapse text-xs select-none">
            <thead>
              <tr className="bg-surface-inset text-2xs text-content-muted">
                <th className="border-b border-line-default px-2 py-1 text-left font-medium">
                  경로
                </th>
                <th className="border-b border-line-default px-2 py-1 text-left font-medium">값</th>
                <th className="border-b border-line-default px-2 py-1 text-left font-medium">
                  종류
                </th>
                <th className="w-16 border-b border-line-default px-2 py-1 text-left font-medium">
                  추세
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => {
                const g = groupOf(r.path)
                const leaf = g ? r.path.slice(g.length + 1) : r.path
                const t = typeOf(r.value)
                const h = hist.current.get(r.path)
                return (
                  <tr
                    key={r.path}
                    className={cn(
                      // 묶음 경계에만 진한 선 — 같은 태그 영역이 한 덩어리로 읽힌다.
                      i > 0 && groupOf(rows[i - 1].path) !== g && 'border-t border-line-strong',
                      inRange(i) && 'bg-accent-soft',
                      changed.has(r.path) && 'ds-flash',
                    )}
                    title={`${r.path} : ${fmtValue(r.value)}`}
                    onPointerDown={(e) => {
                      if (e.button !== 0) return
                      dragging.current = true
                      setRange([i, i])
                    }}
                    onPointerEnter={() => {
                      if (dragging.current) setRange((p) => (p ? [p[0], i] : [i, i]))
                    }}
                    onDoubleClick={() => onPickPath?.(r.path, r.value)}
                  >
                    {/* 경로는 늘 전체를 적되 **부모는 흐리게, 잎만 진하게**. */}
                    <td className="px-2 font-mono" style={{ height: ROW }}>
                      {g ? <span className="text-content-faint">{g}.</span> : null}
                      <span className="font-medium text-content-primary">{leaf}</span>
                    </td>
                    <td className="px-2 font-mono">
                      <span className={VAL_CLASS[t] ?? ''}>
                        {t === 'string' ? `"${String(r.value)}"` : String(r.value)}
                      </span>
                    </td>
                    <td className="px-2 text-content-muted">{t}</td>
                    <td className="px-2">
                      {/* 추세선은 표 한 칸의 **기호**다 — 값은 옆 칸이 말하므로 56×14로 짧게 둔다. */}
                      {t === 'number' && h && h.length >= 2 ? (
                        <span className="inline-flex text-accent-text">
                          <Sparkline values={h} width={56} height={14} />
                        </span>
                      ) : null}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )
      ) : (
        <Row
          k={rootLabel}
          v={value}
          path={rootLabel}
          depth={0}
          open={effective}
          toggle={toggle}
          q={filter}
          onPickPath={onPickPath}
          changed={changed}
          last
        />
      )}
    </div>
  )
}
