// 셸 — 메뉴바(그룹 드롭다운) · 사이드바 · 화면 · 상태바. **리드 소유** — 화면(`components/<page>/`)은
// 각 담당 에이전트가 갈고, 여기서는 탭 스위치만 그것을 가리킨다.
//
// sh4w-web의 App에서 떼어 낸 창(`?panel=`)·플랫폼/로봇 선택·배너·명령 팔레트·토큰 프롬프트를
// 걷어 냈다 — GR 콘솔은 PLC 한 세트를 보는 단일 문맥이라 선택 축이 없다.
import { useCallback, useEffect, useRef, useState } from 'react'
import { ChevronDown, Menu, Moon, Sun } from 'lucide-react'

import { nav, type Tab } from './lib/nav'
import { ALL_TABS, TAB_GROUPS, type TabDef, type TabGroupId } from './lib/tabs'
import { theme } from './lib/theme'
import { api } from './lib/api'
import { panels } from './lib/panels'
import { toast } from './lib/ui/toast'
import { useStore } from './lib/store'
import type { ConsoleInfo } from './lib/types'

import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { PanelHost } from './components/PanelHost'
import TaskIssue from './components/task/TaskIssue'
import TaskManager from './components/taskmgr/TaskManager'
import MeasureMonitor from './components/measure/MeasureMonitor'
import ScenarioPage from './components/scenario/ScenarioPage'
import { ContextMenuHost } from './lib/ui/ContextMenuHost'
import { Toaster } from './lib/ui/Toaster'
import { ErrorBoundary } from './lib/ui/ErrorBoundary'

function groupCls(active: boolean, open: boolean): string {
  return (
    'inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded px-2 py-1 text-[13px] transition-colors ' +
    (open
      ? 'bg-slate-200 text-slate-900 dark:bg-slate-700 dark:text-slate-100'
      : active
        ? 'font-medium text-indigo-700 dark:text-indigo-300'
        : 'text-slate-500 hover:bg-slate-100 hover:text-slate-900 dark:hover:bg-slate-800 dark:hover:text-slate-100')
  )
}

/** 탭 id → 화면. 탭이 늘 때 손댈 자리가 한 곳이다. */
function Screen({ tab }: { tab: Tab }) {
  switch (tab) {
    case 'taskmgr':
      return <TaskManager />
    case 'measure':
      return <MeasureMonitor />
    case 'scenario':
      return <ScenarioPage />
    case 'task':
    default:
      return <TaskIssue />
  }
}

export function App() {
  useStore(nav, theme, panels)

  const [sidebarOpen, setSidebarOpen] = useState(false) // 좁은 화면 사이드바 토글.
  // 콘솔 앱 Profile(`/api/console/info`) — 백엔드가 선언한 탭만 노출. 조회 실패면 `null`이라 전 탭 폴백.
  const [info, setInfo] = useState<ConsoleInfo | null>(null)
  // 맥 메뉴바 관용구: 하나가 열려 있으면 다른 그룹은 **hover만으로** 전환된다.
  const [openGroup, setOpenGroup] = useState<TabGroupId | null>(null)
  /** 초기 URL 반영 전에는 되쓰기 금지(기본값이 URL을 덮어쓰지 않게). */
  const urlApplied = useRef(false)

  const tabs = info ? ALL_TABS.filter((t) => info.tabs.includes(t.id)) : ALL_TABS

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

  // 셸 부팅 — Profile 조회 · 딥링크 1회 반영 · popstate.
  useEffect(() => {
    void api
      .consoleInfo()
      .then((i) => {
        setInfo(i)
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
    <div className="flex h-screen flex-col bg-slate-50 text-slate-900 dark:bg-slate-900 dark:text-slate-100">
      {/* 메뉴바 — 맥 스타일의 얇은 한 줄. 네비게이션만 남기고 상태·단축키는 하단 StatusBar가 맡는다. */}
      <header className="flex h-9 shrink-0 items-center gap-1 border-b border-slate-200 px-2 dark:border-slate-700">
        <button
          type="button"
          className="rounded p-1 text-slate-500 hover:bg-slate-100 md:hidden dark:hover:bg-slate-800"
          aria-label="사이드바 토글"
          onClick={() => setSidebarOpen(!sidebarOpen)}
        >
          <Menu className="h-4 w-4" />
        </button>
        <h1 className="flex items-center gap-1.5 pr-1 text-[13px] font-semibold">
          <span className="grid h-5 w-5 place-items-center rounded bg-indigo-600 text-[10px] text-white">
            GR
          </span>
          <span className="hidden sm:inline">
            GR 콘솔
            {info?.app_name ? (
              <span className="font-normal text-slate-400"> · {info.app_name}</span>
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
                    className="absolute top-full left-0 z-50 mt-0.5 min-w-48 overflow-hidden rounded-md border border-slate-200 bg-white py-1 shadow-lg dark:border-slate-700 dark:bg-slate-800"
                    role="menu"
                    tabIndex={-1}
                  >
                    {g.items.map((t) => {
                      const Icon = t.icon
                      return (
                        <button
                          key={t.id}
                          type="button"
                          className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[13px] ${
                            nav.tab === t.id
                              ? 'bg-indigo-50 font-medium text-indigo-700 dark:bg-indigo-500/15 dark:text-indigo-300'
                              : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700'
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
                            <kbd className="font-mono text-[10px] text-slate-400">{hotkey(t)}</kbd>
                          ) : null}
                        </button>
                      )
                    })}
                  </div>
                ) : null}
              </div>
            )
          })}
        </nav>

        <button
          type="button"
          className="ml-auto rounded p-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
          aria-label="테마 전환"
          title="라이트/다크 전환"
          onClick={() => theme.toggle()}
        >
          {theme.isDark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </button>
      </header>

      {/* 본문 */}
      <div className="flex min-h-0 flex-1">
        {sidebarOpen ? (
          // 모바일 사이드바 backdrop — 바깥 클릭으로 닫기(모달 어포던스).
          <button
            type="button"
            className="fixed inset-x-0 top-9 bottom-0 z-30 bg-black/30 md:hidden"
            aria-label="사이드바 닫기"
            onClick={() => setSidebarOpen(false)}
          />
        ) : null}
        <aside
          className={`shrink-0 border-r border-slate-200 dark:border-slate-700 ${
            sidebarOpen ? 'absolute inset-y-0 top-9 z-40 bg-slate-50 dark:bg-slate-900' : 'hidden'
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

      <StatusBar tab={activeTab?.label ?? nav.tab} />

      <ContextMenuHost />
      <Toaster />
      <PanelHost />
    </div>
  )
}
