// 그리퍼 레이저 센서 진단(LASERDIAG) 표시 도우미 — 순수 함수만. 화면은 components/measure/LaserSensor.tsx.
import type { LaserDiagEntry, LaserZCal } from './types'

/** 방향 순서 — LASERDIAG.Sensor[k] / BeadDev[k] 의 k (0 L, 1 F, 2 R, 3 B). */
export const LASER_DIRS = ['L', 'F', 'R', 'B'] as const

/** LGR_LaserDiagEntry.Source */
export const LASER_SOURCE: Record<number, string> = {
  1: 'PICK 하강',
  2: '들어갈 때',
  3: '나갈 때',
  4: '수동 측정',
}

/** LGR_LaserZCal.ErrorCode */
export const ZCAL_ERROR: Record<number, string> = {
  1: '취소',
  2: '표준편차 초과 (타이어 기울기·변형 의심)',
  3: '보정량 초과',
}

const FLAG_BITS: readonly (readonly [number, string])[] = [
  [8, '편차 폭 초과'],
  [9, '비평면 초과'],
  [10, '타이어 미검출'],
  [11, '원 맞춤 실패'],
]

/** DiagFlags → 표시 목록. X0..X3 방향별 무응답, X4..X7 방향별 불안정, X8.. 측정 단위 플래그. */
export function diagFlagLabels(flags: number): string[] {
  const out: string[] = []
  for (let k = 0; k < 4; k++) if (flags & (1 << k)) out.push(`${LASER_DIRS[k]} 무응답`)
  for (let k = 0; k < 4; k++) if (flags & (1 << (4 + k))) out.push(`${LASER_DIRS[k]} 불안정`)
  for (const [bit, label] of FLAG_BITS) if (flags & (1 << bit)) out.push(label)
  return out
}

export type ZCalPhase = 'idle' | 'busy' | 'done' | 'error'

/** 교정 진행 단계 — Busy 가 가장 우선, 그다음 Error, Done. */
export function zcalPhase(z: Pick<LaserZCal, 'Busy' | 'Done' | 'Error'> | null | undefined): ZCalPhase {
  if (!z) return 'idle'
  if (z.Busy) return 'busy'
  if (z.Error) return 'error'
  if (z.Done) return 'done'
  return 'idle'
}

/** 활성 방향 수 — 최근 이력에 B 센서 샘플이 있으면 4 (TBR), 아니면 3 (PCR). */
export function activeDirs(entries: readonly Pick<LaserDiagEntry, 'Samples'>[]): 3 | 4 {
  return entries.some((e) => (e.Samples?.[3] ?? 0) > 0) ? 4 : 3
}
