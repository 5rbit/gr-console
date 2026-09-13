// 도킹 존 하나 — 탭 띠 + 활성 패널의 몸. Unity의 도킹 창, VSCode의 사이드바/패널 Part에 해당한다.
//
// 여기서 지키는 관용구 셋:
// ① **탭을 끌어 옮긴다.** 드래그 중인 탭은 `workspace.drag`가 전역으로 안다 — HTML5 DnD는
//    `dragover`에서 `dataTransfer`를 읽을 수 없어(보안), 놓을 자리를 미리 비추려면 스토어가 필요하다.
// ② **창 메뉴.** 탭 우클릭(과 ⋮ 버튼)이 최대화·존 이동·닫기를 낸다. Unity의 창 메뉴와 같은 내용이고,
//    드래그를 못 쓰는 사람에게는 이것이 유일한 이동 수단이라 메뉴가 드래그의 장식이 아니다.
// ③ **비활성 탭은 마운트하지 않는다.** 화면 넷이 동시에 살면 SSE·폴이 넷 다 돌고 보이지도 않는 표가
//    초당 여러 번 그려진다(`renderedPanes`가 같은 규칙을 계산한다).
import type * as React from 'react'
import { ChevronsDownUp, Minimize2, MoreVertical, PanelsTopLeft, X } from 'lucide-react'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { PaneChromeProvider } from '../../lib/ui/paneChrome'
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

/**
 * 버튼 좌표에서 창 메뉴를 연다 — `ctxMenu`는 우클릭 좌표를 받으므로 버튼의 왼쪽 아래를 넘긴다.
 *
 * **여는 클릭을 여기서 멈춰야 한다.** `ContextMenuHost`는 바깥 클릭을 window에서 듣는데, 버튼을
 * 누른 그 클릭이 계속 올라가면 열자마자 그 리스너가 닫는다 — 메뉴가 한 프레임도 안 보였다.
 * (우클릭 경로는 `ctxMenu.show`가 contextmenu 이벤트를 멈추므로 멀쩡했고, 그래서 이 버그가
 * 버튼에서만 났다.)
 */
