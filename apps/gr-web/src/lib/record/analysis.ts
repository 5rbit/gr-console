// 측정 기록 분석 — 순수 함수만. 샘플은 /api/record/{id} 가 돌려주는 WEBMON 영역 JSON 한 줄씩이다.
// Axis 인덱스 0 X, 1 Y, 2 Z, 3 G. Gripper.GID 인덱스 0 L, 1 F, 2 R, 3 B. mark 줄에는 t 와 mark 만 있다.
import type { GripperState } from '../types'

export interface RecAxis {
  Position: number
  Target: number
  Speed: number
  Torque: number
  Running: boolean
  StandStill: boolean
}

export interface RecSample {
  t: number
  at?: string
  mark?: string
  Mode?: number
  Task?: {
    TaskType: number
    Position: number[]
    Item: { Code: number; InnerDiameter: number; OuterDiameter: number; Height: number }
    Cell: { Id: number; Position: number[] }
  }
  Step?: { Now: number }
  Alarm?: { Fault: boolean; Warn: boolean; FaultCode: number[]; WarnCode: number[] }
  Axis?: RecAxis[]
  Gripper?: { ItemDetect: boolean; GID: number[]; FLD: number; State?: GripperState }
  Measure?: {
    Item?: Record<string, number | boolean>
    LastBead?: { Enable: boolean; Busy: boolean; Done: boolean; LastBidPos: number; PickZTarget: number }
  }
  MeasLog?: { Total: number; Count: number }
}

/** 레이저가 아무것도 못 볼 때의 거리 (PARA.Sensor.ItemDetectDistance_H 800 으로 클램프) 에서 조금 뺀 값. */
export const LASER_FAR = 790

export function isData(s: RecSample | undefined): s is RecSample & { Axis: RecAxis[] } {
  return Boolean(s) && !s?.mark && Array.isArray(s?.Axis)
}

/** 활성 방향 수 — B(GID[3]) 가 한 번이라도 유효 거리를 보이면 4 (TBR), 아니면 3 (PCR). */
export function dirCount(samples: readonly RecSample[]): 3 | 4 {
  return samples.some((s) => {
    const d = s.Gripper?.GID?.[3]
    return d !== undefined && d > 0 && d < LASER_FAR
  })
    ? 4
    : 3
}

export type SegmentKind = 'in' | 'out' | 'pick' | 'grip'

export interface Segment {
  kind: SegmentKind
  label: string
  i0: number
  i1: number
  t0: number
  t1: number
}

const SEGMENT_DEFS: readonly (readonly [SegmentKind, string, (s: RecSample) => boolean])[] = [
  ['in', '들어갈 때 측정', (s) => Boolean(s.Measure?.Item?.InBusy)],
  ['out', '나갈 때 측정', (s) => Boolean(s.Measure?.Item?.OutBusy)],
  ['pick', 'PICK 하강 비드 측정', (s) => Boolean(s.Measure?.LastBead?.Busy)],
  ['grip', 'Step 500 그립 / 토크 측정', (s) => s.Step?.Now === 500],
]

/** 측정·그립 구간 — 조건이 연속으로 참인 데이터 샘플 묶음 (시작 시각 순). */
export function segments(samples: readonly RecSample[]): Segment[] {
  const out: Segment[] = []
  for (const [kind, label, pred] of SEGMENT_DEFS) {
    let i0 = -1
    let last = -1
    for (let i = 0; i < samples.length; i++) {
      const s = samples[i]
      if (!isData(s)) continue
      if (pred(s)) {
        if (i0 < 0) i0 = i
        last = i
      } else if (i0 >= 0) {
        out.push({ kind, label, i0, i1: last, t0: samples[i0]?.t ?? 0, t1: samples[last]?.t ?? 0 })
        i0 = -1
      }
    }
    if (i0 >= 0) out.push({ kind, label, i0, i1: last, t0: samples[i0]?.t ?? 0, t1: samples[last]?.t ?? 0 })
  }
  return out.sort((a, b) => a.t0 - b.t0)
}

export interface TaskSpec {
  code: number
  taskType: number
  id: number
  od: number
  height: number
  cellZ: number
  /** 작업 목표 Z (Task.Position[Z]) — LastBidPos 기준 */
  targetZ: number
}

