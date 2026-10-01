// 작업 명령 오른쪽 카드 — 자동작업 · 수동작업(설계 7판). 단일 생성 탭은 없고 [+ 수동작업] 팝업이 그 자리다.
//
// 수동작업 = 계획(보내기 전, 이 브라우저) + 이 로봇 대기열의 사람이 넣은 작업(서버 정본, 나갈 순서대로). 맵 클릭은 계획 표에
// 쌓이고 사람이 고친 뒤 [대기열로] 로 넣는다 — 짝이 되자마자 대기열로 가면 고칠 틈이 없었다(2026-10-01). 보내기는 로봇마다
// 켜고 끄며(서버 루프), 거부되면 그 로봇은 멈추고 사람이 [다시 켜기] 를 누를 때까지 보내지 않는다.
import { useEffect, useState, type ReactNode } from 'react'
import { ChevronsUp, ListPlus, Plus, Send, Trash2, X } from 'lucide-react'
import { jobs as jobStore } from '../../lib/jobs/store'
import { kindOf, route, type Job } from '../../lib/jobs/model'
import type { PlanStep } from '../../lib/task/plan'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import type { GripRef, HandEntry, Item, StockEntry } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import type { MenuItem } from '../../lib/ui/menu'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Segmented } from '../../lib/ui/Segmented'
import { StatusDot } from '../../lib/ui/StatusDot'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import { api } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import { stock as stockStore } from '../../lib/stock'
import type { AreaView, Cell, Station } from '../../lib/types'
import { AnticolDialog } from '../task/AnticolDialog'
import { AreaStrip } from '../task/AreaStrip'
import { SyncIssuesDialog } from '../task/SyncIssuesDialog'
import { PlanTable } from './PlanTable'
import { TemplatesDialog } from './TemplatesDialog'

export type CardMode = 'auto' | 'manual'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

