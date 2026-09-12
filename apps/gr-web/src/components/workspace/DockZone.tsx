// 도킹 존 하나 — 탭 띠 + 활성 패널의 몸. Unity의 도킹 창, VSCode의 사이드바/패널 Part에 해당한다.
//
// 여기서 지키는 관용구 셋:
// ① **탭을 끌어 옮긴다.** 드래그 중인 탭은 `workspace.drag`가 전역으로 안다 — HTML5 DnD는
//    `dragover`에서 `dataTransfer`를 읽을 수 없어(보안), 놓을 자리를 미리 비추려면 스토어가 필요하다.
// ② **창 메뉴.** 탭 우클릭(과 ⋮ 버튼)이 최대화·존 이동·닫기를 낸다. Unity의 창 메뉴와 같은 내용이고,
//    드래그를 못 쓰는 사람에게는 이것이 유일한 이동 수단이라 메뉴가 드래그의 장식이 아니다.
// ③ **비활성 탭은 마운트하지 않는다.** 화면 넷이 동시에 살면 SSE·폴이 넷 다 돌고 보이지도 않는 표가
//    초당 여러 번 그려진다(`renderedPanes`가 같은 규칙을 계산한다).
import { ChevronsDownUp, MoreVertical, PanelsTopLeft, X } from 'lucide-react'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { ZONE_IDS, ZONE_LABEL, type ZoneId } from '../../lib/workspace/model'
import { workspace } from '../../lib/workspace/store'
import { paneDef } from './paneRegistry'

/** 이 패널의 창 메뉴 항목 — 탭 우클릭·⋮ 버튼·명령 팔레트가 같은 목록을 쓴다. */
export function paneMenu(pane: string): MenuItem[] {
  const def = paneDef(pane)
  if (!def) return []
  const here = workspace.zoneOf(pane)
  const l = workspace.layout
  const maxed = l.maximized === pane
  const lastInCenter = here === 'center' && l.zones.center.panes.length === 1

  return [
    { label: def.label },
    {
      label: maxed ? '최대화 해제' : '최대화',
      hint: 'Alt+Enter',
      run: () => workspace.maximize(pane),
    },
    ...ZONE_IDS.filter((z) => z !== here).map((z) => ({
      label: `${ZONE_LABEL[z]}으로 이동`,
      disabled: lastInCenter ? '중앙의 마지막 패널은 옮길 수 없습니다' : undefined,
      run: () => workspace.move(pane, z),
    })),
    {
      label: '닫기',
      disabled: lastInCenter ? '중앙의 마지막 패널은 닫을 수 없습니다' : undefined,
      run: () => workspace.close(pane),
    },
  ]
}

/** 버튼 좌표에서 메뉴를 연다 — `ctxMenu`는 우클릭 좌표를 받으므로 버튼의 왼쪽 아래를 넘긴다. */
function menuAt(el: HTMLElement, items: MenuItem[]): void {
  const r = el.getBoundingClientRect()
  ctxMenu.show(
    {
      clientX: r.left,
      clientY: r.bottom,
      preventDefault: () => {},
      stopPropagation: () => {},
    },
    items,
  )
}

const tabCls = (active: boolean): string =>
  'group/tab flex max-w-[14rem] shrink-0 items-center gap-1.5 border-r border-slate-200 px-2 py-1 text-[11px] transition-colors dark:border-slate-700 ' +
  (active
    ? 'bg-white font-medium text-slate-900 shadow-[inset_0_-2px_0_0_var(--color-accent)] dark:bg-slate-900 dark:text-slate-100'
    : 'text-slate-500 hover:bg-slate-100 hover:text-slate-800 dark:hover:bg-slate-800 dark:hover:text-slate-100')

export interface DockZoneProps {
  zone: ZoneId
}