/** 구간 안 첫 데이터 샘플의 작업 규격 (규격 내경이 0 이면 null). */
export function taskSpec(samples: readonly RecSample[], i0 = 0, i1 = samples.length - 1): TaskSpec | null {
  for (let i = i0; i <= i1; i++) {
    const s = samples[i]
    const it = s?.Task?.Item
    if (!isData(s) || !it || !(it.InnerDiameter > 0)) continue
    return {
      code: it.Code,
      taskType: s.Task?.TaskType ?? 0,
      id: it.InnerDiameter,
      od: it.OuterDiameter,
      height: it.Height,
      cellZ: s.Task?.Cell?.Position?.[2] ?? 0,
      targetZ: s.Task?.Position?.[2] ?? 0,
    }
  }
  return null
}

export interface ProfilePoint {
  t: number
  z: number
  dist: number
  /** 센서가 본 지름 = G + 2 × 거리 */
  dia: number
  g: number
}

/** 방향 k 의 유효 거리 샘플 (0 이하·클램프 값 제외). */
export function profile(samples: readonly RecSample[], k: number, i0 = 0, i1 = samples.length - 1): ProfilePoint[] {
  const out: ProfilePoint[] = []
  for (let i = i0; i <= i1; i++) {
    const s = samples[i]
    if (!isData(s)) continue
    const d = s.Gripper?.GID?.[k]
    const z = s.Axis[2]?.Position
    const g = s.Axis[3]?.Position
    if (d === undefined || z === undefined || g === undefined || !(d > 0) || d >= LASER_FAR) continue
    out.push({ t: s.t, z, dist: d, dia: g + 2 * d, g })
  }
  return out
}

export interface BeadReplay {
  dir: number
  seen: number
  minDist: number | null
  zAtMin: number | null
  beadTop: number | null
  beadBottom: number | null
  lastBeadZ: number | null
  edgeZ: number | null
}

export interface ReplayOptions {
  /** 비드 판정 여유 (mm) — FB 의 BeadMargin, 0 이하면 10 */
  band?: number
  /** true 들어갈 때(하강) — 경계는 밖→안, false 나갈 때(상승) — 안→밖 */
  descending?: boolean
  i0?: number
  i1?: number
}

/**
 * FB_MeasureTireLaser 의 방향별 판정을 기록 샘플로 다시 돌린다 (빔 높이 오프셋 없이 그리퍼 Z 기준).
 * 샘플 주기(수십 ms)가 PLC 스캔보다 길어 경계·마지막 비드 높이는 샘플 간격만큼 근사다.
 */
export function replay(samples: readonly RecSample[], k: number, spec: Pick<TaskSpec, 'id' | 'od'>, o: ReplayOptions = {}): BeadReplay {
  const band = o.band !== undefined && o.band > 0 ? o.band : 10
  const descending = o.descending !== false
  const midGate = spec.id > 0 && spec.od > spec.id
  const mid = (spec.id + spec.od) / 2
  let min = Infinity
  let zAtMin: number | null = null
  let seen = 0
  let top = -Infinity
  let bottom = Infinity
  let last: number | null = null
  let inside = 0
  let outside = 0
  let wasInside = false
  let edge: number | null = null
  const i1 = o.i1 ?? samples.length - 1
  for (let i = o.i0 ?? 0; i <= i1; i++) {
    const s = samples[i]
    if (!isData(s)) continue
    const d = s.Gripper?.GID?.[k] ?? 0
    const z = s.Axis[2]?.Position ?? 0
    const dia = (s.Axis[3]?.Position ?? 0) + 2 * d
    const isSeen = d > 0 && d < LASER_FAR && (spec.od > 0 ? dia < spec.od : true)
    if (isSeen) {
      seen++
      if (d < min) {
        min = d
        zAtMin = z
      }
      inside++
      outside = 0
    } else {
      outside++
      inside = 0
    }
    if (isSeen && d <= min + band && (!midGate || dia < mid)) {
      last = z
      top = Math.max(top, z)
      bottom = Math.min(bottom, z)
    }
    if (edge === null) {
      if (descending) {
        if (inside >= 2) edge = z
      } else {
        if (inside >= 2) wasInside = true
        if (wasInside && outside >= 2) edge = z
      }
    }
  }
  return {
    dir: k,
    seen,
    minDist: seen ? min : null,
    zAtMin,
    beadTop: last === null ? null : top,
    beadBottom: last === null ? null : bottom,
    lastBeadZ: last,
    edgeZ: edge,
  }
}

