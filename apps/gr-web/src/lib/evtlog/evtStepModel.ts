// Task 스텝 막대 — STEP 이벤트 하나가 **떠난 스텝의 머문 시간**을 싣는다.
//
// STEP 행: `code` = 들어간 스텝, `b` = 떠난 스텝, `a` = `b` 에 머문 ms, `src` = proc 번호.
// 그래서 막대는 (`b`, 시작 = ts_ms − a, 길이 = a) 이고, 끝이 곧 그 행의 시각이다.
import type { EventRow } from './api'

/** `enum.proc` 의 Task — 스텝 이름표(`enum.task_step`)는 이 공정에만 있다. */
const PROC_TASK = 20

export interface StepBar {
  id: number
  proc: number
  procName: string
  /** 머문 스텝(`b`). */
  step: number
  stepName: string
  /** 다음 스텝(`code`). */
  next: number
  start: number
  dur: number
  plc: string
}

export interface StepLane {
  proc: number
  procName: string
  bars: StepBar[]
}

export interface StepChart {
  lanes: StepLane[]
  t0: number
  t1: number
  total: number
}

/** 카탈로그 enum 라벨 — 없으면 숫자 그대로. */
export function enumLabel(
  enums: Record<string, Record<string, string>> | null | undefined,
  kind: string,
  v: number,
): string {
  return enums?.[kind]?.[String(v)] ?? String(v)
}

export function stepBars(
  rows: readonly EventRow[],
  enums?: Record<string, Record<string, string>> | null,
): StepChart {
  const bars: StepBar[] = rows
    .filter((r) => r.cat_name === 'STEP' && r.a >= 0)
    .map((r) => ({
      id: r.id,
      proc: r.src,
      procName: enumLabel(enums, 'proc', r.src),
      step: r.b,
      stepName: enumLabel(enums, r.src === PROC_TASK ? 'task_step' : 'step', r.b),
      next: r.code,
      start: r.ts_ms - r.a,
      dur: r.a,
      plc: r.plc,
    }))
    .sort((x, y) => x.start - y.start || x.id - y.id)
  const byProc = new Map<number, StepLane>()
  for (const b of bars) {
    let lane = byProc.get(b.proc)
    if (!lane) {
      lane = { proc: b.proc, procName: b.procName, bars: [] }
      byProc.set(b.proc, lane)
    }
    lane.bars.push(b)
  }
  const lanes = [...byProc.values()].sort(
    (x, y) => (x.bars[0]?.start ?? 0) - (y.bars[0]?.start ?? 0) || x.proc - y.proc,
  )
  const t0 = bars.length ? Math.min(...bars.map((b) => b.start)) : 0
  const t1 = bars.length ? Math.max(...bars.map((b) => b.start + b.dur)) : 0
  return { lanes, t0, t1, total: bars.reduce((s, b) => s + b.dur, 0) }
}

/** 막대의 가로 자리(%) — 폭이 0 인 막대도 보이게 최소 폭을 준다. */
export function barGeometry(
  b: StepBar,
  t0: number,
  t1: number,
  minPct = 0.4,
): { left: number; width: number } {
  const span = Math.max(1, t1 - t0)
  const left = Math.min(100 - minPct, Math.max(0, ((b.start - t0) / span) * 100))
  const width = Math.min(100 - left, Math.max(minPct, (b.dur / span) * 100))
  return { left, width }
}
