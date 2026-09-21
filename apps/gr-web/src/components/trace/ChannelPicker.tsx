// 채널 고르기 — 카탈로그 검색(디바운스) + DB 별 묶음 + 고른 것의 칩.
//
// 카탈로그는 멤버 수만 건이다 — **전부 받지 않는다**. 검색어 없이는 조회하지 않고, 조회에는 늘
// 상한을 건다(백엔드도 기본 500·최대 5000 으로 자른다). 경로는 PLC 이름 그대로라 번역하지 않는다.
import { useEffect, useRef, useState } from 'react'
import { Search, X } from 'lucide-react'
import { traceApi, type TraceChannel } from '../../lib/trace/api'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { traceColor } from '../../lib/ui/viz/traceSeries'
import { addChannel, groupByDb, removeChannel } from './traceModel'

const DEBOUNCE_MS = 250
const LIMIT = 200
const MIN_Q = 2

export interface ChannelPickerProps {
  selected: readonly string[]
  onChange: (next: string[]) => void
  chanMax: number
  /** 세션이 도는 동안에는 구성을 못 바꾼다. */
  disabled?: boolean
}

export function ChannelPicker({ selected, onChange, chanMax, disabled = false }: ChannelPickerProps) {
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<TraceChannel[]>([])
  const [total, setTotal] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // 늦게 온 응답이 최신 검색 결과를 덮지 않게 — 타자마다 왕복이 겹친다.
  const seq = useRef(0)

  useEffect(() => {
    const text = q.trim()
    if (text.length < MIN_Q) {
      setHits([])
      setTotal(0)
      setError(null)
      return
    }
    const mine = ++seq.current
    setBusy(true)
    const timer = setTimeout(() => {
      traceApi
        .channels(text, LIMIT)
        .then((r) => {
          if (seq.current !== mine) return
          setHits(r.channels)
          setTotal(r.total)
          setError(null)
        })
        .catch((e: unknown) => {
          if (seq.current !== mine) return
          setHits([])
          setError(e instanceof Error ? e.message : String(e))
        })
        .finally(() => {
          if (seq.current === mine) setBusy(false)
        })
    }, DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [q])

  const groups = groupByDb(hits)
  const full = selected.length >= chanMax

  return (
    <div className="space-y-2" data-testid="trace-picker">
      <div className="flex flex-wrap items-center gap-1">
        {selected.length === 0 ? (
          <span className="text-2xs text-content-muted">고른 채널 없음</span>
        ) : (
          selected.map((p, i) => (
            <span
              key={p}
              className="inline-flex items-center gap-1 rounded border border-line-default bg-surface-inset py-0.5 pr-1 pl-1.5 font-mono text-2xs"
              title={p}
            >
              <span
                className="inline-block h-2 w-2 rounded"
                style={{ background: traceColor(i) }}
                aria-hidden="true"
              />
              {p}
              <button
                type="button"
                className="text-content-faint hover:text-content-primary"
                aria-label={`${p} 빼기`}
                disabled={disabled}
                onClick={() => onChange(removeChannel(selected, p))}
              >
                <X className="h-3 w-3" />
              </button>
            </span>
          ))
        )}
        <span className={`ml-auto text-2xs tabular-nums ${full ? 'text-warn-fg' : 'text-content-muted'}`}>
          {selected.length} / {chanMax}
        </span>
        {selected.length > 0 ? (
          <Button size="sm" intent="ghost" disabled={disabled} onClick={() => onChange([])}>
            비우기
          </Button>
        ) : null}
      </div>

      <Input
        label="Channel search"
        value={q}
        onValueChange={setQ}
        disabled={disabled}
        placeholder="예: Axis[1].Position, WEBMON.Mode"
        hint={
          error
            ? error
            : q.trim().length < MIN_Q
              ? `${MIN_Q}자 이상 입력하면 찾습니다 (표준 접근 DB 의 BOOL·INT·DINT·REAL·TIME 만)`
              : busy
                ? '찾는 중…'
                : `${hits.length} / ${total}건${total > hits.length ? ' — 검색어를 좁히세요' : ''}`
        }
        mono
      />

      {groups.length > 0 ? (
        <div className="max-h-64 overflow-auto rounded border border-line-default">
          {groups.map((g) => (
            <div key={g.db}>
              <div className="sticky top-0 border-b border-line-default bg-surface-inset px-2 py-1 text-2xs font-medium text-content-muted">
                {g.db} ({g.channels.length})
              </div>
              <ul className="m-0 list-none p-0">
                {g.channels.map((c) => {
                  const on = selected.includes(c.path)
                  return (
                    <li
                      key={c.path}
                      className={`flex items-center gap-2 border-b border-line-subtle px-2 py-0.5 text-2xs last:border-0 ${on ? 'bg-accent-soft' : ''}`}
                    >
                      <span className="min-w-0 flex-1 truncate font-mono" title={c.path}>
                        {c.path}
                      </span>
                      <span className="whitespace-nowrap text-content-muted">{c.type_name}</span>
                      <span className="w-24 text-right font-mono tabular-nums whitespace-nowrap text-content-faint">
                        DB{c.db_no}.{c.offset}
                        {c.bit === null ? '' : `.${c.bit}`}
                      </span>
                      <Button
                        size="sm"
                        intent="ghost"
                        disabled={disabled || (!on && full)}
                        title={!on && full ? `채널은 ${chanMax}개까지입니다` : undefined}
                        onClick={() =>
                          onChange(
                            on
                              ? removeChannel(selected, c.path)
                              : addChannel(selected, c.path, chanMax),
                          )
                        }
                      >
                        {on ? '빼기' : '추가'}
                      </Button>
                    </li>
                  )
                })}
              </ul>
            </div>
          ))}
        </div>
      ) : q.trim().length >= MIN_Q && !busy && !error ? (
        <div className="flex items-center gap-1.5 px-1 text-2xs text-content-muted">
          <Search className="h-3 w-3" />
          맞는 채널이 없습니다
        </div>
      ) : null}
    </div>
  )
}
