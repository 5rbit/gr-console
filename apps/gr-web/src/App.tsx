// 셸 — 메뉴바(그룹 드롭다운 · 보기) · 본문(단일 화면 또는 워크스페이스) · 상태바 · 명령 팔레트.
// **리드 소유** — 화면(`components/<page>/`)은 각 담당 에이전트가 갈고, 여기서는 그것을 어디에
// 앉힐지만 정한다.
//
// 본문이 두 모양인 이유(유니티·VSCode 리서치 결과 — `docs/ui-ux-plan.md`):
//   · **단일 화면 모드** — 사이드바 + 화면 하나. 처음 오는 사람이 길을 잃지 않는 바닥이다.
//   · **워크스페이스 모드** — 존 넷에 패널을 도킹하고(`components/workspace/`) 배치를 저장한다.
//     명령을 내면서 결과를 보고 값을 파는 일이 한 화면에서 끝난다.
// 진실원은 `workspace.enabled` 하나고, 두 모양이 **같은 패널 컴포넌트**를 쓴다(`paneRegistry`).
//
// 명령 팔레트는 다시 들어왔다(sh4w에서 걷어 냈던 것) — 도킹·프리셋·존·밀도까지 조작이 늘어나면
// 메뉴만으로는 닿지 않는다. 규칙은 VSCode와 같다: **메뉴에 있는 것은 팔레트에도 있다**.
//
// 셸·패널·표·색·밀도의 규칙은 `docs/DESIGN.md`가 진실원이다. 이 파일을 고치기 전에 그것을 읽는다.
import { useCallback, useEffect, useRef, useState } from 'react'
import type * as React from 'react'
import { Check, ChevronDown, Columns3, Menu, Moon, Search, Sun } from 'lucide-react'

import { nav, type Tab } from './lib/nav'
import { ALL_TABS, TAB_GROUPS, type TabDef, type TabGroupId } from './lib/tabs'
import { theme } from './lib/theme'
import { api } from './lib/api'
import { density } from './lib/density'
import { palette } from './lib/palette'
import { panels } from './lib/panels'
import { chord } from './lib/keys'
import { toast } from './lib/ui/toast'
import { useStore } from './lib/store'
import type { ConsoleInfo } from './lib/types'
import { ZONE_IDS, ZONE_LABEL } from './lib/workspace/model'
import { PRESETS, DEFAULT_PRESET } from './lib/workspace/presets'
import { workspace } from './lib/workspace/store'

import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { PanelHost } from './components/PanelHost'
import { CommandPalette } from './components/CommandPalette'
import { WorkspaceShell } from './components/workspace/WorkspaceShell'
import { PANES } from './components/workspace/paneRegistry'
import TaskIssue from './components/task/TaskIssue'
import ItemsPage from './components/items/ItemsPage'
import TaskManager from './components/taskmgr/TaskManager'
import MeasureMonitor from './components/measure/MeasureMonitor'
import ScenarioPage from './components/scenario/ScenarioPage'
import PalletPage from './components/pallet/PalletPage'
import TracePage from './components/trace/TracePage'
import { ContextMenuHost } from './lib/ui/ContextMenuHost'
import { Toaster } from './lib/ui/Toaster'
import { ErrorBoundary } from './lib/ui/ErrorBoundary'

function groupCls(active: boolean, open: boolean): string {
  return (
    'inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded px-2 py-1 text-sm-tight transition-colors ' +
    (open
      ? 'bg-surface-active text-content-primary'
      : active
        ? 'font-medium text-accent-text'
        : 'text-content-muted hover:bg-surface-inset hover:text-content-primary')
  )
}

/** 탭 id → 화면. 탭이 늘 때 손댈 자리가 한 곳이다. */
function Screen({ tab }: { tab: Tab }) {
  switch (tab) {
    case 'items':
      return <ItemsPage />
    case 'taskmgr':
      return <TaskManager />
    case 'measure':
      return <MeasureMonitor />
    case 'scenario':
      return <ScenarioPage />
    case 'pallet':
      return <PalletPage />
    case 'trace':
      return <TracePage />
    case 'task':
    default:
      return <TaskIssue />
  }
}

const menuRowCls =
  'flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-sm-tight text-content-tertiary hover:bg-surface-inset disabled:opacity-40 disabled:hover:bg-transparent'

/** 메뉴 구분 머리줄 — 항목이 스물 가까이 되면 묶음 이름 없이는 훑을 수 없다. */
function MenuLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-2.5 pt-1.5 pb-0.5 text-3xs font-semibold tracking-wide text-content-faint">
      {children}
    </div>
  )
}

