import { describe, expect, it } from 'vitest'
import { bandItems } from './MeasureMonitorModel'
import type { MeasLogSnapshot, WebMon } from '../../lib/types'

/** 띠가 읽는 자리만 채운 WEBMON — 타입은 넓고 화면이 보는 것은 좁다. */
function wm(patch: Record<string, unknown> = {}): WebMon {
  const base = {
    Mode: 32,
    Stat: {
      Status: { Idle: true, Speed: 50 },
      StatusCode: { Main: 0, Sub: 0 },
      Task: { Status: {}, Now: { WorkId: 0, TaskId: 0 } },
    },
    Proc: { Step: { Now: 400, ElapseTime: 1.5 }, Msg: '' },
    Alarm: { Fault: false, Warn: false, FaultCode: [], WarnCode: [] },
    Axis: [{}, {}, { Position: 2500, Target: 2500 }, { Position: 300, Target: 300 }],
    Gripper: { ItemDetect: false },
    Measure: { Item: {}, Sku: {}, Floor: {}, LastBead: {} },
    MeasLog: { Total: 40, Count: 40 },
  }
  return { ...base, ...patch } as unknown as WebMon
}

const snap = { total: 40, count: 40, capacity: 200 } as unknown as MeasLogSnapshot

const find = (items: ReturnType<typeof bandItems>, label: string) =>
  items.find((i) => i.label === label)

describe('bandItems', () => {
  it('PLC 상태가 없으면 짧은 줄만 세운다 — 빈 값 아홉 개를 세우지 않는다', () => {
    const items = bandItems(null, snap)
    expect(items).toHaveLength(3)
    expect(find(items, '상태')?.value).toBe('PLC 상태 없음')
    expect(find(items, 'MeasLog')?.value).toBe('40')
  })

  it('정상 대기는 색을 거의 쓰지 않는다 — 알람 없음에 색이 붙으면 붉은 하나가 사라진다', () => {
    const items = bandItems(wm(), snap)
    expect(find(items, '알람')?.value).toBe('없음')
    expect(find(items, '알람')?.tone).toBeUndefined()
    expect(find(items, '상태')?.value).toBe('IDLE')
    expect(find(items, '모드')?.value).toBe('AUTO')
  })

  it('Fault 코드가 있으면 코드를 그대로 보이고 fault 색을 받는다', () => {
    const items = bandItems(wm({ Alarm: { Fault: true, FaultCode: [4025, 0, 3128] } }), snap)
    expect(find(items, '알람')?.value).toBe('4025 · 3128')
    expect(find(items, '알람')?.tone).toBe('fault')
  })

  it('Warn 은 코드가 없어도 비트만으로 말한다', () => {
    const items = bandItems(wm({ Alarm: { Warn: true, WarnCode: [] } }), snap)
    expect(find(items, '알람')?.value).toBe('Warn')
    expect(find(items, '알람')?.tone).toBe('warn')
  })

  it('측정 단계는 도는 것 우선, 없으면 끝난 것을 말한다', () => {
    const busy = bandItems(wm({ Measure: { Item: { Busy: true }, Sku: { Done: true } } }), snap)
    expect(find(busy, '측정')?.value).toBe('Item 측정중')
    const done = bandItems(wm({ Measure: { Sku: { Done: true }, Floor: { Done: true } } }), snap)
    expect(find(done, '측정')?.value).toBe('Sku · Floor 완료')
    expect(find(bandItems(wm(), snap), '측정')?.value).toBe('대기')
  })

  it('Z / G 는 한 항목에 붙고 목표는 꼬리표로 간다(열 둘을 쓰지 않는다)', () => {
    const items = bandItems(wm(), snap)
    expect(find(items, 'Z / G (mm)')?.value).toBe('2500 / 300.0')
    expect(find(items, 'Z / G (mm)')?.hint).toBe('→ 2500 / 300.0')
  })
})
