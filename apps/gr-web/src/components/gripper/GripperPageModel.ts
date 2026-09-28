// 그리퍼 화면의 순수 모델 — 코드 이름·톤, Mech 곡선 계열, LEARN 가능 판정, 표 행. DOM·fetch 를 모른다.
//
// 이름표는 PLC 상수 `LGR_Const_Gripper`(GRIP_ST_* / GRIP_* 모드 / GRIP_E_*)를 그대로 옮긴 것이다 — 백엔드도
// 같은 이름을 `live.CodeName` 으로 내지만 그건 2 초 폴이고, Code 는 상태 스트림(WEBMON) 박자로 바뀐다.
import type { XYRule, XYSeries } from '../../lib/ui/viz/xyChartModel'
import type { Status } from '../../lib/ui/status'
import type { GripperBins, GripperLive, GripperTune } from '../../lib/types'

/** `GRIP_ST_*` — Status.Code. */
export const GRIP_CODE: Readonly<Record<number, string>> = {
  0: 'IDLE',
  10: 'RELEASED',
  20: 'OPENING',
  30: 'CLOSING',
  40: 'GRIPPED_EMPTY',
  50: 'HOLDING',
  60: 'MEASURING',
  65: 'LEARNING',
  70: 'STALLED',
  80: 'TIMEOUT',
  90: 'OBSTACLE',
  92: 'THERMAL',
  95: 'FAULT',
}

/** `GRIP_*` 모드 — 진행 중 요청. */
export const GRIP_MODE: Readonly<Record<number, string>> = {
  0: 'IDLE',
  1: 'MOVE',
  2: 'GRIP',
  3: 'CLOSE',
  4: 'MEASURE_OPEN',
  5: 'RETRY',
  6: 'LEARN',
  7: 'LIMIT',
}

/** `GRIP_E_*` — Result.ErrorCode. */
export const GRIP_ERROR: Readonly<Record<number, string>> = {
  0: 'NONE',
  1: 'BUSY',
  2: 'TORQ_ECHO',
  3: 'RANGE_MAX',
  4: 'ABORT',
  5: 'TIMEOUT',
  6: 'RETRY_EXHAUSTED',
  7: 'OBSTACLE',
  8: 'LEARN',
  9: 'THERMAL',
  10: 'NOT_READY',
}

/** `GRIP_OWNER_*` — 지금 그리퍼를 쥔 주체. */
export const GRIP_OWNER: Readonly<Record<number, string>> = {
  0: 'NONE',
  1: 'TASK',
  2: 'MEASURE',
  3: 'MANUAL_MEASURE',
  4: 'MANUAL',
  5: 'LEARN',
}

export function ownerName(owner: number | null | undefined): string {
  if (owner === null || owner === undefined) return '-'
  return GRIP_OWNER[owner] ?? String(owner)
}

/** 토크 제한 에코 — 드라이브가 돌려준 PV 가 SV 와 같은가(0.05 % 안). 둘 다 없으면 null. */
export function limitEchoMatches(sv: number | null | undefined, pv: number | null | undefined): boolean | null {
  if (sv === null || sv === undefined || pv === null || pv === undefined) return null
  if (!Number.isFinite(sv) || !Number.isFinite(pv)) return null
  return Math.abs(sv - pv) <= 0.05
}

/** `Tune.LearnError` — 마지막 LEARN 실패 원인. */
export const LEARN_ERROR: Readonly<Record<number, string>> = {
  0: '없음',
  1: '샘플 부족',
  2: '급변',
  3: 'Need > Protect',
  4: '화물 있음',
  5: '토크 정지',
}

/** `Mech[1..2]` 곡선의 뜻 — [0] 파지 속도 등급, [1] 측정 느린 속도. */
export const CURVE_LABEL = ['Mech[1] 파지 속도', 'Mech[2] 측정 느린 속도'] as const

/** 모르는 값은 이름으로 꾸미지 않고 숫자 그대로. */
export function codeName(code: number | null | undefined): string {
  if (code === null || code === undefined) return '-'
  return GRIP_CODE[code] ?? String(code)
}

export function modeLabel(mode: number | null | undefined): string {
  if (mode === null || mode === undefined) return '-'
  return GRIP_MODE[mode] ?? String(mode)
}

export function errorName(code: number | null | undefined): string {
  if (!code) return 'NONE'
  return GRIP_ERROR[code] ?? String(code)
}

/** Code → 상태색. 정상 정지(IDLE/RELEASED)는 칠하지 않는다 — 정상을 칠하면 붉은 하나가 사라진다. */
export function codeTone(code: number | null | undefined): Status {
  switch (code) {
    case 50:
      return 'ok'
    case 20:
    case 30:
    case 60:
    case 65:
      return 'info'
    case 40:
    case 70:
    case 80:
    case 90:
      return 'warn'
    case 92:
    case 95:
      return 'fault'
    default:
      return 'neutral'
  }
}

