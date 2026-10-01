// 작업 표 — 시간순 세 묶음(대기 · 예정 → 할당 · 실행 → 완료 · 실패 · 취소). 설계 7판(작업 할당 재정립).
//
// 행이 덜 움직이게: ① 묶음마다 높이 고정(`ds-jobs-rows-*`) ② 묶음 안 순서 고정(WorkId 순, `stableSort`) —
// 우선이 바뀌면 순번만 바뀐다 ③ 단계가 바뀌면 행은 곧바로 새 묶음의 제자리로 가고 거기서 **새 단계 색**으로 한 번
// 깜박인다 ④ 사람이 눌러서 보고 있는 행 하나만 제자리에 남는다(상세 창을 닫으면 제 묶음으로). [정리] 같은 조작은 없다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { jobs as jobStore } from '../../lib/jobs/store'
import {
  GROUPS,
  STAGE_LABEL,
  cargoOf,
  groupOf,
  placeJobs,
  route,
  taskLabel,
  whoOf,
  type Job,
  type JobGroup,
} from '../../lib/jobs/model'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import type { Item } from '../../lib/types'
import { DataTable } from '../../lib/ui/DataTable'
import { ChipPopover } from '../../lib/ui/InfoChip'
import { InfoRows } from '../../lib/ui/Pair'
import type { Column } from '../../lib/ui/table'
import { cn } from '../../lib/utils'
import { JobDetailDialog } from './JobDetailDialog'
import { JobStageBadge } from './JobStageBadge'

const FLASH_MS = 1200

const ROWS: Record<'full' | 'compact', Record<JobGroup, string>> = {
  full: { queue: 'ds-jobs-rows-8', live: 'ds-jobs-rows-3', ended: 'ds-jobs-rows-6' },
  compact: { queue: 'ds-jobs-rows-5', live: 'ds-jobs-rows-3', ended: 'ds-jobs-rows-3' },
}

function hhmmss(s: string | null | undefined): string {
  return s ? s.replace('T', ' ').slice(11, 19) : ''
}

/** 이유 · 메모 칸 — 지금 왜 그런가 한 줄. */
function reasonOf(j: Job): string {
  if (j.error && j.stage === 'wait') return `거부: ${j.error}`
  if (j.wait_reason) return j.wait_reason
  if (j.stage === 'canceled') return '취소'
  if (j.stage === 'failed') return j.error ?? 'PLC 거부 · 실패'
  return ''
}

export function CargoCell({ job, items }: { job: Job; items: readonly Item[] }) {
  const c = cargoOf(job)
  if (!c.code) return <span className="text-content-faint">—</span>
  const it = items.find((i) => i.code === c.code)
  // 칩을 눌러도 행 클릭(상세 창)으로 번지지 않게 — 칩은 화물만 연다.
  return (
    <span onClick={(e) => e.stopPropagation()}>
      <ChipPopover
        chip={{ label: c.code, value: it?.name ?? '' }}
        title="화물"
        testid={`job-cargo-${job.id}`}
      >
        <InfoRows
          rows={[
            { label: 'ItemCode', value: String(c.code) },
            { label: '규격', value: it?.name ?? '등록 안 됨' },
            { label: 'InnerDiameter (mm)', value: it ? it.inner_diameter.toFixed(1) : '-' },
            { label: 'OuterDiameter (mm)', value: it ? it.outer_diameter.toFixed(1) : '-' },
            { label: 'Height (mm)', value: it ? it.height.toFixed(1) : '-' },
            { label: '개수', value: String(c.count) },
            { label: 'StackMax', value: it?.spec?.stack_max ? String(it.spec.stack_max) : '-' },
            { label: '바코드', value: it?.spec?.barcode || '-' },
          ]}
        />
      </ChipPopover>
    </span>
  )
}