export function DockZone({ zone }: DockZoneProps) {
  const l = workspace.layout
  const state = l.zones[zone]
  const drag = workspace.drag
  const active = state.active
  const def = active ? paneDef(active) : null
  const Body = def?.component ?? null
  const Summary = def?.summary ?? null
  /** 놓을 자리를 비추는 건 **다른 존에서 끌어온 것**일 때만 — 자기 존 안의 순서 바꾸기는 탭이 말한다. */
  const dropping = drag !== null && drag.from !== zone

  function dropAt(index?: number): void {
    workspace.drop(zone, index)
  }

  return (
    <section
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-white dark:bg-slate-900"
      data-testid={`zone-${zone}`}
      aria-label={`${ZONE_LABEL[zone]} 존`}
    >
      {/* ── 탭 띠 ── */}
      <div
        className="flex h-control-sm shrink-0 items-stretch overflow-x-auto border-b border-slate-200 bg-slate-100/70 dark:border-slate-700 dark:bg-slate-800/50"
        data-testid={`tabs-${zone}`}
        onDragOver={(e) => {
          if (drag) e.preventDefault()
        }}
        onDrop={(e) => {
          e.preventDefault()
          dropAt()
        }}
      >
        {state.panes.map((id, i) => {
          const d = paneDef(id)
          if (!d) return null
          const Icon = d.icon
          const on = id === active
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={on}
              draggable
              className={tabCls(on)}
              data-testid={`tab-${id}`}
              title={`${d.label} — 끌어서 다른 존으로, 우클릭으로 창 메뉴`}
              onClick={() => workspace.activate(id)}
              onDragStart={(e) => {
                e.dataTransfer.effectAllowed = 'move'
                // 빈 데이터로는 파이어폭스가 드래그를 시작하지 않는다.
                e.dataTransfer.setData('text/plain', id)
                workspace.beginDrag(id)
              }}
              onDragEnd={() => workspace.endDrag()}
              onDragOver={(e) => {
                if (drag) e.preventDefault()
              }}
              onDrop={(e) => {
                e.preventDefault()
                e.stopPropagation()
                // 탭의 **왼쪽 절반**에 놓으면 그 앞, 오른쪽 절반이면 그 뒤 — 브라우저 탭과 같은 감각.
                const r = e.currentTarget.getBoundingClientRect()
                dropAt(e.clientX < r.left + r.width / 2 ? i : i + 1)
              }}
              onContextMenu={(e) => ctxMenu.show(e, paneMenu(id))}
            >
              <Icon className="h-3 w-3 shrink-0 opacity-70" />
              <span className="truncate">{d.label}</span>
              {on && Summary ? (
                <span className="shrink-0 text-[10px] text-slate-400">
                  <Summary />
                </span>
              ) : null}
              <span
                role="button"
                tabIndex={-1}
                aria-label={`${d.label} 닫기`}
                title="닫기"
                className="ml-0.5 hidden rounded p-0.5 text-slate-400 group-hover/tab:inline hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
                data-testid={`tab-close-${id}`}
                onClick={(e) => {
                  e.stopPropagation()
                  workspace.close(id)
                }}
              >
                <X className="h-2.5 w-2.5" />
              </span>
            </button>
          )
        })}

        <span className="flex-1" />

        {active ? (
          <button
            type="button"
            className="shrink-0 px-1 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
            aria-label="창 메뉴"
            title="창 메뉴 — 최대화 · 존 이동 · 닫기"
            data-testid={`zone-menu-${zone}`}
            onClick={(e) => menuAt(e.currentTarget, paneMenu(active))}
          >
            <MoreVertical className="h-3.5 w-3.5" />
          </button>
        ) : null}
        {zone !== 'center' ? (
          <button
            type="button"
            className="shrink-0 px-1 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
            aria-label={`${ZONE_LABEL[zone]} 존 접기`}
            title="접기 — 탭은 기억한다"
            data-testid={`zone-collapse-${zone}`}
            onClick={() => workspace.toggleZone(zone, true)}
          >
            <ChevronsDownUp className="h-3.5 w-3.5" />
          </button>
        ) : null}
      </div>

      {/* ── 몸 ── */}
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {Body ? (
          <ErrorBoundary label={def?.label ?? '패널'} resetKey={active}>
            <Body />
          </ErrorBoundary>
        ) : (
          // 빈 면은 **다음 행동**을 말한다(`docs/DESIGN.md` 4절 ⑤) — "비었음"만 말하면 처음 쓰는
          // 사람은 여기에 무엇을 넣을 수 있는지 찾아 다닌다.
          <EmptyState
            icon={<PanelsTopLeft className="h-6 w-6" />}
            title="빈 존"
            hint="다른 존의 탭을 끌어다 놓거나, 메뉴바의 `보기`(또는 명령 팔레트)에서 패널을 엽니다."
          />
        )}

        {/* 드롭 면 — 다른 존의 탭을 끌고 있을 때만 깔린다(평소에는 DOM에 없다: 클릭을 먹지 않게). */}
        {dropping ? (
          <div
            className="absolute inset-0 z-20 m-1 rounded-md border-2 border-dashed border-indigo-400 bg-indigo-500/10"
            data-testid={`drop-${zone}`}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault()
              dropAt()
            }}
          >
            <span className="m-1 inline-block rounded bg-indigo-600 px-1.5 py-0.5 text-[10px] text-white">
              {ZONE_LABEL[zone]}에 도킹
            </span>
          </div>
        ) : null}
      </div>
    </section>
  )
}

/**
 * 접힌 존의 레일 — 아이콘 줄. VSCode의 Activity Bar와 같은 일을 한다: **접어도 무엇이 들어 있는지**
 * 남는다. 접힌 존을 그냥 지우면 사용자가 그 패널을 다시 찾는 길이 메뉴뿐이고, 그러면 아무도 안 접는다.
 */
export function ZoneRail({ zone }: DockZoneProps) {
  const state = workspace.layout.zones[zone]
  const drag = workspace.drag
  const side = zone === 'bottom' ? 'row' : 'col'

  return (
    <div
      className={
        (side === 'col'
          ? 'flex w-8 shrink-0 flex-col items-center gap-0.5 border-slate-200 py-1 dark:border-slate-700'
          : 'flex h-control-sm shrink-0 items-center gap-0.5 border-slate-200 px-1 dark:border-slate-700') +
        (zone === 'left' ? ' border-r' : zone === 'right' ? ' border-l' : ' border-t') +
        ' bg-slate-100/70 dark:bg-slate-800/50'
      }
      data-testid={`rail-${zone}`}
      aria-label={`${ZONE_LABEL[zone]} 존(접힘)`}
      onDragOver={(e) => {
        if (drag) e.preventDefault()
      }}
      onDrop={(e) => {
        e.preventDefault()
        workspace.drop(zone)
      }}
    >
      {state.panes.map((id) => {
        const d = paneDef(id)
        if (!d) return null
        const Icon = d.icon
        return (
          <button
            key={id}
            type="button"
            className="rounded p-1.5 text-slate-500 hover:bg-slate-200 hover:text-slate-900 dark:hover:bg-slate-700 dark:hover:text-slate-100"
            title={`${d.label} 펼치기`}
            aria-label={`${d.label} 펼치기`}
            data-testid={`rail-${id}`}
            onClick={() => workspace.activate(id)}
            onContextMenu={(e) => ctxMenu.show(e, paneMenu(id))}
          >
            <Icon className="h-4 w-4" />
          </button>
        )
      })}
    </div>
  )
}