/** 곡선 칸의 가운데 G 위치(mm) — `PARA.Drive.RangeMin/RangeMax["G"]` 를 `count` 칸으로 나눈 것. */
export function binCenters(b: GripperBins): number[] {
  const n = Math.max(1, b.count)
  const w = b.width > 0 ? b.width : (b.range_max - b.range_min) / n
  return Array.from({ length: n }, (_, k) => b.range_min + w * (k + 0.5))
}

/** 학습된 곡선만 계열로 — 안 된 곡선은 없는 것이다(0 을 선으로 그리면 "값이 0" 으로 읽힌다). */
export function mechSeries(tune: GripperTune | null, bins: GripperBins): XYSeries[] {
  if (!tune) return []
  const xs = binCenters(bins)
  const out: XYSeries[] = []
  const tones = [0, 2]
  for (let i = 0; i < 2; i++) {
    if (!tune.Valid?.[i]) continue
    const ys = tune.Mech?.[i] ?? []
    const points = xs.map((x, k) => ({ x, y: Number(ys[k] ?? 0) }))
    out.push({ name: `${CURVE_LABEL[i]} (Spd ${tune.LearnedSpd?.[i] ?? '-'})`, tone: tones[i], points })
  }
  return out
}

/** 지금 G 위치의 세로 기준선 — 값이 없으면 그리지 않는다. */
export function gRule(gPos: number | null | undefined): XYRule[] {
  if (gPos === null || gPos === undefined || !Number.isFinite(gPos)) return []
  return [{ axis: 'x', value: gPos, label: 'G', tone: 4 }]
}

/** 학습된 곡선이 하나라도 있나. */
export function anyValid(tune: GripperTune | null): boolean {
  return Boolean(tune?.Valid?.some(Boolean))
}

/** LEARN 을 막는 사유 — 없으면 undefined. PLC 수락 조건(수동/정비 모드 · 화물 없음)과 명령 경로를 본다. */
export function learnDisabledReason(
  mode: string | null,
  live: Pick<GripperLive, 'ItemPresent' | 'ItemDetect' | 'Busy' | 'Code'> | null,
  cmdReady: boolean,
  demo = false,
): string | undefined {
  if (!cmdReady) return '명령 경로(OPC UA) 준비 안 됨'
  if (live?.Code === 65) return '학습 중'
  if (!demo && mode !== null && mode !== 'MANUAL' && mode !== 'MAINT')
    return `MANUAL·MAINT 에서만 LEARN (지금 ${mode})`
  if (live?.ItemPresent || live?.ItemDetect) return '화물이 감지된 상태 — 그리퍼를 비우고 다시'
  return undefined
}

export interface ParaRowView {
  name: string
  param: number
  value: number | null
  group: 'Machine' | 'Task' | 'Sensor'
}

/** 그리퍼 PARA — 이름 · p 번호 · 값(없으면 null). 순서는 p 번호. */
const PARA_NO: readonly (readonly [string, number, 'Machine' | 'Task'])[] = [
  ['G_TorqRefDia', 37, 'Machine'],
  ['G_OpenTorq_Nm', 38, 'Machine'],
  ['G_CloseTorq_Nm', 39, 'Machine'],
  ['G_MeasureTorq_Nm', 44, 'Machine'],
  ['G_ProtectForce_N', 45, 'Machine'],
  ['G_TorqMaxPct', 46, 'Machine'],
  ['G_OpenTorq_Pct', 974, 'Task'],
  ['G_CloseTorq_Pct', 975, 'Task'],
  ['G_MeasureTorq_Pct', 976, 'Task'],
  ['G_RetryTorq_Pct', 977, 'Task'],
  ['G_MeasureApproachMargin', 978, 'Task'],
  ['G_MeasureSlowSpd', 979, 'Task'],
  ['G_ContactDwell', 980, 'Task'],
  ['G_ContactFactor_k', 981, 'Task'],
  ['G_ObstacleMargin', 982, 'Task'],
  ['G_GripRetryMax', 983, 'Task'],
  ['G_AtSpeedTol', 984, 'Task'],
  ['G_HoldTorqPct', 985, 'Task'],
  ['G_ErrorHoldTime', 986, 'Task'],
  ['G_HoldFactor', 987, 'Task'],
  ['G_HoldReduceTime', 988, 'Task'],
  ['G_MotorTempMax', 989, 'Task'],
  ['G_LoadAvgMax', 990, 'Task'],
]

/** 인치별 수동 토크 — OpenPct p450~p462(12..24"), MeasPct p463~p475. */
const INCHES = Array.from({ length: 13 }, (_, i) => 12 + i)
const INCH_PARA: readonly (readonly [string, number, 'Sensor'])[] = [
  ...INCHES.map((inch) => [`G_Inch${inch}_OpenPct`, 450 + inch - 12, 'Sensor'] as const),
  ...INCHES.map((inch) => [`G_Inch${inch}_MeasPct`, 463 + inch - 12, 'Sensor'] as const),
]

export function paraRows(para: Record<string, number> | null | undefined): ParaRowView[] {
  return [...PARA_NO, ...INCH_PARA].map(([name, param, group]) => {
    const v = para?.[name]
    return { name, param, group, value: typeof v === 'number' && Number.isFinite(v) ? v : null }
  })
}