export interface BitEdge {
  t: number
  bit: keyof GripperState
  on: boolean
  g: number
  gTarget: number
  torque: number
  z: number
}

/** 그리퍼 상태 비트의 켜짐·꺼짐 변화 (구간 안). */
export function bitEdges(
  samples: readonly RecSample[],
  bits: readonly (keyof GripperState)[],
  i0 = 0,
  i1 = samples.length - 1,
): BitEdge[] {
  const out: BitEdge[] = []
  const prev = new Map<keyof GripperState, boolean>()
  for (let i = i0; i <= i1; i++) {
    const s = samples[i]
    const st = s?.Gripper?.State
    if (!isData(s) || !st) continue
    for (const b of bits) {
      const on = Boolean(st[b])
      const p = prev.get(b)
      if (p !== undefined && p !== on) {
        out.push({ t: s.t, bit: b, on, g: s.Axis[3]?.Position ?? 0, gTarget: s.Axis[3]?.Target ?? 0, torque: s.Axis[3]?.Torque ?? 0, z: s.Axis[2]?.Position ?? 0 })
      }
      prev.set(b, on)
    }
  }
  return out
}

export interface StallEpisode {
  t0: number
  t1: number
  maxStallTime: number
  g: number
  gTarget: number
  torque: number
  torqueReached: boolean
  itemDetect: boolean
  alarms: number[]
}

/** G 멈춤(State.Stall) 이 이어진 구간 — 끝 샘플의 위치·토크·아이템 감지와 구간 중 알람 코드. */
export function stallEpisodes(samples: readonly RecSample[], i0 = 0, i1 = samples.length - 1): StallEpisode[] {
  const out: StallEpisode[] = []
  let cur: StallEpisode | null = null
  for (let i = i0; i <= i1; i++) {
    const s = samples[i]
    if (!isData(s)) continue
    const st = s.Gripper?.State
    if (st?.Stall) {
      const g = s.Axis[3]
      cur ??= { t0: s.t, t1: s.t, maxStallTime: 0, g: 0, gTarget: 0, torque: 0, torqueReached: false, itemDetect: false, alarms: [] }
      cur.t1 = s.t
      cur.maxStallTime = Math.max(cur.maxStallTime, st.StallTime ?? 0)
      cur.g = g?.Position ?? 0
      cur.gTarget = g?.Target ?? 0
      cur.torque = g?.Torque ?? 0
      cur.torqueReached = Boolean(st.TorqueReached)
      cur.itemDetect = Boolean(s.Gripper?.ItemDetect)
      for (const c of alarmCodes([s])) if (!cur.alarms.includes(c)) cur.alarms.push(c)
    } else if (cur) {
      out.push(cur)
      cur = null
    }
  }
  if (cur) out.push(cur)
  return out
}

/** 샘플들에 나타난 알람 코드 (Fault / Warn 최근 코드 목록, 0 제외, 오름차순). */
export function alarmCodes(samples: readonly RecSample[]): number[] {
  const set = new Set<number>()
  for (const s of samples) {
    for (const c of [...(s.Alarm?.FaultCode ?? []), ...(s.Alarm?.WarnCode ?? [])]) if (c) set.add(c)
  }
  return [...set].sort((a, b) => a - b)
}

/** 시간(초) 대 값 점열 — 값이 없는 샘플은 건너뛴다. */
export function timeSeries(
  samples: readonly RecSample[],
  value: (s: RecSample & { Axis: RecAxis[] }) => number | undefined,
  i0 = 0,
  i1 = samples.length - 1,
): { x: number; y: number }[] {
  const out: { x: number; y: number }[] = []
  for (let i = i0; i <= i1; i++) {
    const s = samples[i]
    if (!isData(s)) continue
    const y = value(s)
    if (y !== undefined && Number.isFinite(y)) out.push({ x: s.t / 1000, y })
  }
  return out
}
