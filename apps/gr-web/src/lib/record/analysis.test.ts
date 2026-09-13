import { describe, expect, it } from 'vitest'
import { alarmCodes, bitEdges, dirCount, profile, replay, segments, stallEpisodes, taskSpec, timeSeries, type RecSample } from './analysis'

const ID = 406
const OD = 632
const G = 376

/** 하강 샘플 : Z 를 1 mm 씩 내리며 거리 d(z) 를 L 방향에 넣는다. */
function descent(dist: (z: number) => number, z0: number, z1: number, extra: Partial<RecSample> = {}): RecSample[] {
  const out: RecSample[] = []
  let t = 0
  for (let z = z0; z >= z1; z--) {
    const d = dist(z)
    out.push({
      t,
      Axis: [
        { Position: 0, Target: 0, Speed: 0, Torque: 0, Running: false, StandStill: true },
        { Position: 0, Target: 0, Speed: 0, Torque: 0, Running: false, StandStill: true },
        { Position: z, Target: z1, Speed: -20, Torque: 0, Running: true, StandStill: false },
        { Position: G, Target: G, Speed: 0, Torque: 0, Running: false, StandStill: true },
      ],
      Gripper: { ItemDetect: false, GID: [d, d, d, 800], FLD: 0 },
      Task: { TaskType: 68, Position: [0, 0, z1, G], Item: { Code: 11, InnerDiameter: ID, OuterDiameter: OD, Height: 200 }, Cell: { Id: 5, Position: [0, 0, 1500] } },
      ...extra,
    })
    t += 50
  }
  return out
}

/** 타이어 모델 (셀 바닥 1500, 높이 200) : 위 200 초과 = 못 봄, 180~200 비드, 20~180 내부 공간, 0~20 아래 비드. */
function tire(z: number): number {
  const r = z - 1500
  if (r > 200) return 800
  const dia = r >= 180 ? ID + Math.abs(r - 190) : r <= 20 ? ID + Math.abs(r - 10) : OD - 40
  return (dia - G) / 2
}

describe('profile / dirCount', () => {
  it('클램프·0 거리는 빼고 본 지름을 계산한다', () => {
    const s = descent(tire, 1720, 1690)
    const p = profile(s, 0)
    expect(p.every((q) => q.dist < 790)).toBe(true)
    expect(p[0]?.z).toBe(1700)
    expect(p[0]?.dia).toBeCloseTo(ID + 10, 5)
    expect(dirCount(s)).toBe(3)
  })
})

describe('replay', () => {
  it('하강 : 마지막 비드 = 비드 아래 끝, 경계 = 밖→안 두 번째 샘플', () => {
    const s = descent(tire, 1720, 1600)
    const r = replay(s, 0, { id: ID, od: OD })
    expect(r.minDist).toBeCloseTo((ID - G) / 2, 5)
    expect(r.zAtMin).toBe(1690)
    expect(r.lastBeadZ).toBe(1680)
    expect(r.beadBottom).toBe(1680)
    expect(r.edgeZ).toBe(1699)
  })
  it('규격 내경이 실제보다 30 mm 작아도 실제 비드를 따라간다', () => {
    const s = descent(tire, 1720, 1600)
    expect(replay(s, 0, { id: ID - 30, od: OD }).lastBeadZ).toBe(1680)
  })
  it('중간값 게이트 : 여유를 크게 줘도 내부 공간은 비드가 아니다', () => {
    const s = descent(tire, 1720, 1600)
    expect(replay(s, 0, { id: ID, od: OD }, { band: 500 }).lastBeadZ).toBe(1680)
  })
})

describe('segments / taskSpec', () => {
  it('측정 플래그가 켜진 연속 구간을 나눈다', () => {
    const s = descent(tire, 1710, 1700)
    s.forEach((x, i) => (x.Measure = { Item: { InBusy: i >= 2 && i <= 5 }, LastBead: { Enable: false, Busy: i === 8, Done: false, LastBidPos: 0, PickZTarget: 0 } }))
    const seg = segments(s)
    expect(seg.map((g) => [g.kind, g.i0, g.i1])).toEqual([
      ['in', 2, 5],
      ['pick', 8, 8],
    ])
    expect(taskSpec(s)).toMatchObject({ code: 11, id: ID, od: OD, cellZ: 1500 })
  })
})

describe('grip analysis', () => {
  const base = descent(() => 20, 1700, 1690)
  const withState = base.map((s, i) => ({
    ...s,
    Gripper: {
      ItemDetect: i >= 6,
      GID: [20, 20, 20, 800],
      FLD: 0,
      State: { Commanded: true, Stopped: i >= 3, AtCommand: false, TorqueReached: i >= 4, TorqueStop: i >= 4, Stall: i >= 3 && i <= 8, StallNoTorque: i === 3, StallTime: i >= 3 ? (i - 3) * 0.05 : 0 },
    },
    Alarm: { Fault: false, Warn: false, FaultCode: i === 8 ? [4025] : [0], WarnCode: [0] },
  }))
  it('비트 변화', () => {
    const e = bitEdges(withState, ['TorqueReached', 'Stall'])
    expect(e.map((x) => [x.bit, x.on, x.t])).toEqual([
      ['Stall', true, 150],
      ['TorqueReached', true, 200],
      ['Stall', false, 450],
    ])
  })
  it('멈춤 구간과 알람', () => {
    const ep = stallEpisodes(withState)
    expect(ep).toHaveLength(1)
    expect(ep[0]).toMatchObject({ t0: 150, t1: 400, torqueReached: true, itemDetect: true, alarms: [4025] })
    expect(ep[0]?.maxStallTime).toBeCloseTo(0.25, 5)
    expect(alarmCodes(withState)).toEqual([4025])
  })
  it('시간 점열은 초 단위', () => {
    expect(timeSeries(withState, (s) => s.Axis[2]?.Position).slice(0, 2)).toEqual([
      { x: 0, y: 1700 },
      { x: 0.05, y: 1699 },
    ])
  })
})