export function JobsTable({
  robot = null,
  followRobot = false,
  q = '',
  variant = 'full',
  items,
  testid = 'jobs',
}: {
  /** 이 로봇 것만(없으면 전체). */
  robot?: number | null
  /** 사이드바에서 고른 로봇을 따라간다(`robot` 대신). */
  followRobot?: boolean
  /** 찾기 — WorkId · C101 · S2102 · 품목 코드. */
  q?: string
  variant?: 'full' | 'compact'
  items: readonly Item[]
  testid?: string
}) {
  useStore(jobStore, robots)
  useEffect(() => jobStore.start(), [])
  const want = followRobot ? robots.selected : robot
  const needle = q.trim().toUpperCase()
  const all = jobStore.all.filter((j) => {
    if (want !== null && j.robot !== want) return false
    if (!needle) return true
    const r = route(j)
    return [String(j.work_id ?? ''), r.from, r.to, String(cargoOf(j).code ?? '')].some((x) =>
      x.toUpperCase().includes(needle),
    )
  })
  const [held, setHeld] = useState<string | null>(null)
  // 지난 그림에서 각 행이 놓였던 묶음 — 눌러서 보는 행을 그 자리에 둔다.
  const placed = useRef(new Map<string, JobGroup>())
  // 단계 바뀜 깜박: 지난 단계를 기억하고, 바뀐 행은 1.2 초 동안 `ds-flash-stage`.
  const prevStage = useRef(new Map<string, Job['stage']>())
  const [flashing, setFlashing] = useState<ReadonlySet<string>>(new Set())
  const ver = jobStore.getSnapshot()
  useEffect(() => {
    const changed: string[] = []
    for (const j of jobStore.all) {
      const p = prevStage.current.get(j.id)
      if (p !== undefined && p !== j.stage) changed.push(j.id)
      prevStage.current.set(j.id, j.stage)
    }
    if (!changed.length) return
    setFlashing((f) => new Set([...f, ...changed]))
    const t = setTimeout(
      () => setFlashing((f) => new Set([...f].filter((id) => !changed.includes(id)))),
      FLASH_MS,
    )
    return () => clearTimeout(t)
  }, [ver])

  const groups = useMemo(() => placeJobs(all, held, placed.current), [all, held]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    for (const g of GROUPS) for (const j of groups[g.id]) placed.current.set(j.id, g.id)
  })

  const detail = held ? (jobStore.all.find((j) => j.id === held) ?? null) : null
  const robotName = (id: number | null) =>
    robots.list.find((r) => r.id === id)?.name ?? (id === null ? '미정' : `#${id}`)

  const cols = (g: JobGroup): Column<Job>[] => {
    const full = variant === 'full'
    const c: (Column<Job> | false)[] = [
      g === 'queue' && {
        key: 'rank',
        label: '순번',
        get: (j) => jobStore.rank(j) ?? 0,
        cell: (j) => <span className="font-mono">{jobStore.rank(j) ?? ''}</span>,
        numeric: true,
        priority: 2,
      },
      {
        key: 'work',
        label: 'WorkId',
        get: (j) => j.work_id ?? 0,
        cell: (j) =>
          j.work_id ? (
            <span className="font-mono">{j.work_id}</span>
          ) : (
            <span className="text-content-faint">—</span>
          ),
        priority: 1,
      },
      {
        key: 'task',
        label: 'Task',
        get: (j) => taskLabel(j),
        cell: (j) => <span className="font-mono">{taskLabel(j)}</span>,
        priority: 2,
      },
      {
        key: 'stage',
        label: '단계',
        get: (j) => STAGE_LABEL[j.stage],
        cell: (j) => <JobStageBadge stage={j.stage} />,
        priority: 1,
      },
      full
        ? {
            key: 'from',
            label: '출발',
            get: (j) => route(j).from,
            cell: (j) => <span className="font-mono">{route(j).from}</span>,
            priority: 1,
          }
        : {
            key: 'route',
            label: '출발 → 도착',
            get: (j) => `${route(j).from} ${route(j).to}`,
            cell: (j) => {
              const r = route(j)
              return (
                <span className="font-mono">
                  {r.from}
                  {r.to ? <span className="text-content-faint"> → </span> : null}
                  {r.to}
                </span>
              )
            },
            priority: 1,
          },
      full && {
        key: 'to',
        label: '도착',
        get: (j) => route(j).to,
        cell: (j) => <span className="font-mono">{route(j).to}</span>,
        priority: 1,
      },
      {
        key: 'cargo',
        label: '화물',
        get: (j) => cargoOf(j).code ?? 0,
        cell: (j) => <CargoCell job={j} items={items} />,
        priority: 2,
      },
      { key: 'count', label: '개수', get: (j) => cargoOf(j).count, numeric: true, priority: 3 },
      full && { key: 'robot', label: '로봇', get: (j) => robotName(j.robot), priority: 2 },
      full && { key: 'prio', label: '우선', get: (j) => j.priority, numeric: true, priority: 3 },
      full && { key: 'who', label: '주체', get: (j) => whoOf(j), priority: 3 },
      {
        key: 'why',
        label: '이유',
        get: (j) => reasonOf(j),
        cell: (j) => <span className={cn(j.error && 'text-fault-fg')}>{reasonOf(j)}</span>,
        priority: 2,
      },
      full && {
        key: 'at',
        label: '시각',
        get: (j) =>
          g === 'ended' ? (j.ended_at ?? '') : g === 'queue' ? j.created_at : j.updated_at,
        cell: (j) => (
          <span className="font-mono">
            {hhmmss(g === 'ended' ? j.ended_at : g === 'queue' ? j.created_at : j.updated_at)}
          </span>
        ),
        priority: 3,
      },
    ]
    return c.filter((x): x is Column<Job> => !!x)
  }

  return (
    <div className="flex min-h-0 flex-col" data-testid={testid}>
      {GROUPS.map((g) => {
        const rows = groups[g.id]
        return (
          <div key={g.id} className="flex flex-none flex-col" data-testid={`${testid}-${g.id}`}>
            <div className="flex items-center gap-2 border-y border-line-default bg-surface-inset px-3 py-1 text-xs font-semibold text-content-secondary">
              <span>{g.label}</span>
              <span className="font-mono font-medium text-content-muted">{rows.length}</span>
            </div>
            <DataTable
              rows={rows}
              columns={cols(g.id)}
              rowKey={(j) => j.id}
              selected={held}
              onPick={(j) => setHeld((h) => (h === j.id ? null : j.id))}
              density="compact"
              stickyHeader
              className={cn('overflow-y-auto', ROWS[variant][g.id])}
              rowAttrs={(j) => ({
                className: flashing.has(j.id) ? 'ds-flash-stage' : undefined,
                data: { stage: j.stage },
              })}
              empty={
                g.id === 'queue'
                  ? '대기 중인 작업 없음'
                  : g.id === 'live'
                    ? '나가 있는 작업 없음'
                    : '최근 끝난 작업 없음'
              }
              emptyDense
              testid={`${testid}-${g.id}-table`}
            />
          </div>
        )
      })}
      <JobDetailDialog
        job={detail}
        items={items}
        onClose={() => setHeld(null)}
        heldElsewhere={detail ? groupOf(detail.stage) !== placed.current.get(detail.id) : false}
      />
    </div>
  )
}
