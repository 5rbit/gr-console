// 조작 인스펙터 — 조작이 화면마다 흩어지지 않게 **한 판**에 모으는 껍데기.
// `Toolbar`가 띠 하나를 맡는다면 이 판은 화면 오른쪽 전체 높이를 맡는다
// (폭 248, 메뉴바 밑부터 상태바까지 — 화면 헤더가 그 사이에 끼지 않는다).
//
// 내부 순서는 고정이다(UX 규칙 ⑦). 화면이 바뀌어도 **컨트롤이 움직이지 않아야** 근육 기억이 선다:
//   머리 → 세그먼트 → 절반 폭 2열 → 화면 전용 블록 → 안내 띠 → 정지 → 마지막 조작.
//
// 안내 띠와 정지는 **스크롤 영역 밖**이다. 안내 문구를 스크롤 안에 두면 판을 굴릴 때마다
// 정지 버튼이 따라 움직인다 — 그것만은 안 된다.
//
// 면은 흰색 그대로 두고 **머리띠와 경계만** 연한 파랑으로 갈라 둔다. 면 전체를 물들이면
// 값 영역과의 대비가 과해져 정작 값이 눌린다. 컨트롤은 브랜드 그린이다 —
// 갈리는 것은 면이지 컨트롤이 아니다.
import type * as React from 'react'
import { Button } from './Button'
import { HelpTip } from './HelpTip'
import { cn } from '../utils'
import type { SegmentGroup, OpsAction, StopAction } from './opsPanelModel'

export type { SegmentGroup, OpsAction, StopAction }

export interface OpsPanelProps {
  /** 머리띠 라벨 — 세로로 접히지 않게 `nowrap`. */
  title?: string
  /** 조작 대상 — 넘치면 `…` + 툴팁. */
  target?: string
  segments?: SegmentGroup[]
  /** 절반 폭 2열 버튼. */
  grid?: OpsAction[]
  /** 사유·거부 문구. 스크롤 밖 고정 띠라 떴다 사라져도 위 컨트롤이 움직이지 않는다. */
  note?: string
  stop?: StopAction | null
  /** 마지막으로 나간 조작 한 줄(28px). */
  lastAction?: string
  /** 화면 전용 블록 — 조그·이동, 자동 모드의 작업 큐, 미니 파형. */
  children?: React.ReactNode
}

export function OpsPanel({
  title = '조작',
  target = '',
  segments = [],
  grid = [],
  note = '',
  stop = null,
  lastAction = '',
  children,
}: OpsPanelProps) {
  const StopIcon = stop?.icon
  return (
    <aside
      className="flex w-inspector flex-none flex-col border-l border-control-border bg-surface-panel"
      aria-label={`${title} 판넬`}
    >
      {/* ① 머리 — 연한 파랑 띠. 라벨은 접히지 않고, 대상만 줄었다가 넘치면 잘린다. */}
      <div className="flex flex-none items-center gap-1.5 border-b border-control-border bg-control-header px-2 py-1">
        <span className="text-2xs font-semibold whitespace-nowrap text-control-text">{title}</span>
        {target ? (
          <span className="min-w-0 truncate font-mono text-2xs text-control-text/80" title={target}>
            {target}
          </span>
        ) : null}
      </div>

      {/* 스크롤 영역 — 여기만 구른다. */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* ② 세그먼트 — 상호배타 상태 전환만. 붙은 탭이라 '하나를 고른다'가 형태로 읽힌다. */}
        {segments.map((g) => (
          <div key={g.label} className="border-b border-line-subtle px-2 py-1.5">
            <div className="mb-1 flex items-center gap-1">
              <span className="text-3xs font-medium text-content-muted">{g.label}</span>
              {g.hint ? <HelpTip text={g.hint} title={g.label} /> : null}
            </div>
            <div className="flex overflow-hidden rounded-md border border-line-default">
              {g.options.map((o, i) => (
                <button
                  key={o.label}
                  type="button"
                  className={cn(
                    'h-control-sm min-w-0 flex-1 truncate px-1 text-2xs transition-colors',
                    i > 0 && 'border-l border-line-default',
                    o.active
                      ? 'bg-accent font-semibold text-white'
                      : 'bg-surface-panel text-content-secondary hover:bg-surface-hover',
                    o.disabled && 'cursor-not-allowed opacity-50',
                  )}
                  disabled={o.disabled}
                  title={o.title}
                  aria-pressed={!!o.active}
                  onClick={() => o.run?.()}
                >
                  {o.label}
                </button>
              ))}
            </div>
          </div>
        ))}

        {/* ③ 절반 폭 2열 — 나머지 조작. 폭이 같아 누르기 쉽고 자리가 외워진다. */}
        {grid.length ? (
          <div className="grid grid-cols-2 gap-1 border-b border-line-subtle p-2">
            {grid.map((a) => {
              const Icon = a.icon
              return (
                <Button
                  key={a.label}
                  size="sm"
                  intent={a.intent ?? 'neutral'}
                  disabled={a.disabled}
                  title={a.title}
                  className="w-full justify-start"
                  icon={Icon ? <Icon size={14} /> : undefined}
                  data-testid={a.testid}
                  onClick={() => a.run?.()}
                >
                  <span className="truncate">{a.label}</span>
                </Button>
              )
            })}
          </div>
        ) : null}

        {/* ④⑤ 화면 전용 블록 · 미니 시각화 */}
        {children}
      </div>

      {/* ⑥ 안내 띠 — 스크롤 밖. 사유가 떴다 사라져도 위 컨트롤은 제자리다. */}
      {note ? (
        <div className="flex-none border-t border-line-subtle bg-surface-inset px-2 py-1 text-3xs leading-normal text-content-muted">
          {note}
        </div>
      ) : null}

      {/* ⑦ 정지 — 맨 아래 전폭 하나. 확인을 끼우지 않는다(UX 규칙 ⑧):
          급할 때 한 번을 더 누르게 만드는 UI는 안전 장치가 아니라 사고 원인이다. */}
      {stop ? (
        <div className="flex-none border-t border-fault bg-fault-soft p-2">
          <button
            type="button"
            className={cn(
              'flex h-control-stop w-full items-center justify-center gap-2 rounded-md bg-danger text-base font-semibold text-white transition-colors',
              'hover:bg-danger-hover focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none',
              stop.disabled && 'cursor-not-allowed opacity-50',
            )}
            disabled={stop.disabled}
            title={stop.title}
            onClick={() => stop.run?.()}
          >
            {StopIcon ? <StopIcon size={18} /> : null}
            {stop.label}
          </button>
          {stop.immediate !== false ? (
            <p className="mt-1 mb-0 text-center text-3xs text-danger-text">
              즉시 나갑니다 — 확인 없이, 자동 모드에서도 통과합니다
            </p>
          ) : null}
        </div>
      ) : null}

      {/* ⑧ 마지막 조작 한 줄 — 방금 무엇을 보냈는지가 판을 떠나지 않는다. */}
      {lastAction ? (
        <div
          className="flex h-control-sm flex-none items-center truncate border-t border-line-subtle bg-surface-inset px-2 text-3xs text-content-faint"
          title={lastAction}
        >
          {lastAction}
        </div>
      ) : null}
    </aside>
  )
}
