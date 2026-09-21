// 왼쪽 사이드바 — **단일 화면 모드**의 보조 면. 로봇·PLC·상태 세 패널을 접이식 섹션으로 얹는다.
//
// 내용은 더 여기 없다: 세 섹션의 몸은 `components/panes/*`로 떼어 냈고 워크스페이스(도킹) 모드는
// 같은 컴포넌트를 존에 얹는다. 한 벌을 두 껍데기가 쓰는 모양이라 "사이드바의 PLC 목록"과
// "오른쪽에 도킹한 PLC 목록"이 갈릴 수 없다.
//
// 폭·밀도 두 손잡이도 정리됐다. 폭은 `Splitter`(도킹 존과 같은 것 — 키보드로도 끈다)를 쓰고,
// 밀도는 전역 `density`가 맡는다(예전에는 사이드바만 자기 밀도 키를 들고 있었다).
import { useState } from 'react'
import type * as React from 'react'
import { ChevronRight, Rows2, Rows3 } from 'lucide-react'
import { density } from '../lib/density'
import { useStore } from '../lib/store'
import { ZONE_LIMITS } from '../lib/workspace/model'
import { Splitter } from './workspace/Splitter'
import PlcPane, { PlcSummary } from './panes/PlcPane'
import RobotsPane, { RobotsSummary } from './panes/RobotsPane'
import StatusPane, { StatusSummary } from './panes/StatusPane'
import { IndicatorLegend } from './panes/IndicatorLegend'

const LS_WIDTH = 'gr-sidebar-w'
const MIN_W = ZONE_LIMITS.left.min
const MAX_W = ZONE_LIMITS.left.max

function readWidth(): number {
  const v = Number(localStorage.getItem(LS_WIDTH))
  return Number.isFinite(v) && v >= MIN_W && v <= MAX_W ? v : 240
}

const headCls = 'flex shrink-0 items-center gap-1.5 border-b border-line-default px-2 py-1.5'
// 개수는 알약이 아니라 숫자다 — 제목 옆 같은 자리에 같은 크기로 서야 세 섹션을 훑을 수 있다.
const countCls = 'text-2xs tabular-nums text-content-tertiary'
const iconCls =
  'rounded p-1 text-content-faint transition-colors hover:bg-surface-active hover:text-content-primary'

/** 접이식 섹션 하나 — 머리줄(제목·요약·오른쪽 손잡이) + 몸. 도킹 모드의 탭 띠와 같은 일을 한다. */
function Section({
  id,
  title,
  summary,
  help,
  trailing,
  children,
}: {
  id: string
  title: string
  summary?: React.ReactNode
  /** 제목 줄의 `?` — 접기 버튼 **밖**에 둔다(버튼 안의 버튼은 누르면 섹션까지 접힌다). */
  help?: React.ReactNode
  trailing?: React.ReactNode
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(true)
  return (
    <>
      <div className={headCls}>
        <button
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          data-testid={`sec-${id}`}
        >
          <ChevronRight
            className={`h-3 w-3 shrink-0 text-content-faint transition-transform ${open ? 'rotate-90' : ''}`}
          />
          <span className="text-xs font-semibold text-content-muted">{title}</span>
          {summary ? <span className={countCls}>{summary}</span> : null}
        </button>
        {help}
        {trailing}
      </div>
      {/* 펼친 섹션은 **남은 높이를 나눠 갖는다**(`flex-1` + `min-h-0`) — 스크롤은 패널 자신이 한다.
          바깥을 스크롤시키고 안쪽을 `flex-1`로 두면 높이가 auto라 목록이 0px로 접힌다. */}
      {open ? <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div> : null}
    </>
  )
}

export function Sidebar() {
  useStore(density)
  const [width, setWidth] = useState(readWidth)

  return (
    <div
      className="relative flex h-full min-h-0 flex-col"
      style={{ width: `${width}px` }}
      data-testid="sidebar"
    >
      <div className="flex min-h-0 flex-1 flex-col">
        <Section
          id="robot"
          title="로봇"
          summary={<RobotsSummary />}
          help={
            <IndicatorLegend
              testid="legend-robot"
              extra="행 왼쪽 색 막대 = 로봇 색(맵의 작업 테두리) · 테두리 강조 행 = 명령을 보낼 로봇 · 칩에 마우스를 올리면 사유"
            />
          }
        >
          <RobotsPane />
        </Section>

        <Section
          id="plc"
          title="PLC"
          summary={<PlcSummary />}
          help={
            <IndicatorLegend
              testid="legend-plc"
              extra="레이아웃은 불일치일 때만 표시 · 칩에 마우스를 올리면 불일치 DB · 행을 누르면 상세"
            />
          }
          trailing={
            <button
              className={iconCls}
              title={density.isCompact ? '표준 밀도로' : '조밀 밀도로'}
              aria-label="표시 밀도 전환"
              aria-pressed={density.isCompact}
              data-testid="sidebar-density"
              onClick={() => density.toggle()}
            >
              {density.isCompact ? <Rows3 className="h-4 w-4" /> : <Rows2 className="h-4 w-4" />}
            </button>
          }
        >
          <PlcPane />
        </Section>

        <Section id="status" title="상태" trailing={<StatusSummary />}>
          <StatusPane />
        </Section>
      </div>

      {/* 폭 손잡이 — 도킹 존과 같은 스플리터(키보드 ←→ 로도 끈다). */}
      <div className="absolute inset-y-0 -right-1 z-10 flex">
        <Splitter
          axis="col"
          size={width}
          min={MIN_W}
          max={MAX_W}
          label="사이드바 폭"
          onResize={setWidth}
          onCommit={(v) => localStorage.setItem(LS_WIDTH, String(Math.round(v)))}
        />
      </div>
    </div>
  )
}
