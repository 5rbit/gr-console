import { describe, expect, it } from 'vitest'
import {
  feedIndicator,
  layoutSummary,
  mismatchText,
  modeIndicator,
  plcConnection,
  plcLayout,
  robotGate,
  TONE_LEGEND,
  toneStatus,
} from './indicators'
import type { PlcStatus, Robot } from './types'

function plc(over: Partial<PlcStatus> = {}): PlcStatus {
  return {
    id: 'gr2_s7',
    name: 'GR2',
    role: 'gr',
    label: 'GR2 S7',
    kind: 's7',
    endpoint: '192.168.1.102:102',
    connected: true,
    rtt_ms: 4.4,
    last_ok_at: '2026-09-21T10:00:00',
    last_error: null,
    layout: { ok: true, detail: null, checked_at: null, mismatches: [] },
    ...over,
  }
}

const FAIL_LAYOUT: PlcStatus['layout'] = {
  ok: false,
  detail: 'x',
  checked_at: null,
  mismatches: [
    { db: 'WEBMON', kind: 'signature', expected: '812 B / 16#1', actual: 'LayoutSig 16#2, expected 16#1' },
    { db: 'LASERDIAG', kind: 'size', expected: '5188 B', actual: 'size 5242 B, expected 5188 B' },
  ],
}

function robot(over: Partial<Robot> = {}): Robot {
  return {
    id: 1,
    name: 'GR1',
    plc: 'GR1',
    opcua_root: 'GR[1].CMD',
    dst: 4001,
    default: true,
    cmd_ready: true,
    cmd_error: null,
    plc_connected: true,
    layout_ok: true,
    gate: { can_submit: true, reasons: [] },
    active_tasks: 0,
    ...over,
  }
}

describe('toneStatus', () => {
  it('톤 넷 → 상태 토큰(degraded 는 빨강이 아니다)', () => {
    expect(toneStatus('ok')).toBe('ok')
    expect(toneStatus('degraded')).toBe('warn')
    expect(toneStatus('danger')).toBe('fault')
    expect(toneStatus('idle')).toBe('neutral')
  })
  it('범례가 톤 넷을 모두 말한다', () => {
    expect(TONE_LEGEND.map((l) => l.tone)).toEqual(['ok', 'degraded', 'danger', 'idle'])
  })
})

describe('plcConnection', () => {
  it('S7 연결 → 연결 + RTT', () => {
    const i = plcConnection(plc())
    expect(i).toMatchObject({ label: '연결 4ms', tone: 'ok' })
    expect(i.tooltip).toContain('192.168.1.102:102')
  })
  it('S7 끊김(오류) → danger, 오류가 툴팁', () => {
    const i = plcConnection(plc({ connected: false, last_error: 'timeout' }))
    expect(i).toMatchObject({ label: '끊김', tone: 'danger' })
    expect(i.tooltip).toContain('timeout')
  })
  it('한 번도 붙지 않음 → 연결 중(idle)', () => {
    expect(plcConnection(plc({ connected: false, last_ok_at: null }))).toMatchObject({
      label: '연결 중',
      tone: 'idle',
    })
  })
  it('OPC UA → 준비 / 연결 중 / 끊김', () => {
    const o = { kind: 'opcua' as const, rtt_ms: null, name: undefined, label: 'GRM OPC UA → GR1' }
    expect(plcConnection(plc(o))).toMatchObject({ label: '준비', tone: 'ok' })
    expect(plcConnection(plc({ ...o, connected: false, last_ok_at: null }))).toMatchObject({
      label: '연결 중',
      tone: 'idle',
    })
    expect(
      plcConnection(plc({ ...o, connected: false, last_error: 'BadTimeout' })),
    ).toMatchObject({ label: '끊김', tone: 'danger', tooltip: expect.stringContaining('BadTimeout') })
  })
})