function openPaneMenu(e: React.MouseEvent<HTMLElement>, items: MenuItem[]): void {
  e.stopPropagation()
  const r = e.currentTarget.getBoundingClientRect()
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

/** 문맥 값은 상수로 든다 — 렌더마다 새 객체를 만들면 그 아래 전부가 다시 그려진다. */
const PANE_TITLED = { titled: true }

/**
 * 탭 한 칸의 클래스.
 *
 * 강조가 **두 단계**인 이유: 화면에 탭 띠가 넷까지 동시에 서는데, 각 띠의 활성 탭을 다 같은 초록
 * 밑줄로 칠하면 "지금 어디를 만지고 있나"가 사라진다. 그래서 활성 **존**의 활성 탭만 accent 밑줄을
 * 받고, 다른 존의 활성 탭은 회색 밑줄로 "이 존에서는 이것"만 말한다(VSCode가 비활성 에디터 그룹의
 * 탭을 죽이는 것과 같은 이유).
 */
const tabCls = (active: boolean, inFocusedZone: boolean): string =>
  'group/tab flex max-w-[14rem] shrink-0 items-center gap-1.5 border-r border-line-default px-2 py-1 text-2xs transition-colors ' +
  (active
    ? 'bg-surface-panel font-medium text-content-primary ' +
      (inFocusedZone
        ? 'shadow-[inset_0_-2px_0_0_var(--color-accent)]'
        : 'shadow-[inset_0_-2px_0_0_var(--color-line-strong)]')
    : 'text-content-muted hover:bg-surface-inset hover:text-content-primary')

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
  const focused = workspace.isFocusedZone(zone)

  function dropAt(index?: number): void {
    workspace.drop(zone, index)
  }

  return (
    <section
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface-panel"
      data-testid={`zone-${zone}`}
      data-focused={focused ? '' : undefined}
      aria-label={`${ZONE_LABEL[zone]} 존`}
      // 이 면 안을 누르거나 포커스가 들어오면 활성 존이 된다 — `Alt+Enter`가 **보고 있는 것**을
      // 최대화하도록. 캡처 단계로 받는 이유: 안쪽 버튼이 `stopPropagation`을 해도 놓치지 않는다.
      onPointerDownCapture={() => workspace.focus(active)}
      onFocusCapture={() => workspace.focus(active)}
    >
      {/* ── 탭 띠 ── */}
      <div
        className={
          'flex h-control-sm shrink-0 items-stretch overflow-x-auto border-b border-line-default ' +
          // 면은 스케일에서 고른다 — `/70` 같은 투명도로 5층을 만들지 않는다(`docs/DESIGN.md` 5절).
          // 활성 존은 raised(200/700), 비활성은 inset(100/800).
          (focused ? 'bg-surface-active' : 'bg-surface-inset')
        }
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
          // 중앙의 마지막 탭은 닫히지 않는다 — 그러면 **닫기 버튼을 그리지 않는다**. 눌러도 아무 일도
          // 없는 버튼은 고장으로 읽힌다(회색으로 침묵시키는 것도 같은 잘못이다).
          const closable = !(zone === 'center' && state.panes.length === 1)
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={on}
              draggable
              className={tabCls(on, focused)}
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
                <span className="shrink-0 text-3xs text-content-faint">
                  <Summary />
                </span>
              ) : null}
              {closable ? (
                // 활성 탭의 닫기는 **늘 보인다**. hover에만 두면 닫는 법을 배울 자리가 없다
                // (마우스를 얹어 볼 이유가 없는 사람에게는 없는 기능이다).
                <span
                  role="button"
                  tabIndex={-1}
                  aria-label={`${d.label} 닫기`}
                  title="닫기"
                  className={
                    'ml-0.5 rounded p-0.5 text-content-faint hover:bg-surface-active hover:text-content-secondary ' +
                    (on ? 'inline' : 'hidden group-hover/tab:inline')
                  }
                  data-testid={`tab-close-${id}`}
                  onClick={(e) => {
                    e.stopPropagation()
                    workspace.close(id)
                  }}
                >
                  <X className="h-2.5 w-2.5" />
                </span>
              ) : null}
            </button>
          )
        })}

        <span className="flex-1" />

        {l.maximized ? (
          // 최대화 중에는 **나가는 길이 보여야 한다.** 단축키와 창 메뉴만 두면 갇힌 것처럼 느껴진다.
          <button
            type="button"
            className="flex shrink-0 items-center gap-1 px-1.5 text-3xs text-content-muted hover:bg-surface-active hover:text-content-primary"
            aria-label="최대화 해제"
            title="최대화 해제 — Alt+Enter"
            data-testid="zone-unmaximize"
            onClick={() => workspace.maximize(null)}
          >
            <Minimize2 className="h-3.5 w-3.5" />
            해제
          </button>
        ) : null}
        {active ? (
          <button
            type="button"
            className="shrink-0 px-1 text-content-faint hover:bg-surface-active hover:text-content-secondary"
            aria-label="창 메뉴"
            title="창 메뉴 — 최대화 · 존 이동 · 닫기"
            data-testid={`zone-menu-${zone}`}
            onClick={(e) => openPaneMenu(e, paneMenu(active))}
          >
            <MoreVertical className="h-3.5 w-3.5" />
          </button>
        ) : null}
        {zone !== 'center' ? (
          <button
            type="button"
            className="shrink-0 px-1 text-content-faint hover:bg-surface-active hover:text-content-secondary"
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
          // `titled` — 탭 띠가 이미 이 면의 이름을 말했다. 안쪽 `ScreenHeader`는 제목을 접고
          // 요약·조작만 남긴다(`lib/ui/paneChrome.ts`). 화면 코드는 이것을 모른다.
          <PaneChromeProvider value={PANE_TITLED}>
            <ErrorBoundary label={def?.label ?? '패널'} resetKey={active}>
              <Body />
            </ErrorBoundary>
          </PaneChromeProvider>
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
            className="absolute inset-0 z-20 m-1 rounded-md border-2 border-dashed border-accent bg-accent/10"
            data-testid={`drop-${zone}`}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault()
              dropAt()
            }}
          >
            <span className="m-1 inline-block rounded bg-accent px-1.5 py-0.5 text-3xs text-content-on-accent">
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
 *
 * **빈 존에도 드래그 중에는 이 레일이 뜬다.** 예전에는 패널이 하나도 없는 존이 화면에서 완전히
 * 사라져서, 탭을 끌고 있어도 놓을 자리가 없었다 — 존으로 옮기는 유일한 길이 창 메뉴였다. 놓을 수
 * 있는 자리는 **끌고 있는 동안 보여야** 한다.
 */
export function ZoneRail({ zone }: DockZoneProps) {
  const state = workspace.layout.zones[zone]
  const drag = workspace.drag
  const side = zone === 'bottom' ? 'row' : 'col'
  const empty = state.panes.length === 0

  return (
    <div
      className={
        (side === 'col'
          ? 'flex w-8 shrink-0 flex-col items-center gap-0.5 border-line-default py-1'
          : 'flex h-control-sm shrink-0 items-center gap-0.5 border-line-default px-1') +
        (zone === 'left' ? ' border-r' : zone === 'right' ? ' border-l' : ' border-t') +
        (empty && drag
          ? ' border-dashed border-accent bg-accent/10'
          : ' bg-surface-inset')
      }
      title={empty ? `${ZONE_LABEL[zone]} 존 — 탭을 여기에 놓으면 도킹된다` : undefined}
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
      {empty ? (
        <span className="grid flex-1 place-items-center text-3xs text-accent-text">
          <PanelsTopLeft className="h-4 w-4" />
        </span>
      ) : null}
      {state.panes.map((id) => {
        const d = paneDef(id)
        if (!d) return null
        const Icon = d.icon
        return (
          <button
            key={id}
            type="button"
            className="rounded p-1.5 text-content-muted hover:bg-surface-active hover:text-content-primary"
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
