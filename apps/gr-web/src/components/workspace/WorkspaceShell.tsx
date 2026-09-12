// 워크스페이스 셸 — 존 넷을 격자로 앉히고 스플리터를 끼운다. 도킹 모드의 본문 전체가 이것 하나다.
//
// 격자는 `left | (center / bottom) | right`다. 하단이 중앙 **안쪽**에 드는 이유는 VSCode와 같다:
// 하단 패널(문제·터미널)은 작업면의 부속이고 사이드 목록은 그 위를 가로지르지 않는다. 하단을 바닥
// 전체로 깔면 왼쪽 목록의 높이가 하단을 열 때마다 바뀌어 눌러야 할 행이 움직인다.
//
// 최대화는 존 하나만 그리는 것으로 구현한다 — 다른 존을 `display:none`으로 숨기면 그 안의 화면이
// 계속 살아서 SSE·폴이 돌고, "최대화했는데 왜 느려지나"가 된다.
import { useEffect, useRef } from 'react'
import { nav, type Tab } from '../../lib/nav'
import { useStore } from '../../lib/store'
import { ZONE_LABEL, ZONE_LIMITS } from '../../lib/workspace/model'
import { workspace } from '../../lib/workspace/store'
import { DockZone, ZoneRail } from './DockZone'
import { isScreenPane, paneDef } from './paneRegistry'
import { Splitter } from './Splitter'

export function WorkspaceShell() {
  useStore(workspace, nav)
  const l = workspace.layout

  // ── `nav`와의 맞물림 ───────────────────────────────────────────────────────
  //
  // 두 방향이 다 필요하다. ① 다른 화면이 `nav.goTask(id)`로 부르면 그 패널이 **드러나야** 한다
  // (탭이 뒤에 숨어 있으면 눌러도 아무 일도 안 일어난 것처럼 보인다). ② 사용자가 중앙 탭을 바꾸면
  // `nav.tab`이 따라가야 URL(`?tab=`)·상태바·딥링크가 맞는다.
  //
  // 두 방향을 한 효과에 담으면 서로를 다시 부른다 — 그래서 **마지막으로 본 값**을 들고 누가 먼저
  // 움직였는지로 가른다.
  const lastTab = useRef<string>(nav.tab)
  const lastCenter = useRef<string | null>(l.zones.center.active)

  useEffect(() => {
    const center = workspace.layout.zones.center.active
    if (nav.tab !== lastTab.current) {
      lastTab.current = nav.tab
      lastCenter.current = nav.tab
      workspace.reveal(nav.tab)
      return
    }
    if (center !== lastCenter.current) {
      lastCenter.current = center
      if (center && isScreenPane(center) && center !== nav.tab) {
        lastTab.current = center
        nav.go(center as Tab)
      }
    }
  })

  // 최대화 — 그 패널이 든 존만 그린다(탭 띠는 남는다: 풀 길이 화면 안에 있어야 한다).
  if (l.maximized) {
    const zone = workspace.zoneOf(l.maximized)
    if (zone)
      return (
        <div className="flex min-h-0 flex-1" data-testid="ws-maximized">
          <DockZone zone={zone} />
        </div>
      )
  }

  const left = l.zones.left
  const right = l.zones.right
  const bottom = l.zones.bottom

  return (
    <div className="flex min-h-0 flex-1" data-testid="workspace">
      {/* 왼쪽 */}
      {left.collapsed ? (
        left.panes.length > 0 ? (
          <ZoneRail zone="left" />
        ) : null
      ) : (
        <>
          <div className="flex min-h-0 shrink-0 flex-col" style={{ width: `${left.size}px` }}>
            <DockZone zone="left" />
          </div>
          <Splitter
            axis="col"
            size={left.size}
            min={ZONE_LIMITS.left.min}
            max={ZONE_LIMITS.left.max}
            label={`${ZONE_LABEL.left} 폭`}
            onResize={(v) => workspace.resize('left', v)}
          />
        </>
      )}

      {/* 중앙 + 하단 */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <DockZone zone="center" />
        {bottom.collapsed ? (
          bottom.panes.length > 0 ? (
            <ZoneRail zone="bottom" />
          ) : null
        ) : (
          <>
            <Splitter
              axis="row"
              size={bottom.size}
              min={ZONE_LIMITS.bottom.min}
              max={ZONE_LIMITS.bottom.max}
              invert
              label={`${ZONE_LABEL.bottom} 높이`}
              onResize={(v) => workspace.resize('bottom', v)}
            />
            <div
              className="flex min-h-0 shrink-0 flex-col border-t border-slate-200 dark:border-slate-700"
              style={{ height: `${bottom.size}px` }}
            >
              <DockZone zone="bottom" />
            </div>
          </>
        )}
      </div>

      {/* 오른쪽 */}
      {right.collapsed ? (
        right.panes.length > 0 ? (
          <ZoneRail zone="right" />
        ) : null
      ) : (
        <>
          <Splitter
            axis="col"
            size={right.size}
            min={ZONE_LIMITS.right.min}
            max={ZONE_LIMITS.right.max}
            invert
            label={`${ZONE_LABEL.right} 폭`}
            onResize={(v) => workspace.resize('right', v)}
          />
          <div
            className="flex min-h-0 shrink-0 flex-col border-l border-slate-200 dark:border-slate-700"
            style={{ width: `${right.size}px` }}
          >
            <DockZone zone="right" />
          </div>
        </>
      )}
    </div>
  )
}

/** 지금 보이는 패널 이름들 — 상태바가 "무엇을 보고 있나"를 한 줄로 말한다. */
export function visibleLabels(): string[] {
  return workspace.rendered.flatMap((id) => {
    const d = paneDef(id)
    return d ? [d.label] : []
  })
}