/** 체크 자리를 늘 비워 두는 메뉴 항목 — 켜짐/꺼짐 항목이 섞여도 글자 시작이 안 흔들린다. */
function MenuRow({
  label,
  hint,
  checked = false,
  disabled,
  testid,
  onPick,
}: {
  label: string
  hint?: string
  checked?: boolean
  disabled?: string
  testid?: string
  onPick: () => void
}) {
  return (
    <button
      type="button"
      role="menuitem"
      className={menuRowCls}
      disabled={!!disabled}
      title={disabled}
      data-testid={testid}
      aria-checked={checked}
      onClick={onPick}
    >
      <span className="w-3 shrink-0 text-accent-text">
        {checked ? <Check className="h-3 w-3" /> : null}
      </span>
      {/* **라벨이 먼저다.** 예전에는 라벨이 `flex-1`이라 긴 힌트에 밀려 `명령 중 / 심`처럼 글자마다
          끊겼다 — 가이드 4절 ②가 금지하는 그것을 메뉴가 하고 있었다. 라벨은 줄어들지 않고, 남는
          폭을 힌트가 가져가며 모자라면 **힌트가 먼저** 잘린다. */}
      <span className="shrink-0 whitespace-nowrap">{label}</span>
      {hint ? (
        <kbd className="min-w-0 flex-1 truncate text-right font-mono text-3xs text-content-faint">
          {hint}
        </kbd>
      ) : (
        <span className="flex-1" />
      )}
    </button>
  )
}

/**
 * 보기 메뉴 — 셸의 모양을 정하는 것들만 든다(Unity의 `Window > Layouts` + VSCode의 `View`).
 *
 * 여기 있는 모든 항목은 명령 팔레트에도 같은 이름으로 있다. 메뉴는 **자주 쓰는 길**이고 팔레트는
 * 전부를 담는 길이라, 둘이 갈리면 사용자가 배운 이름이 한쪽에서 통하지 않는다.
 */
function ViewMenu({ onDone }: { onDone: () => void }) {
  const ws = workspace.enabled
  const l = workspace.layout
  const pick = (run: () => void) => () => {
    run()
    onDone()
  }

  return (
    <div
      className="absolute top-full left-0 z-50 mt-0.5 max-h-[70vh] min-w-80 overflow-y-auto rounded-md border border-line-default bg-surface-panel py-1 shadow-lg"
      role="menu"
      tabIndex={-1}
      data-testid="view-menu"
    >
      <MenuRow
        label="워크스페이스(도킹) 모드"
        checked={ws}
        testid="view-ws-mode"
        onPick={pick(() => workspace.toggleEnabled())}
      />
      <MenuRow
        label="명령 팔레트…"
        hint={chord('K')}
        testid="view-palette"
        onPick={pick(() => palette.show())}
      />

      <MenuLabel>배치</MenuLabel>
      {PRESETS.map((pr) => (
        <MenuRow
          key={pr.id}
          label={pr.label}
          hint={pr.hint}
          checked={ws && workspace.presetId === pr.id}
          testid={`view-preset-${pr.id}`}
          onPick={pick(() => {
            workspace.setEnabled(true)
            workspace.applyPreset(pr.id)
          })}
        />
      ))}
      <MenuRow
        label="현재 배치 저장…"
        disabled={ws ? undefined : '워크스페이스 모드에서만'}
        testid="view-save-layout"
        onPick={pick(() => palette.ask('layout.save'))}
      />
      {workspace.saved.map((sv) => (
        <MenuRow
          key={sv.name}
          label={sv.name}
          testid={`view-saved-${sv.name}`}
          onPick={pick(() => {
            workspace.setEnabled(true)
            workspace.load(sv.name)
          })}
        />
      ))}
      <MenuRow
        label="배치 초기화"
        testid="view-reset-layout"
        onPick={pick(() => {
          workspace.applyPreset(DEFAULT_PRESET)
          toast.info('기본 배치로 되돌렸습니다')
        })}
      />

      <MenuLabel>존</MenuLabel>
      {ZONE_IDS.filter((z) => z !== 'center').map((z) => {
        const zs = l.zones[z]
        return (
          <MenuRow
            key={z}
            label={`${ZONE_LABEL[z]} 존`}
            checked={!zs.collapsed}
            disabled={
              !ws ? '워크스페이스 모드에서만' : zs.panes.length === 0 ? '패널이 없습니다' : undefined
            }
            testid={`view-zone-${z}`}
            onPick={pick(() => workspace.toggleZone(z))}
          />
        )
      })}

      <MenuLabel>표시</MenuLabel>
      <MenuRow
        label="조밀하게"
        checked={density.isCompact}
        testid="view-density"
        onPick={pick(() => density.toggle())}
      />
      <MenuRow
        label="다크 테마"
        checked={theme.isDark}
        testid="view-theme"
        onPick={pick(() => theme.toggle())}
      />
    </div>
  )
}

