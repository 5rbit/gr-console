// 명령 팔레트 — `Ctrl/⌘+K`(또는 `Ctrl/⌘+Shift+P`). 셸의 모든 조작을 이름으로 부른다.
//
// VSCode에서 가져온 것은 창이 아니라 **규칙**이다: 메뉴에 있는 것은 팔레트에도 있다. 그래서 메뉴는
// 자주 쓰는 것만 들고 얇게 남고(지금 셸의 메뉴바는 한 줄이다), 패널 일곱 × 존 넷 × 프리셋 넷 같은
// 조합은 목록이 아니라 검색으로 닿는다.
//
// 명령 목록을 모듈 상수로 두지 않는다 — 프리셋 체크·최대화 상태·저장한 배치가 매번 달라서, 열릴
// 때마다 지금 스토어를 보고 만든다.
import { useEffect, useRef, useState } from 'react'
import type * as React from 'react'
import { Check, Search } from 'lucide-react'
import { groupHits, matchCommands, type Command } from '../lib/commands'
import { density } from '../lib/density'
import { chord, hasMod } from '../lib/keys'
import { nav, type Tab } from '../lib/nav'
import { palette } from '../lib/palette'
import { panels } from '../lib/panels'
import { useStore } from '../lib/store'
import { theme } from '../lib/theme'
import { toast } from '../lib/ui/toast'
import { useFocusTrap } from '../lib/ui/focusTrap'
import { ZONE_IDS, ZONE_LABEL } from '../lib/workspace/model'
import { PRESETS, DEFAULT_PRESET } from '../lib/workspace/presets'
import { workspace } from '../lib/workspace/store'
import { PANES, paneDef } from './workspace/paneRegistry'

/**
 * 지금 상태로 명령 목록을 만든다 — 그룹 순서가 팔레트를 그냥 열었을 때의 목차다.
 * 보기(패널) → 레이아웃 → 존 → 창 → 설정 순으로, 자주 쓰는 것이 위에 온다.
 */
function buildCommands(): Command[] {
  const l = workspace.layout
  const ws = workspace.enabled
  const out: Command[] = []

  // ── 보기: 패널 열기 ──
  for (const p of PANES) {
    const zone = workspace.zoneOf(p.id)
    out.push({
      id: `open.${p.id}`,
      group: '보기',
      label: `${p.label} 열기`,
      keywords: p.keywords,
      checked: ws ? workspace.rendered.includes(p.id) : nav.tab === p.id,
      hint: zone && ws ? ZONE_LABEL[zone] : undefined,
      run: () => {
        if (!ws) {
          // 단일 화면 모드에서는 화면만 바꿀 수 있다 — 보조 패널은 도킹 모드가 필요하다.
          if (p.kind === 'screen') nav.go(p.id as Tab)
          else {
            workspace.setEnabled(true)
            workspace.reveal(p.id, p.defaultZone)
            toast.info(`'${p.label}'을 열기 위해 워크스페이스 모드로 전환했습니다`)
          }
          return
        }
        workspace.reveal(p.id, p.defaultZone)
      },
    })
  }

  // ── 레이아웃: 프리셋 · 저장 ──
  for (const p of PRESETS)
    out.push({
      id: `preset.${p.id}`,
      group: '레이아웃',
      label: `${p.label} 배치`,
      keywords: `preset layout ${p.hint}`,
      checked: workspace.presetId === p.id,
      run: () => {
        workspace.setEnabled(true)
        workspace.applyPreset(p.id)
      },
    })
  out.push({
    id: 'layout.save',
    group: '레이아웃',
    label: '현재 배치 저장…',
    keywords: 'save 저장 이름',
    prompt: {
      label: '배치 이름',
      placeholder: '예: 2호기 정렬 작업',
      run: (name) => {
        workspace.saveAs(name)
        toast.ok(`배치 '${name.trim()}'을 저장했습니다`)
      },
    },
  })
  for (const s of workspace.saved) {
    out.push({
      id: `layout.load.${s.name}`,
      group: '레이아웃',
      label: `저장한 배치: ${s.name}`,
      keywords: 'load 불러오기',
      run: () => workspace.load(s.name),
    })
    out.push({
      id: `layout.remove.${s.name}`,
      group: '레이아웃',
      label: `저장한 배치 삭제: ${s.name}`,
      keywords: 'delete 삭제',
      run: () => {
        workspace.remove(s.name)
        toast.info(`배치 '${s.name}'을 지웠습니다`)
      },
    })
  }
  out.push({
    id: 'layout.reset',
    group: '레이아웃',
    label: '배치 초기화',
    keywords: 'reset 기본 되돌리기',
    run: () => {
      workspace.applyPreset(DEFAULT_PRESET)
      toast.info('기본 배치로 되돌렸습니다')
    },
  })

  // ── 존 접기/펼치기 ──
  for (const z of ZONE_IDS) {
    if (z === 'center') continue
    const s = l.zones[z]
    out.push({
      id: `zone.${z}`,
      // 라벨을 뒤집지 않고 **체크로 상태를 말한다** — 메뉴의 `보기 > 존`과 같은 관용구다.
      // 같은 항목이 자리마다 다른 이름("펼치기"/"접기")으로 나오면 두 개를 배우게 된다.
      group: '존',
      label: `${ZONE_LABEL[z]} 존`,
      keywords: 'zone dock 사이드바 패널 접기 펼치기',
      checked: !s.collapsed,
      disabled: s.panes.length === 0 ? '이 존에는 패널이 없습니다' : undefined,
      run: () => workspace.toggleZone(z),
    })
  }

  // ── 창(최대화·모드) ──
  //
  // 대상은 **활성 패널**이다(중앙의 활성 탭이 아니라) — 오른쪽 상태 패널을 보다가 최대화를 눌렀는데
  // 중앙이 커지면, 누른 것과 반응한 것이 다르다.
  const target = workspace.focused
  const targetLabel = target ? paneDef(target)?.label : null
  out.push({
    id: 'window.maximize',
    group: '창',
    label: l.maximized
      ? '최대화 해제'
      : targetLabel
        ? `'${targetLabel}' 최대화`
        : '활성 패널 최대화',
    keywords: 'maximize zen 전체',
    hint: 'Alt+Enter',
    disabled: ws ? undefined : '워크스페이스 모드에서만',
    run: () => workspace.maximize(l.maximized ? null : target),
  })
  out.push({
    id: 'window.mode',
    group: '창',
    label: ws ? '단일 화면 모드로' : '워크스페이스(도킹) 모드로',
    keywords: 'mode dock workspace 단일 화면',
    checked: ws,
    run: () => workspace.toggleEnabled(),
  })

  // ── 설정 ──
  out.push({
    id: 'view.density',
    group: '설정',
    label: density.isCompact ? '표준 밀도로' : '조밀 밀도로',
    keywords: 'density compact 좁게 빽빽',
    checked: density.isCompact,
    run: () => density.toggle(),
  })
  out.push({
    id: 'view.theme',
    group: '설정',
    label: theme.isDark ? '라이트 테마로' : '다크 테마로',
    keywords: 'theme dark light 테마',
    checked: theme.isDark,
    run: () => theme.toggle(),
  })

  return out
}