export function ManualCard({
  mode,
  onModeChange,
  autoGen,
  plan,
  onPlanChange,
  planReady,
  onEnqueue,
  onAdd,
  handNow,
  onEditHand,
  menu,
  cells,
  stations,
  items,
  stockNow,
  planHand,
  gripRef,
}: {
  mode: CardMode
  onModeChange: (m: CardMode) => void
  autoGen: ReactNode
  /** 계획 — 맵 클릭 · 측정 경로 · 팔렛이 쌓은 보내기 전 스텝. */
  plan: readonly PlanStep[]
  onPlanChange: (next: PlanStep[]) => void
  /** 계획에서 지금 대기열로 갈 수 있는 작업 수(짝이 맞은 PICK/DROP · 한 건 작업). */
  planReady: number
  /** 계획 앞에서부터 `limit` 건(없으면 전부)을 대기열에 넣고 넣은 수를 돌려준다. */
  onEnqueue: (limit?: number) => Promise<number>
  /** [+ 수동작업] 팝업 열기. */
  onAdd: () => void
  handNow: HandEntry | null
  /** Hand 지정 · 수정 창 열기. */
  onEditHand: () => void
  /** 넘침 메뉴(측정 경로 · 그립 기준). */
  menu: MenuItem[]
  /** 두 로봇 영역 띠의 축(설비 전체 X). */
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  stockNow: ReadonlyMap<number, StockEntry>
  /** 계획 시작 때 로봇이 들고 있을 화물(예상 Hand). */
  planHand: { item_code: number; count: number } | null
  gripRef: GripRef
}) {
  useStore(jobStore, robots)
  useEffect(() => jobStore.start(), [])
  const robot = robots.selected
  const name = robots.current?.name ?? '로봇'
  const d = jobStore.dispatch(robot)
  const order = d?.order ?? []
  const queue: Job[] = order
    .map((id) => jobStore.all.find((j) => j.id === id))
    .filter((j): j is Job => !!j)
  const manual = queue.filter((j) => j.origin === 'manual')
  const top = queue[0]
  const [busy, setBusy] = useState(false)
  // 동기화 경고(로봇 실제 상태 vs 콘솔 Hand · 이송 지시) — 옛 계획 카드에서 옮겨 왔다.
  useStore(stockStore)
  const sync = robots.current ? stockStore.syncIssues(robots.current.plc) : []
  const [syncOpen, setSyncOpen] = useState(false)
  const [templates, setTemplates] = useState(false)
  const fullMenu: MenuItem[] = [
    {
      label: '수동작업 목록 — 저장 · 불러오기 · 시나리오 가져오기…',
      run: () => setTemplates(true),
      testid: 'manual-templates',
    },
    ...menu,
  ]
  // 두 로봇 영역 — 로봇이 둘 이상일 때만 띠를 그린다(간격 바꾸기는 띠의 [간격]).
  const [area, setArea] = useState<AreaView | null>(null)
  const [anticolOpen, setAnticolOpen] = useState(false)
  const [areaNonce, setAreaNonce] = useState(0)
  const twoRobots = robots.list.length > 1
  useEffect(() => {
    if (!twoRobots) {
      setArea(null)
      return
    }
    let live = true
    const load = () =>
      void api
        .anticolView(robot, null)
        .then((v) => live && setArea(v))
        .catch(() => live && setArea(null))
    load()
    const t = visibleInterval(load, 1000)
    return () => {
      live = false
      clearInterval(t)
    }
  }, [twoRobots, robot, areaNonce])

  async function run(f: () => Promise<unknown>, ok?: string) {
    setBusy(true)
    try {
      await f()
      if (ok) toast.ok(ok)
    } catch (e) {
      toast.error(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const cols: Column<Job>[] = [
    {
      key: 'rank',
      label: '#',
      get: (j) => order.indexOf(j.id) + 1,
      cell: (j) => <span className="font-mono">{order.indexOf(j.id) + 1}</span>,
      numeric: true,
      priority: 1,
    },
    { key: 'kind', label: '종류', get: (j) => kindOf(j), priority: 2 },
    {
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
    {
      key: 'count',
      label: '개수',
      get: (j) => j.steps[0]?.request.count ?? 0,
      numeric: true,
      priority: 3,
    },
    {
      key: 'work',
      label: 'WorkId',
      get: (j) => j.work_id ?? 0,
      cell: (j) => <span className="font-mono">{j.work_id ?? ''}</span>,
      priority: 2,
    },
  ]

  return (
    <Card padded={false} className="flex flex-col" data-testid="manual-card">
      <div className="flex min-h-screen-header flex-none items-center gap-2 border-b border-line-default px-3 py-1">
        <Segmented<CardMode>
          ariaLabel="작업 종류"
          value={mode}
          onChange={onModeChange}
          options={[
            { id: 'auto', label: '자동작업', testid: 'side-auto' },
            {
              id: 'manual',
              label: '수동작업',
              badge: manual.length + plan.length || '',
              testid: 'side-manual',
            },
          ]}
        />
        <span className="flex-1" />
        {/* 보내기 — 수동 · 자동 작업 모두 이 스위치로 나간다(로봇마다 하나, 서버 루프). */}
        <Switch
          inline
          label={`${name} 보내기`}
          checked={!!d?.enabled}
          disabled={robot === null || busy}
          onCheckedChange={(v) => robot !== null && void run(() => jobStore.setEnabled(robot, v))}
          title="켜면 콘솔 서버가 대기열을 순서대로 보냅니다 — 다른 화면으로 가거나 창을 닫아도 계속됩니다"
          testid="manual-dispatch"
        />
        <Button
          size="sm"
          intent="ghost"
          className={cn(
            'text-2xs whitespace-nowrap',
            handNow && handNow.count > 0 ? 'text-content-secondary' : 'text-content-faint',
          )}
          title="그리퍼에 든 화물(콘솔 Hand) — 눌러서 지정 · 수정"
          disabled={robot === null}
          onClick={onEditHand}
          data-testid="manual-hand"
        >
          Hand: {handNow && handNow.count > 0 ? `${handNow.item_code} ×${handNow.count}` : '-'}
        </Button>
        {sync.length ? (
          <Button
            size="sm"
            intent="outline"
            className="text-warn-fg"
            title={sync.map((i) => i.message).join(' · ')}
            onClick={() => setSyncOpen(true)}
            data-testid="manual-sync"
          >
            동기화 {sync.length}
          </Button>
        ) : null}
        <OverflowMenu items={fullMenu} title="더 보기" testid="manual-more" />
      </div>
      {mode === 'auto' ? (
        <div className="min-h-0 min-w-0 overflow-auto p-3" data-testid="manual-card-auto">
          {autoGen}
        </div>
      ) : (
        <>
          <div className="flex flex-none flex-wrap items-center gap-2 border-b border-line-default px-3 py-1.5">
            <Button
              size="sm"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={onAdd}
              data-testid="manual-add"
            >
              수동작업
            </Button>
            <span className="flex-1" />
            <Button
              size="sm"
              intent="outline"
              icon={<ListPlus className="h-3.5 w-3.5" />}
              disabled={robot === null || busy || !planReady}
              title={
                planReady
                  ? `계획의 작업 ${planReady}건을 대기열에 넣습니다 — 보내기가 켜져 있으면 차례대로 나갑니다`
                  : plan.length
                    ? '짝이 맞은 PICK/DROP 이 없습니다 — PICK 다음에 놓을 곳을 누르세요'
                    : '계획이 비었습니다'
              }
              onClick={() => void run(() => onEnqueue())}
              data-testid="manual-enqueue"
            >
              대기열로{planReady ? ` ${planReady}` : ''}
            </Button>
            <Button
              size="sm"
              intent="primary"
              icon={<Send className="h-3.5 w-3.5" />}
              disabled={robot === null || busy || (!queue.length && !planReady) || !!d?.paused}
              title={
                d?.paused
                  ? `보내기 멈춤 — ${d.paused}`
                  : queue.length
                    ? '대기열 맨 앞 작업을 지금 한 건 보냅니다(규칙은 같다)'
                    : planReady
                      ? '계획 맨 앞 작업 한 건을 대기열에 넣고 바로 보냅니다'
                      : '대기열과 계획이 비었습니다'
              }
              onClick={() =>
                robot !== null &&
                void run(async () => {
                  // 옛 계획 카드의 "다음 1건 제출" — 대기열이 비었으면 계획 맨 앞 한 건을 넣고 보낸다.
                  // 보내기가 켜져 있으면 넣은 것은 서버 루프가 보낸다(여기서 또 부르면 "보낼 작업 없음").
                  if (!queue.length) {
                    if (!(await onEnqueue(1)) || d?.enabled) return
                  }
                  const why = await jobStore.next(robot)
                  if (why) toast.info(`${name}: 아직 안 보냄 — ${why}`)
                })
              }
              data-testid="manual-next"
            >
              다음 1건
            </Button>
          </div>
          {d?.paused ? (
            <div
              className="flex flex-none items-center gap-2 border-b border-line-default bg-warn-soft px-3 py-1.5 text-xs text-warn-fg"
              data-testid="manual-paused"
            >
              <StatusDot status="warn" size="sm" />
              <span className="min-w-0 flex-1">보내기 멈춤 — {d.paused}</span>
              <Button
                size="sm"
                onClick={() =>
                  robot !== null && void run(() => jobStore.resume(robot), `${name} 보내기 다시 켬`)
                }
              >
                다시 켜기
              </Button>
            </div>
          ) : null}
          {plan.length ? (
            <div
              className="flex min-h-0 flex-none flex-col border-b border-line-default"
              data-testid="manual-plan"
            >
              <div className="flex flex-none items-center gap-2 px-3 py-1 text-2xs text-content-muted">
                <span className="font-semibold text-content-secondary">계획 {plan.length}</span>
                <span className="min-w-0 flex-1 truncate">보내기 전 — 고친 뒤 [대기열로]</span>
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<Trash2 size={14} />}
                  title="계획 비우기(Ctrl+Z 로 되돌림)"
                  onClick={() => onPlanChange([])}
                  data-testid="manual-plan-clear"
                />
              </div>
              <div className="min-h-0 overflow-auto">
                <PlanTable
                  steps={plan}
                  onChange={onPlanChange}
                  cells={cells}
                  stations={stations}
                  items={items}
                  stockNow={stockNow}
                  gripRef={gripRef}
                  hand={planHand}
                  robot={robots.chip}
                />
              </div>
            </div>
          ) : null}
          <DataTable
            rows={manual}
            columns={cols}
            rowKey={(j) => j.id}
            density="compact"
            actions={(j) => (
              <span className="flex gap-1">
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<ChevronsUp size={14} />}
                  title="맨 앞으로"
                  disabled={order[0] === j.id}
                  onClick={() => void run(() => jobStore.front(j.id))}
                />
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<X size={14} />}
                  title="대기열에서 빼기(작업 취소)"
                  onClick={() =>
                    void run(() => jobStore.cancel(j.id), `WorkId ${j.work_id ?? '-'} 취소`)
                  }
                />
              </span>
            )}
            empty="대기열의 수동작업 없음"
            emptyHint="맵에서 PICK → DROP 을 눌러 계획을 만들고 [대기열로], 또는 [수동작업] 으로 넣습니다."
            emptyDense
            testid="manual-list"
          />
          {top ? (
            <div
              className="flex flex-none items-center gap-2 px-3 py-1.5 text-2xs text-content-muted"
              data-testid="manual-status"
            >
              <StatusDot status={top.wait_reason ? 'warn' : 'neutral'} size="sm" />
              <span className="truncate">
                다음 {top.work_id ?? ''} {route(top).from}
                {route(top).to ? ` → ${route(top).to}` : ''}
                {top.wait_reason ? ` · ${top.wait_reason}` : d?.enabled ? '' : ' · 보내기 꺼짐'}
              </span>
            </div>
          ) : null}
        </>
      )}
      {area ? (
        <AreaStrip
          view={area}
          xs={[...cells.map((c) => c.position[0]), ...stations.map((c) => c.info.position[0])]}
          onEdit={() => setAnticolOpen(true)}
        />
      ) : null}
      <TemplatesDialog open={templates} onClose={() => setTemplates(false)} />
      <SyncIssuesDialog
        open={syncOpen}
        onOpenChange={setSyncOpen}
        robot={robots.chip}
        robotId={robot}
        issues={sync}
      />
      {area ? (
        <AnticolDialog
          open={anticolOpen}
          onOpenChange={setAnticolOpen}
          view={area}
          onSaved={() => setAreaNonce((n) => n + 1)}
        />
      ) : null}
    </Card>
  )
}
