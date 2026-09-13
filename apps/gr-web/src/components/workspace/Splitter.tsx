// 존 사이의 크기 손잡이 — 포인터 드래그와 **키보드** 둘 다 받는다.
//
// 키보드를 받는 이유는 접근성이 아니라 현장이다: 장갑 낀 손으로 2px 선을 집는 것보다 탭으로 손잡이에
// 와서 화살표를 누르는 편이 빠르고, 터치 모니터에서는 드래그가 스크롤로 먹힌다. `role="separator"` +
// `aria-valuenow`가 있으면 스크린리더도 지금 폭을 읽는다.
//
// 드래그 중 계산은 **시작 지점 기준**이다(현재 포인터 좌표를 그대로 크기로 쓰지 않는다) — 존이
// 왼쪽 끝에 붙어 있지 않으면 좌표와 폭이 다르고, 손잡이를 잡은 위치만큼 값이 튄다.
import { useRef } from 'react'
import type * as React from 'react'

export interface SplitterProps {
  /** `col` 세로선(폭을 끈다) · `row` 가로선(높이를 끈다). */
  axis: 'col' | 'row'
  /** 지금 크기(px). */
  size: number
  min: number
  max: number
  /** 포인터를 **양(오른쪽/아래)** 방향으로 끌 때 크기가 줄어드는 자리(오른쪽 존·하단 존). */
  invert?: boolean
  label: string
  onResize: (next: number) => void
  /** 드래그가 끝났을 때(저장 시점을 호출부가 고를 수 있게 — 매 프레임 적을 이유가 없다). */
  onCommit?: (next: number) => void
}

export function Splitter({
  axis,
  size,
  min,
  max,
  invert = false,
  label,
  onResize,
  onCommit,
}: SplitterProps) {
  /** 드래그 시작 시점의 포인터 좌표와 크기 — 델타 계산의 기준. */
  const from = useRef<{ at: number; size: number } | null>(null)
  const latest = useRef(size)

  const coord = (e: { clientX: number; clientY: number }): number =>
    axis === 'col' ? e.clientX : e.clientY

  function down(e: React.PointerEvent<HTMLDivElement>): void {
    from.current = { at: coord(e), size }
    latest.current = size
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  function move(e: React.PointerEvent<HTMLDivElement>): void {
    const start = from.current
    if (!start) return
    const delta = (coord(e) - start.at) * (invert ? -1 : 1)
    const next = Math.min(max, Math.max(min, start.size + delta))
    latest.current = next
    onResize(next)
  }

  function up(e: React.PointerEvent<HTMLDivElement>): void {
    if (!from.current) return
    from.current = null
    e.currentTarget.releasePointerCapture(e.pointerId)
    onCommit?.(latest.current)
  }

  function key(e: React.KeyboardEvent<HTMLDivElement>): void {
    // 세로선은 ←→, 가로선은 ↑↓. Shift는 큰 걸음(16 → 64).
    const step = e.shiftKey ? 64 : 16
    const keys =
      axis === 'col' ? { less: 'ArrowLeft', more: 'ArrowRight' } : { less: 'ArrowUp', more: 'ArrowDown' }
    let next: number | null = null
    if (e.key === keys.less) next = size + (invert ? step : -step)
    else if (e.key === keys.more) next = size + (invert ? -step : step)
    else if (e.key === 'Home') next = min
    else if (e.key === 'End') next = max
    if (next === null) return
    e.preventDefault()
    const v = Math.min(max, Math.max(min, next))
    onResize(v)
    onCommit?.(v)
  }

  return (
    <div
      role="separator"
      tabIndex={0}
      aria-orientation={axis === 'col' ? 'vertical' : 'horizontal'}
      aria-label={label}
      aria-valuenow={Math.round(size)}
      aria-valuemin={min}
      aria-valuemax={max}
      data-testid={`splitter-${label}`}
      // 기하(7px 잡을 면 · 1px 선 · hover 3px)는 `app.css`의 `.ds-splitter*`가 든다 — 임의값을
      // 여기 뿌리지 않는다. 이 컴포넌트는 **동작**만 안다(포인터·키보드·값 계산).
      className={
        (axis === 'col' ? 'ds-splitter ds-splitter-col' : 'ds-splitter ds-splitter-row') +
        ' focus-visible:outline-none'
      }
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onKeyDown={key}
    >
      {/* 선은 **항상 보인다.** 투명하게 두면 "여기를 끌 수 있다"가 화면에 없고, 존 경계도 1px 보더
          하나로만 남아 면이 어디서 갈리는지 읽히지 않는다. 손을 얹으면 두꺼워지며 초록으로 바뀐다. */}
      <span className="ds-splitter-line" />
    </div>
  )
}