export function CommandPalette() {
  // 팔레트 항목이 지금 상태(체크·존·저장 목록)를 말하므로 이 스토어들을 전부 구독한다.
  useStore(palette, workspace, nav, theme, density)

  const open = palette.open
  const query = palette.query
  const [at, setAt] = useState(0)
  /** 두 번째 걸음에 든 입력 값. 어느 명령인지는 `palette.askId`가 안다(메뉴에서도 여니까). */
  const [text, setText] = useState('')
  const box = useFocusTrap<HTMLDivElement>(open)
  const input = useRef<HTMLInputElement>(null)

  // 매 렌더마다 다시 만든다(메모하지 않는다) — 체크 표시·존 이름이 스토어를 보고 있어서, 메모하면
  // 레이아웃이 바뀌어도 팔레트가 옛 상태를 든 채로 남는다. 명령 스무 개의 배열 하나가 비용 전부다.
  const commands = open ? buildCommands() : []
  const hits = matchCommands(commands, query)
  const groups = groupHits(hits)
  const asking: Command | null = commands.find((c) => c.id === palette.askId) ?? null

  // ── 여는 키 ──
  //
  // `⌘K`와 `⌘⇧P` 둘 다 받는다 — 앞은 요즘 웹앱의 관용구, 뒤는 VSCode를 쓰는 사람의 손가락이다.
  // 다른 전역 단축키(숫자 1~9)와 달리 **입력 안에서도** 살아야 한다(글을 쓰다 명령을 부른다).
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (
        hasMod(e) &&
        (e.key === 'k' || e.key === 'K' || (e.shiftKey && e.key.toLowerCase() === 'p'))
      ) {
        e.preventDefault()
        palette.toggle()
        setAt(0)
        return
      }
      // Alt+Enter — **활성 패널** 최대화(Unity의 창 최대화 단축키와 같은 감각).
      if (e.altKey && e.key === 'Enter' && workspace.enabled) {
        e.preventDefault()
        workspace.maximize(workspace.layout.maximized ? null : workspace.focused)
      }
    }
    addEventListener('keydown', onKey)
    return () => removeEventListener('keydown', onKey)
  }, [])

  // 열릴 때 포커스를 입력으로. 패널 스택(모달)이 떠 있으면 그 위에 겹치지 않게 물러난다 —
  // 삭제 확인 위에 팔레트가 뜨면 앞에 있는 질문이 무의미해진다.
  useEffect(() => {
    if (!open) return
    if (panels.stack.length > 0) {
      palette.hide()
      return
    }
    input.current?.focus()
  }, [open])

  // 값 입력으로 들어오면(메뉴의 `…` 항목) 초기값을 넣는다.
  useEffect(() => {
    if (palette.askId) setText('')
  }, [palette.askId])

  // 선택 커서가 목록 밖으로 나가지 않게(질의가 좁혀지면 줄 수가 준다).
  useEffect(() => {
    if (at >= hits.length) setAt(Math.max(0, hits.length - 1))
  }, [hits.length, at])

  if (!open) return null

  const flat = hits.map((h) => h.command)

  function pick(c: Command): void {
    if (c.disabled) {
      toast.warn(c.disabled)
      return
    }
    if (c.prompt) {
      palette.ask(c.id)
      setText(c.prompt.initial ?? '')
      return
    }
    palette.hide()
    c.run?.()
  }

  function onKeyDown(e: React.KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault()
      if (asking) palette.clearAsk()
      else palette.hide()
      return
    }
    if (asking) {
      if (e.key === 'Enter' && text.trim()) {
        e.preventDefault()
        const run = asking.prompt?.run
        palette.hide()
        run?.(text)
      }
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setAt((i) => Math.min(flat.length - 1, i + 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setAt((i) => Math.max(0, i - 1))
    } else if (e.key === 'Home') {
      e.preventDefault()
      setAt(0)
    } else if (e.key === 'End') {
      e.preventDefault()
      setAt(flat.length - 1)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const c = flat[at]
      if (c) pick(c)
    }
  }

  let row = -1

  return (
    // design-lint-allow: no-arbitrary-value — 팔레트는 화면 높이의 12%에서 시작한다(VSCode와 같은
    // 자리). 뷰포트 비율이라 간격 스케일에 있을 수 없는 값이다.
    <div className="fixed inset-0 z-[60] flex items-start justify-center p-4 pt-[12vh]">
      <button
        className="absolute inset-0 bg-black/30"
        aria-label="팔레트 닫기"
        tabIndex={-1}
        onClick={() => palette.hide()}
      />
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-label="명령 팔레트"
        className="relative flex max-h-[60vh] w-full max-w-xl flex-col overflow-hidden rounded-lg border border-line-default bg-surface-panel shadow-xl"
        data-testid="command-palette"
        onKeyDown={onKeyDown}
      >
        {asking ? (
          // 두 번째 걸음 — 이름 받기. 무엇을 적는지 라벨로 말하고 Enter로 끝낸다.
          <label className="flex flex-col gap-1 p-3">
            <span className="text-2xs font-medium text-content-muted">{asking.prompt?.label}</span>
            <input
              autoFocus
              value={text}
              placeholder={asking.prompt?.placeholder}
              className="h-8 rounded-md border border-line-strong bg-transparent px-2 text-sm focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none"
              data-testid="palette-prompt"
              onChange={(e) => setText(e.target.value)}
            />
            <span className="text-3xs text-content-faint">
              Enter 저장 · Escape 취소 — 저장한 배치는 이 브라우저에 남습니다
            </span>
          </label>
        ) : (
          <>
            <div className="flex items-center gap-2 border-b border-line-default px-3">
              <Search className="h-3.5 w-3.5 shrink-0 text-content-faint" />
              <input
                ref={input}
                value={query}
                placeholder="명령 검색 — 패널 · 배치 · 존 · 설정"
                aria-label="명령 검색"
                className="h-9 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-content-faint"
                data-testid="palette-input"
                onChange={(e) => {
                  palette.setQuery(e.target.value)
                  setAt(0)
                }}
              />
              <kbd className="shrink-0 font-mono text-3xs text-content-faint">{chord('K')}</kbd>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto py-1" role="listbox">
              {flat.length === 0 ? (
                <p className="px-3 py-4 text-center text-2xs text-content-faint">
                  '{query}'에 맞는 명령이 없습니다
                </p>
              ) : null}
              {groups.map((g) => (
                <div key={`${g.group}-${g.hits[0].command.id}`}>
                  <div className="px-3 pt-1.5 pb-0.5 text-3xs font-semibold tracking-wide text-content-faint">
                    {g.group}
                  </div>
                  {g.hits.map((h) => {
                    row += 1
                    const c = h.command
                    const on = row === at
                    return (
                      <button
                        key={c.id}
                        type="button"
                        role="option"
                        aria-selected={on}
                        className={`flex w-full items-center gap-2 px-3 py-1 text-left text-sm-tight ${
                          on
                            ? 'bg-accent-soft text-accent-text'
                            : 'text-content-secondary'
                        } ${c.disabled ? 'opacity-50' : ''}`}
                        data-testid={`cmd-${c.id}`}
                        title={c.disabled}
                        onMouseEnter={() => setAt(row)}
                        onClick={() => pick(c)}
                      >
                        <span className="w-3.5 shrink-0 text-accent-text">
                          {c.checked ? <Check className="h-3.5 w-3.5" /> : null}
                        </span>
                        <span className="min-w-0 flex-1 truncate">{c.label}</span>
                        {c.hint ? (
                          <kbd className="shrink-0 font-mono text-3xs text-content-faint">
                            {c.hint}
                          </kbd>
                        ) : null}
                      </button>
                    )
                  })}
                </div>
              ))}
            </div>

            <div className="flex shrink-0 items-center gap-3 border-t border-line-default px-3 py-1 text-3xs text-content-faint">
              <span>↑↓ 이동</span>
              <span>Enter 실행</span>
              <span>Escape 닫기</span>
              <span className="flex-1" />
              <span className="tabular-nums">{flat.length}개</span>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