/** 메뉴바에 열리는 서랍 — 탭 그룹들 + 보기(`view`). 보기는 탭이 아니라 **셸 조작**이라 따로 둔다. */
type MenuId = TabGroupId | 'view'

export function App() {
  useStore(nav, theme, panels, workspace, density)

  const [sidebarOpen, setSidebarOpen] = useState(false) // 좁은 화면 사이드바 토글.
  // 콘솔 앱 Profile(`/api/console/info`) — 백엔드가 선언한 탭만 노출. 조회 실패면 `null`이라 전 탭 폴백.
  const [info, setInfo] = useState<ConsoleInfo | null>(null)
  // 맥 메뉴바 관용구: 하나가 열려 있으면 다른 그룹은 **hover만으로** 전환된다.
  const [openGroup, setOpenGroup] = useState<MenuId | null>(null)
  /** 초기 URL 반영 전에는 되쓰기 금지(기본값이 URL을 덮어쓰지 않게). */
  const urlApplied = useRef(false)

  // `local` 탭은 Profile 에 없어도 남는다(`lib/tabs.ts` 의 주석 — 백엔드 라우터는 있는데 목록에만
  // 안 실린 화면이 통째로 닿지 않는 것을 막는다).
  const tabs = info ? ALL_TABS.filter((t) => t.local || info.tabs.includes(t.id)) : ALL_TABS

  // 메뉴바 — 보이는 탭이 하나도 없는 그룹은 버튼 자체를 내린다.
  const groups = TAB_GROUPS.map((g) => ({
    ...g,
    items: tabs.filter((t) => t.group === g.id),
  })).filter((g) => g.items.length > 0)
  const activeTab = tabs.find((t) => t.id === nav.tab) ?? null

  /** 단축키 힌트(1~9) — 보이는 탭의 평탄화 순서. */
  const hotkey = (t: TabDef): string => {
    const i = tabs.indexOf(t)
    return i >= 0 && i < 9 ? String(i + 1) : ''
  }

  // ── URL 딥링크(`?tab=`) ─────────────────────────────────────────────────────
  //
  // 새로고침·뒤로가기·링크 공유로 **작업 맥락이 유지**된다. 진실원은 `nav.tab`이고 URL은 그 투영이다.
  const readUrl = useCallback((): void => {
    const q = new URLSearchParams(location.search)
    const t = q.get('tab')
    if (t && ALL_TABS.some((x) => x.id === t)) nav.tab = t as Tab
  }, [])

  const syncUrl = useCallback((): void => {
    if (!urlApplied.current) return
    const q = new URLSearchParams(location.search)
    q.set('tab', nav.tab)
    const next = `${location.pathname}?${q.toString()}`
    if (next !== location.pathname + location.search) history.replaceState(null, '', next)
  }, [])

  // ── 키보드 단축키 ──────────────────────────────────────────────────────────
  //
  // **가드가 둘 필요하다.**
  // ① 포커스가 조작 요소(버튼·링크·입력)에 있으면 손대지 않는다 — 입력 요소만 거르면 버튼에
  //    포커스가 있을 때 키가 여기로 와 `preventDefault`되고, **그 버튼을 키보드로 누를 수 없게 된다**.
  // ② 모달(패널 스택·확인 다이얼로그)이 떠 있으면 전역 단축키를 전부 멈춘다. 삭제 확인이 떠 있는데
  //    `1~4`가 뒤 화면의 탭을 바꾸면 앞에 있는 질문이 무의미해진다.
  const onKey = useCallback(
    (e: KeyboardEvent): void => {
      const t = e.target as HTMLElement | null
      if (e.key === 'Escape' && openGroup !== null) {
        setOpenGroup(null)
        return
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (panels.stack.length > 0 || document.querySelector('[role="dialog"]') !== null) return
      if (
        t?.closest(
          'input, textarea, select, button, a[href], [role="button"], [contenteditable=""], [contenteditable="true"]',
        )
      )
        return
      // 숫자 1..9 = 탭 이동(보이는 탭 순서).
      if (/^[1-9]$/.test(e.key)) {
        const t2 = tabs[Number(e.key) - 1]
        if (t2) {
          nav.go(t2.id)
          setOpenGroup(null)
          e.preventDefault()
        }
      }
    },
    [openGroup, tabs],
  )

  // 셸 부팅 — 밀도 반영 · Profile 조회 · 딥링크 1회 반영 · popstate.
  useEffect(() => {
    density.start()
    void api
      .consoleInfo()
      .then((i) => {
        setInfo(i)
        // 백엔드가 안 내는 화면의 **탭이 레이아웃에 남지 않게** 한 번 걷어 낸다(보조 패널은 늘 있다).
        workspace.setAvailable(
          PANES.filter(
            (p) =>
              p.kind === 'aux' ||
              i.tabs.includes(p.id) ||
              ALL_TABS.some((t) => t.id === p.id && t.local),
          ).map((p) => p.id),
          i.default_tab,
        )
        // 백엔드 기본 탭은 **URL이 없을 때만** 적용한다(공유 링크가 이겨야 한다).
        if (
          !new URLSearchParams(location.search).get('tab') &&
          ALL_TABS.some((t) => t.id === i.default_tab)
        )
          nav.tab = i.default_tab as Tab
      })
      .catch(() => {})
    readUrl()
    urlApplied.current = true
    addEventListener('popstate', readUrl)
    return () => removeEventListener('popstate', readUrl)
  }, [readUrl])

  // 단축키는 `tabs`·`openGroup`에 매여 있어 부팅과 수명이 다르다 — 따로 건다.
  useEffect(() => {
    addEventListener('keydown', onKey)
    return () => removeEventListener('keydown', onKey)
  }, [onKey])

  // 메뉴 바깥 클릭이면 닫는다(메뉴 안 클릭은 자체 핸들러가 처리).
  useEffect(() => {
    if (openGroup === null) return
    const onDocClick = (e: MouseEvent): void => {
      const t = e.target as HTMLElement | null
      if (!t?.closest('[data-menubar]')) setOpenGroup(null)
    }
    addEventListener('click', onDocClick)
    return () => removeEventListener('click', onDocClick)
  }, [openGroup])

  // 탭이 바뀔 때마다 URL에 투영(사용자 조작이 곧 공유 가능한 주소가 된다).
  useEffect(() => {
    syncUrl()
  }, [syncUrl, nav.tab])

  // **이 백엔드가 안 내는 탭으로는 머무르지 않는다.** `?tab=`은 전 탭 목록으로 검증하는데 메뉴는
  // 백엔드가 선언한 것만 그린다 — 그 틈으로 들어오면 메뉴 어디에도 없는 화면에 갇힌다.
  useEffect(() => {
    if (!info || tabs.length === 0) return
    const cur = nav.tab
    if (tabs.some((t) => t.id === cur)) return
    const fallback = (tabs.find((t) => t.id === info.default_tab) ?? tabs[0]).id
    nav.go(fallback)
    toast.warn(`이 백엔드에는 '${cur}' 화면이 없습니다 — 기본 화면으로 돌아갑니다`)
  }, [info, tabs, nav.tab])

  return (
    <div className="flex h-screen flex-col bg-surface-app text-content-primary">
      {/* 메뉴바 — 맥 스타일의 얇은 한 줄. 네비게이션만 남기고 상태·단축키는 하단 StatusBar가 맡는다. */}
      <header className="flex h-menubar shrink-0 items-center gap-1 border-b border-line-default px-2">
        <button
          type="button"
          className="rounded p-1 text-content-muted hover:bg-surface-inset md:hidden"
          aria-label="사이드바 토글"
          onClick={() => setSidebarOpen(!sidebarOpen)}
        >
          <Menu className="h-4 w-4" />
        </button>
        <h1 className="flex items-center gap-1.5 pr-1 text-sm-tight font-semibold">
          <span className="grid h-5 w-5 place-items-center rounded bg-accent text-3xs text-content-on-accent">
            GR
          </span>
          <span className="hidden sm:inline">
            GR 콘솔
            {info?.app_name ? (
              <span className="font-normal text-content-faint"> · {info.app_name}</span>
            ) : null}
          </span>
        </h1>

        <nav className="flex items-center gap-0.5" data-menubar>
          {groups.map((g) => {
            const active = g.items.some((t) => t.id === nav.tab)
            return (
              <div key={g.id} className="relative">
                {/* 그룹 버튼은 **그룹 이름만** 낸다 — 활성 탭 라벨을 붙이면 폭이 흔들린다. 지금 어느 화면인지는
                    하단 상태바가 말한다. */}
                <button
                  type="button"
                  className={groupCls(active, openGroup === g.id)}
                  data-testid={`tabgroup-${g.id}`}
                  data-tabs={g.items.map((t) => t.id).join(',')}
                  aria-haspopup="menu"
                  aria-expanded={openGroup === g.id}
                  onClick={() => setOpenGroup(openGroup === g.id ? null : g.id)}
                  onMouseEnter={() => {
                    if (openGroup !== null) setOpenGroup(g.id)
                  }}
                >
                  {g.label}
                  <ChevronDown className="h-3 w-3 opacity-50" />
                </button>
                {openGroup === g.id ? (
                  <div
                    className="absolute top-full left-0 z-50 mt-0.5 min-w-48 overflow-hidden rounded-md border border-line-default bg-surface-panel py-1 shadow-lg"
                    role="menu"
                    tabIndex={-1}
                  >
                    {g.items.map((t) => {
                      const Icon = t.icon
                      return (
                        <button
                          key={t.id}
                          type="button"
                          className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-sm-tight ${
                            nav.tab === t.id
                              ? 'bg-accent-soft font-medium text-accent-text'
                              : 'text-content-tertiary hover:bg-surface-inset'
                          }`}
                          role="menuitem"
                          data-testid={`tab-${t.id}`}
                          aria-current={nav.tab === t.id ? 'page' : undefined}
                          onClick={() => {
                            nav.go(t.id)
                            setOpenGroup(null)
                          }}
                        >
                          <Icon className="h-4 w-4 shrink-0 opacity-70" />
                          <span className="flex-1">{t.label}</span>
                          {hotkey(t) ? (
                            <kbd className="font-mono text-3xs text-content-faint">{hotkey(t)}</kbd>
                          ) : null}
                        </button>
                      )
                    })}
                  </div>
                ) : null}
              </div>
            )
          })}

          {/* 보기 — 탭이 아니라 셸의 모양(도킹·배치·존·밀도). 탭 그룹과 같은 관용구로 열린다. */}
          <div className="relative">
            <button
              type="button"
              className={groupCls(false, openGroup === 'view')}
              data-testid="menu-view"
              aria-haspopup="menu"
              aria-expanded={openGroup === 'view'}
              onClick={() => setOpenGroup(openGroup === 'view' ? null : 'view')}
              onMouseEnter={() => {
                if (openGroup !== null) setOpenGroup('view')
              }}
            >
              <Columns3 className="h-3.5 w-3.5 opacity-60" />
              보기
              <ChevronDown className="h-3 w-3 opacity-50" />
            </button>
            {openGroup === 'view' ? <ViewMenu onDone={() => setOpenGroup(null)} /> : null}
          </div>
        </nav>

        <button
          type="button"
          className="ml-auto flex items-center gap-1.5 rounded border border-line-default px-2 py-0.5 text-2xs text-content-faint hover:bg-surface-inset hover:text-content-secondary"
          aria-label="명령 팔레트 열기"
          title="명령 팔레트 — 패널 · 배치 · 존 · 설정을 이름으로"
          data-testid="open-palette"
          onClick={() => palette.show()}
        >
          <Search className="h-3 w-3" />
          <span className="hidden sm:inline">명령</span>
          <kbd className="font-mono text-3xs">{chord('K')}</kbd>
        </button>

        <button
          type="button"
          className="rounded p-1 text-content-muted hover:bg-surface-inset"
          aria-label="테마 전환"
          title="라이트/다크 전환"
          onClick={() => theme.toggle()}
        >
          {theme.isDark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </button>
      </header>

      {/* 본문 — 워크스페이스 모드면 도킹 격자 하나, 아니면 사이드바 + 화면. */}
      {workspace.enabled ? (
        <WorkspaceShell />
      ) : (
      <div className="flex min-h-0 flex-1">
        {sidebarOpen ? (
          // 모바일 사이드바 backdrop — 바깥 클릭으로 닫기(모달 어포던스).
          <button
            type="button"
            className="fixed inset-x-0 top-menubar bottom-0 z-30 bg-black/30 md:hidden"
            aria-label="사이드바 닫기"
            onClick={() => setSidebarOpen(false)}
          />
        ) : null}
        <aside
          className={`shrink-0 border-r border-line-default ${
            sidebarOpen ? 'absolute inset-y-0 top-menubar z-40 bg-surface-app' : 'hidden'
          } md:relative md:top-0 md:block`}
        >
          <Sidebar />
        </aside>
        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          <ErrorBoundary label="화면" resetKey={nav.tab}>
            <Screen tab={nav.tab} />
          </ErrorBoundary>
        </main>
      </div>
      )}

      <StatusBar tab={activeTab?.label ?? nav.tab} />

      <ContextMenuHost />
      <Toaster />
      <PanelHost />
      <CommandPalette />
    </div>
  )
}
