// PARA 탭 — 사이드바에서 고른 로봇 PLC 의 PARA DB 를 그룹(최상위 구조체)별 표로 본다. 읽기 전용.
//
// 데이터는 GET /api/para?robot= (5 초 폴링, 숨은 탭 스킵) — 백엔드가 계약 선언 주석(`[p428] …`)과 형식을 붙여 준다.
// 쓰기를 두지 않는 이유: PARA 는 장비 자체 파라미터라 TIA·HMI 가 주인이다. 여기서는 "지금 그 호기에 무엇이
// 들어 있나"를 두 호기 사이를 오가며 대조하는 것이 일이다.
//
// 표는 **손으로 짠 `<table>` 로 남긴다**(2026-09-18 정리에서 유일하게 남긴 읽기 표): 그룹(비-leaf)
// 행이 `colSpan` 으로 서고 깊이만큼 들여쓰는 **트리**라, 행이 전부 같은 열을 쓰는 `DataTable` 의
// 계약에 들어가지 않는다. 억지로 넣으면 그룹 이름이 값 열에 서거나 깊이가 사라진다.
import { Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { api } from '../../lib/api'
import { dtl } from '../../lib/meas/format'
import { filterPara, formatParaValue, leafCount } from '../../lib/para'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import type { ParaSnapshot } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Input } from '../../lib/ui/Input'
import { Toolbar } from '../../lib/ui/Toolbar'
import { statusTone } from '../../lib/ui/status'

const POLL_MS = 5000
/** 그룹 안 깊이 → 이름 칸 들여쓰기(임의값 없이 킷 간격으로). */
const INDENT = ['', 'pl-4', 'pl-8', 'pl-12', 'pl-16'] as const

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function ParaView() {
  useStore(robots)
  const robot = robots.selected
  const [data, setData] = useState<ParaSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [q, setQ] = useState('')

  useEffect(() => {
    let alive = true
    setData(null)
    setError(null)
    const load = () =>
      api
        .para(robot)
        .then((d) => {
          if (!alive) return
          setData(d)
          setError(null)
        })
        .catch((e: unknown) => {
          if (alive) setError(errText(e))
        })
    void load()
    const t = visibleInterval(() => void load(), POLL_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [robot])

  const groups = useMemo(() => filterPara(data?.groups ?? [], q), [data, q])
  const total = useMemo(() => leafCount(data?.groups ?? []), [data])
  const shown = useMemo(() => leafCount(groups), [groups])

  if (!data)
    return (
      <EmptyState
        title={`${robots.current?.name ?? ''} PARA 없음`.trim()}
        hint={error ?? 'PARA 를 읽는 중입니다.'}
      />
    )

  return (
    <div className="space-y-3" data-testid="measure-para">
      {/* 검색이 이 화면의 주 조작이라 접지 않는다 — 한 번에 닿아야 하고, 지우는 손잡이도 같은 줄에 둔다. */}
      <Toolbar
        icon={<Search className="h-3.5 w-3.5" />}
        dense
        meta={
          <span data-testid="para-meta">
            로봇 {data.robot_name} · PLC {data.plc} ({data.contract}) · 읽은 시각 {dtl(data.at)} ·{' '}
            {q.trim() ? `${shown} / ${total}` : total} 항목
            {error ? <span className={statusTone('fault').text}> · {error}</span> : null}
          </span>
        }
      >
        <Input
          value={q}
          onValueChange={setQ}
          placeholder="이름·설명·p번호 (예: p428)"
          aria-label="PARA 검색"
          className="w-64"
          data-testid="para-search"
        />
        <Button
          size="sm"
          intent="ghost"
          disabled={q.trim() === ''}
          title={q.trim() === '' ? '검색어가 없습니다' : '검색어를 지웁니다'}
          onClick={() => setQ('')}
        >
          지우기
        </Button>
      </Toolbar>
      {groups.length === 0 ? (
        <EmptyState
          title="검색에 맞는 항목 없음"
          hint={`전체 ${total} 항목 — 이름·설명·p번호로 찾습니다.`}
          action={
            <Button size="sm" intent="ghost" onClick={() => setQ('')}>
              검색 지우기
            </Button>
          }
        />
      ) : null}
      {groups.map((g) => (
        <Card key={g.name} padded={false}>
          <details open>
            <summary className="flex cursor-pointer items-center gap-2 border-b border-line-default px-3 py-1.5">
              <span className="text-xs font-semibold">{g.name}</span>
              <span className="text-2xs text-content-muted">{g.comment}</span>
              <span className="ml-auto text-2xs text-content-faint tabular-nums">
                {g.rows.filter((r) => r.leaf).length}
              </span>
            </summary>
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="w-16 px-3 py-1 text-right">Param</th>
                    <th className="px-2 text-left">Name</th>
                    <th className="px-2 text-right">Value</th>
                    <th className="px-2 text-left">Type</th>
                    <th className="px-2 text-left">Comment</th>
                  </tr>
                </thead>
                <tbody>
                  {g.rows.map((r) =>
                    r.leaf ? (
                      <tr key={r.path} className="border-t border-line-subtle" title={r.path}>
                        <td className="px-3 py-1 text-right font-mono text-content-faint tabular-nums">
                          {r.param !== null ? `p${r.param}` : ''}
                        </td>
                        <td className={`px-2 font-mono ${INDENT[Math.min(r.depth, INDENT.length - 1)]}`}>
                          {r.name}
                        </td>
                        <td className="px-2 text-right font-mono tabular-nums">
                          {formatParaValue(r.value, r.ty)}
                        </td>
                        <td className="px-2 font-mono text-content-faint">{r.ty}</td>
                        <td className="px-2 text-content-muted">{r.comment}</td>
                      </tr>
                    ) : (
                      <tr key={r.path} className="border-t border-line-default bg-surface-inset">
                        <td className="px-3 py-1" />
                        <td
                          colSpan={4}
                          className={`px-2 font-semibold ${INDENT[Math.min(r.depth, INDENT.length - 1)]}`}
                        >
                          {r.name}
                          <span className="ml-2 font-normal text-content-faint">{r.ty}</span>
                          {r.comment ? (
                            <span className="ml-2 font-normal text-content-muted">{r.comment}</span>
                          ) : null}
                        </td>
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
            </div>
          </details>
        </Card>
      ))}
    </div>
  )
}
