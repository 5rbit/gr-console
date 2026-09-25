// 트레이스 채널 프리셋 — 자주 보는 묶음을 한 번에 고른다(순수 모듈).
//
// 경로는 GR2 계약의 WEBMON 레이아웃 그대로(`Axis[4]` = G, `Axis[3]` = Z — `Array["X".."G"]` 가 1 부터 센다).
// 카탈로그에 없는 경로는 `applyPreset` 이 걸러 낸다 — 옛 PLC 레이아웃에는 `Gripper.Drive.*` 가 없다.
import { CHAN_MAX } from './traceModel'

export type TracePresetId = 'gripper'

export interface TracePreset {
  id: TracePresetId
  label: string
  channels: readonly string[]
  divider: string
  flushMs: string
}

/** 그리퍼 토크 제어 — 제한/에코/도달과 위치·속도·토크를 같은 스캔에서. */
export const GRIPPER_PRESET: TracePreset = {
  id: 'gripper',
  label: '그리퍼',
  channels: [
    'WEBMON.Axis[4].Position',
    'WEBMON.Axis[4].Target',
    'WEBMON.Axis[4].Speed',
    'WEBMON.Axis[4].Torque',
    'WEBMON.Axis[4].LagError',
    'WEBMON.Gripper.LimitNow',
    'WEBMON.Gripper.Mech',
    'WEBMON.Gripper.Rise',
    'WEBMON.Gripper.TorqPct',
    'WEBMON.Gripper.ContactThr',
    'WEBMON.Gripper.Drive.TorqLimitPV',
    'WEBMON.Gripper.Drive.TorqLimitActivated',
    'WEBMON.Gripper.Drive.TorqLimitReached',
    'WEBMON.Gripper.Drive.CmdStart',
    'WEBMON.Gripper.Code',
    'WEBMON.Gripper.Mode',
    'WEBMON.Gripper.Step',
    'WEBMON.Gripper.AtSpeed',
    'WEBMON.Gripper.Contact',
    'WEBMON.Gripper.GripOk',
    'WEBMON.Gripper.ItemPresent',
    'WEBMON.Gripper.Obstacle',
    'WEBMON.Axis[3].Position',
    'WEBMON.Proc.Step.Now',
  ],
  divider: '1',
  flushMs: '20',
}

export const TRACE_PRESETS: readonly TracePreset[] = [GRIPPER_PRESET]

export function tracePreset(id: string | null | undefined): TracePreset | null {
  return TRACE_PRESETS.find((p) => p.id === id) ?? null
}

export interface AppliedPreset {
  channels: string[]
  /** 카탈로그(계약)에 없어 뺀 경로 — 옛 레이아웃이면 여기 남는다. */
  missing: string[]
}

/**
 * 프리셋을 카탈로그에 맞춰 고른다. `available` 이 `null` 이면(카탈로그를 못 받음) 거르지 않고 그대로 —
 * 시작할 때 백엔드가 경로마다 사유를 말해 준다. 상한(32)을 넘는 프리셋은 앞에서부터 자른다.
 */
export function applyPreset(preset: TracePreset, available: readonly string[] | null, max = CHAN_MAX): AppliedPreset {
  const set = available ? new Set(available) : null
  const channels: string[] = []
  const missing: string[] = []
  for (const p of preset.channels) {
    if (set && !set.has(p)) {
      missing.push(p)
      continue
    }
    if (channels.length < max && !channels.includes(p)) channels.push(p)
  }
  return { channels, missing }
}