describe('plcLayout', () => {
  it('OK 는 숨긴다', () => {
    expect(plcLayout(plc())).toMatchObject({ hidden: true, tone: 'ok' })
  })
  it('불일치 → degraded + 짧은 사유', () => {
    const i = plcLayout(plc({ layout: FAIL_LAYOUT }))
    expect(i).toMatchObject({ label: '레이아웃 불일치', tone: 'degraded', hidden: false })
    expect(i.tooltip).toBe('WEBMON 서명 · LASERDIAG 5242≠5188')
  })
  it('미검사·OPC UA 는 숨긴다', () => {
    expect(plcLayout(plc({ layout: { ...FAIL_LAYOUT, ok: null } })).hidden).toBe(true)
    expect(plcLayout(plc({ kind: 'opcua' })).hidden).toBe(true)
  })
  it('불일치 종류별 문구', () => {
    expect(mismatchText({ db: 'PARA', kind: 'missing', expected: '', actual: '' })).toBe('PARA 없음')
    expect(
      mismatchText({
        db: 'OPCUA',
        kind: 'semantic',
        expected: '4786 B',
        actual: 'STAT.ComponentID = 4001, expected 99',
      }),
    ).toBe('OPCUA STAT.ComponentID 4001≠99')
  })
})

describe('layoutSummary', () => {
  it('목록 전 → 그리지 않음', () => {
    expect(layoutSummary([], false)).toBeNull()
  })
  it('불일치 PLC 이름을 라벨에', () => {
    const i = layoutSummary(
      [plc({ id: 'grm_s7', name: 'GR1', layout: FAIL_LAYOUT }), plc(), plc({ kind: 'opcua', name: undefined })],
      true,
    )
    expect(i).toMatchObject({ label: '레이아웃 불일치 GR1', tone: 'degraded' })
    expect(i?.tooltip).toBe('GR1: WEBMON 서명 · LASERDIAG 5242≠5188')
  })
  it('전부 OK / 일부 미검사', () => {
    expect(layoutSummary([plc()], true)).toMatchObject({ label: '레이아웃 OK', tone: 'ok' })
    expect(
      layoutSummary([plc(), plc({ name: 'GRM', layout: { ...FAIL_LAYOUT, ok: null } })], true),
    ).toMatchObject({ label: '레이아웃 미검사', tone: 'idle', tooltip: '미검사: GRM' })
  })
})

describe('robotGate', () => {
  it('열림 → 제출 가능', () => {
    expect(robotGate(robot())).toMatchObject({ label: '제출 가능', tone: 'ok' })
  })
  it('닫힘 → 제출 불가(degraded) + 사유 전부', () => {
    const i = robotGate(
      robot({
        gate: {
          can_submit: false,
          reasons: ['GR1: Task.Status.Accept = FALSE', 'GR1: AUTO 모드가 아님'],
        },
      }),
    )
    expect(i).toMatchObject({ label: '제출 불가', tone: 'degraded' })
    expect(i.tooltip).toBe('GR1: Task.Status.Accept = FALSE · GR1: AUTO 모드가 아님')
  })
  it('PLC 끊김으로 닫힘 → danger', () => {
    const i = robotGate(
      robot({ plc_connected: false, gate: { can_submit: false, reasons: ['GR1: GR1 S7 연결 없음'] } }),
    )
    expect(i.tone).toBe('danger')
  })
  it('OPC UA 명령 경로 오류로 닫힘 → danger, 사유가 없으면 오류를 툴팁에', () => {
    const i = robotGate(robot({ cmd_ready: false, cmd_error: 'BadTimeout', gate: { can_submit: false, reasons: [] } }))
    expect(i).toMatchObject({ tone: 'danger', tooltip: 'BadTimeout' })
  })
})

describe('modeIndicator · feedIndicator', () => {
  it('모드', () => {
    expect(modeIndicator(null)).toBeNull()
    expect(modeIndicator('AUTO')).toMatchObject({ label: 'AUTO', tone: 'ok' })
    expect(modeIndicator('MANUAL')?.tone).toBe('idle')
    expect(modeIndicator('FAULT')?.tone).toBe('danger')
  })
  it('스트림', () => {
    expect(feedIndicator('Task', true, null)).toMatchObject({ label: 'Task 연결', tone: 'ok' })
    expect(feedIndicator('Task', false, 'EOF')).toMatchObject({ label: 'Task 끊김', tone: 'danger' })
    expect(feedIndicator('Task', false, null)).toMatchObject({ label: 'Task 연결 중', tone: 'idle' })
  })
})
