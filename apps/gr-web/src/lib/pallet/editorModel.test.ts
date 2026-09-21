import { describe, expect, it } from 'vitest'
import {
  addSlot,
  cycleDir,
  diffPattern,
  dirPickerGrid,
  dragToU,
  duplicatePattern,
  isDirty,
  liveIssues,
  mmToNorm,
  newPattern,
  nextPatternNumber,
  nextSlotName,
  normMinDistance,
  normToMm,
  normalizeFlowId,
  odGaps,
  odOverlaps,
  patternBody,
  pitchOf,
  removeSlot,
  reorderSeq,
  rescaleToUnit,
  scaleOf,
  seqByClicks,
  slotsBySeq,
  snapU,
  toDraft,
  transformSlots,
  validatePattern,
  viewHalfExtent,
  type DraftPattern,
  type DraftSlot,
} from './editorModel'
import { MACHINE_AXES, applyDir, dirGrid, type ScreenAxes } from './model'

// HP_IN P3 (spec_r4.json, machine axes)
const P3: DraftPattern = {
  pattern: 3,
  od_min: 756,
  od_max: 813,
  note: '',
  slots: [
    { slot: 'C#1', seq: 1, u: [-0.4611, -0.4077], drag_dir: 0 },
    { slot: 'C#2', seq: 2, u: [0.5389, -0.4077], drag_dir: 3 },
    { slot: 'C#3', seq: 3, u: [0.0389, 0.4583], drag_dir: 1 },
  ],
}
const P4: DraftPattern = {
  pattern: 4,
  od_min: 663,
  od_max: 755,
  note: '',
  slots: [
    { slot: 'C#1', seq: 1, u: [-0.5, -0.5], drag_dir: 0 },
    { slot: 'C#2', seq: 2, u: [0.5, -0.5], drag_dir: 2 },
    { slot: 'C#3', seq: 3, u: [-0.5, 0.5], drag_dir: 5 },
    { slot: 'C#4', seq: 4, u: [0.5, 0.5], drag_dir: 3 },
  ],
}
const SLIDE2: ScreenAxes = { right: 'X-', down: 'Y-' }
const copy = (p: DraftPattern) => toDraft(p)
const names = (s: DraftSlot[]) => slotsBySeq(s).map((x) => x.slot)

describe('flow id', () => {
  it('uppercases and checks the pattern', () => {
    expect(normalizeFlowId(' hp_in_site ')).toEqual({ id: 'HP_IN_SITE', error: null })
    for (const bad of ['A', 'HP IN', 'HP-IN', 'A'.repeat(33), '한글']) expect(normalizeFlowId(bad).error).not.toBeNull()
  })
})

describe('units and dragging', () => {
  it('converts norm <-> mm with the preview pitch', () => {
    const pitch = pitchOf(780, 50)
    expect(pitch).toBe(830)
    expect(normToMm(0.5389, pitch)).toBe(447.3)
    expect(mmToNorm(447.3, pitch)).toBe(0.5389)
    expect(mmToNorm(100, 0)).toBe(0)
    expect(pitchOf(-100, 50)).toBe(0)
  })

  it('snaps to 10 mm or 0.05 norm', () => {
    expect(snapU([0.5389, -0.4077], 'norm005', 830)).toEqual([0.55, -0.4])
    const mm = snapU([0.5389, -0.4077], 'mm10', 830).map((v) => v * 830)
    expect(Math.round(mm[0] * 1000) / 1000).toBeCloseTo(450, 1)
    expect(Math.round(mm[1])).toBe(-340)
    expect(snapU([0.123456, 0], 'off', 830)).toEqual([0.1235, 0])
  })

  it('maps a screen drag back to machine axes for each view orientation', () => {
    // machine view: right = X+, down = Y-  → dragging 83 mm right and 83 mm down = X +0.1, Y -0.1
    expect(dragToU([0, 0], [10, 10], [93, 93], MACHINE_AXES, 830, 'off')).toEqual([0.1, -0.1])
    // slide 2 view: right = X-, down = Y-
    expect(dragToU([0.5, 0.5], [0, 0], [83, 0], SLIDE2, 830, 'off')).toEqual([0.4, 0.5])
    expect(dragToU([0.5, 0.5], [0, 0], [40, 0], SLIDE2, 830, 'norm005')).toEqual([0.45, 0.5])
    expect(dragToU([0.5, 0.5], [0, 0], [40, 0], SLIDE2, 0, 'off')).toEqual([0.5, 0.5])
  })
})

describe('seq editing', () => {
  it('reorders by table drag', () => {
    const r = reorderSeq(P4.slots, 4, 1)
    expect(names(r)).toEqual(['C#4', 'C#1', 'C#2', 'C#3'])
    expect(slotsBySeq(r).map((s) => s.seq)).toEqual([1, 2, 3, 4])
    expect(names(reorderSeq(P4.slots, 1, 9))).toEqual(['C#2', 'C#3', 'C#4', 'C#1'])
    expect(names(reorderSeq(P4.slots, 7, 1))).toEqual(['C#1', 'C#2', 'C#3', 'C#4'])
    expect(P4.slots[0].seq).toBe(1) // not mutated
  })

  it('renumbers by click order, keeping the rest in order', () => {
    expect(names(seqByClicks(P4.slots, ['C#3', 'C#1']))).toEqual(['C#3', 'C#1', 'C#2', 'C#4'])
    expect(names(seqByClicks(P4.slots, ['C#4', 'C#4', 'nope', 'C#2', 'C#1', 'C#3']))).toEqual(['C#4', 'C#2', 'C#1', 'C#3'])
  })

  it('adds a free slot and removes with renumbering', () => {
    expect(nextSlotName([{ slot: 'C#2', seq: 1, u: [0, 0], drag_dir: 0 }])).toBe('C#1')
    const added = addSlot(P4.slots)
    const n = added[4]
    expect([n.slot, n.seq, n.drag_dir]).toEqual(['C#5', 5, 0])
    expect(normMinDistance(added)).toBeGreaterThanOrEqual(0.5) // grid 0.5 but >= 1 from all others
    for (const s of P4.slots) expect(Math.hypot(s.u[0] - n.u[0], s.u[1] - n.u[1])).toBeGreaterThanOrEqual(1 - 1e-9)
    expect(addSlot([])[0]).toEqual({ slot: 'C#1', seq: 1, u: [0, 0], drag_dir: 0 })
    const removed = removeSlot(P4.slots, 'C#2')
    expect(removed.map((s) => [s.slot, s.seq])).toEqual([['C#1', 1], ['C#3', 2], ['C#4', 3]])
  })
})

describe('transforms and scale', () => {
  it('rotates offsets and drag dirs together', () => {
    const r = transformSlots(P4.slots, { rotation: 90, mirror_x: false, mirror_y: false })
    const c2 = r.find((s) => s.slot === 'C#2')!
    expect(c2.u).toEqual([0.5, 0.5])
    expect(c2.drag_dir).toBe(5) // X+ -> Y+
    const m = transformSlots(P3.slots, { rotation: 0, mirror_x: true, mirror_y: false })
    for (const [a, b] of P3.slots.map((s, i) => [s, m[i]] as const)) {
      expect(b.u[0]).toBeCloseTo(-a.u[0], 6)
      expect(b.drag_dir).toBe(applyDir({ rotation: 0, mirror_x: true, mirror_y: false }, a.drag_dir))
    }
    expect(m.find((s) => s.slot === 'C#2')!.drag_dir).toBe(8)
    // normalized min distance is invariant
    expect(normMinDistance(m)).toBeCloseTo(normMinDistance(P3.slots)!, 3)
  })

  it('reports and removes the generator scale', () => {
    const half = P4.slots.map((s) => ({ ...s, u: [s.u[0] * 0.5, s.u[1] * 0.5] as [number, number] }))
    expect(normMinDistance(half)).toBeCloseTo(0.5, 6)
    expect(scaleOf(half)).toBeCloseTo(2, 6)
    expect(normMinDistance(rescaleToUnit(half))).toBeCloseTo(1, 4)
    expect(scaleOf([P4.slots[0]])).toBe(1)
    expect(normMinDistance([P4.slots[0]])).toBeNull()
  })

  it('direction picker has the legend layout with 0 in the middle', () => {
    const g = dirPickerGrid(SLIDE2)
    expect(g[1][1]).toBe(0)
    const legend = dirGrid(SLIDE2)
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) if (!(i === 1 && j === 1)) expect(g[i][j]).toBe(legend[i][j])
    expect(g.flat().sort()).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8])
    expect([cycleDir(0), cycleDir(7), cycleDir(8), cycleDir(-1)]).toEqual([1, 8, 0, 0])
  })
})

describe('validation mirrors the backend', () => {
  it('accepts the spec pattern', () => {
    const v = validatePattern(copy(P3), 'in', [P4])
    expect(v.errors).toEqual([])
    expect(v.warnings).toEqual([])
  })

  it('collects errors', () => {
    const bad = copy(P3)
    bad.slots[1].seq = 1
    bad.slots[2].slot = 'C#1'
    bad.slots[0].drag_dir = 9
    const v = validatePattern(bad, 'in', [])
    expect(v.errors.some((e) => e.includes('Seq 는 1..3'))).toBe(true)
    expect(v.errors.some((e) => e.includes('중복'))).toBe(true)
    expect(v.errors.some((e) => e.includes('DragDir 9'))).toBe(true)
    expect(validatePattern({ ...copy(P3), od_min: 820, od_max: 810 }, 'in', []).errors[0]).toContain('OdMin')
    expect(validatePattern({ ...copy(P3), slots: [] }, 'in', []).errors[0]).toContain('슬롯 수 0')
    const many = { ...copy(P3), slots: Array.from({ length: 21 }, (_, i) => ({ slot: `C#${i + 1}`, seq: i + 1, u: [i * 1.1, 0] as [number, number], drag_dir: 0 })) }
    expect(validatePattern(many, 'in', []).errors.some((e) => e.includes('슬롯 수 21'))).toBe(true)
    const same = copy(P3)
    same.slots[1].u = [...same.slots[0].u]
    expect(validatePattern(same, 'in', []).errors.some((e) => e.includes('같은 자리'))).toBe(true)
    expect(validatePattern({ ...copy(P3), pattern: 4 }, 'in', [P4]).errors.some((e) => e.includes('같은 번호'))).toBe(true)
  })

  it('checks OD overlaps and reports gaps', () => {
    const over = validatePattern({ ...copy(P3), od_min: 750 }, 'in', [P4])
    expect(over.errors.some((e) => e.includes('겹칩니다'))).toBe(true)
    expect(odOverlaps([P3, P4])).toEqual([])
    expect(odGaps([P3, P4])).toEqual([]) // 755 / 756 touch
    const gap = validatePattern({ ...copy(P3), od_min: 800 }, 'in', [P4])
    expect(gap.errors).toEqual([])
    expect(gap.warnings.some((w) => w.includes('no pattern for OD 755~800'))).toBe(true)
  })

  it('warns on min distance and on the drag rule', () => {
    const shrunk = copy(P4)
    shrunk.slots = shrunk.slots.map((s) => ({ ...s, u: [s.u[0] * 0.9, s.u[1] * 0.9] }))
    shrunk.slots[0].drag_dir = 2
    const v = validatePattern(shrunk, 'in', [])
    expect(v.errors).toEqual([])
    expect(v.warnings.some((w) => w.includes('MinDistance 0.9'))).toBe(true)
    expect(v.warnings.some((w) => w.includes('첫 작업'))).toBe(true)
    expect(validatePattern(copy(P4), 'out', []).warnings.some((w) => w.includes('마지막 작업'))).toBe(true)
  })
})

describe('live issues', () => {
  it('flags overlaps in edit coordinates and overhang of the generated result', () => {
    expect(liveIssues(P4.slots, 700, 50, 1600)).toEqual([])
    const tight = copy(P4)
    tight.slots[1].u = [-0.1, -0.5] // 0.4 * 750 = 300 < 700
    const issues = liveIssues(tight.slots, 700, 50, 1600)
    expect(issues.filter((i) => i.kind === 'overlap').map((i) => i.slot).sort()).toEqual(['C#1', 'C#2'])
    // generator rescales by 1/0.4 → corners at 0.5*750*2.5 = 937.5 + 350 > 800
    expect(issues.some((i) => i.kind === 'overhang' && i.text.includes('X'))).toBe(true)
    expect(liveIssues(P4.slots, 0, 50, 1600)).toEqual([])
    expect(liveIssues(P3.slots, 937, 50, 1600).some((i) => i.kind === 'overhang')).toBe(true)
  })
})

describe('diff and bodies', () => {
  it('detects field and slot changes', () => {
    expect(diffPattern(copy(P3), P3)).toEqual({ status: 'same', fields: [], slots: P3.slots.map((s) => ({ slot: s.slot, status: 'same', fields: [] })) })
    const c = copy(P3)
    c.slots[1].drag_dir = 5
    c.slots[2].u = [0.1, 0.4583]
    c.od_max = 812
    c.note = 'x'
    c.slots.push({ slot: 'C#9', seq: 4, u: [2, 2], drag_dir: 0 })
    const d = diffPattern(c, P3)
    expect(d.status).toBe('changed')
    expect(d.fields).toEqual(['od_max', 'note'])
    expect(d.slots).toEqual([
      { slot: 'C#1', status: 'same', fields: [] },
      { slot: 'C#2', status: 'changed', fields: ['drag_dir'] },
      { slot: 'C#3', status: 'changed', fields: ['u'] },
      { slot: 'C#9', status: 'added', fields: [] },
    ])
    expect(diffPattern(null, P3).status).toBe('removed')
    expect(diffPattern(P3, null).status).toBe('added')
    const tiny = copy(P3)
    tiny.slots[0].u = [tiny.slots[0].u[0] + 4e-5, tiny.slots[0].u[1]]
    expect(isDirty(tiny, P3)).toBe(false)
    expect(isDirty({ ...copy(P3), pattern: 30 }, P3)).toBe(true)
  })

  it('builds bodies and new patterns', () => {
    const b = patternBody({ ...copy(P3), note: ' memo ', slots: [{ slot: ' C#1 ', seq: 1, u: [0.123456, 0], drag_dir: 0 }] })
    expect(b).toEqual({ pattern: 3, od_min: 756, od_max: 813, note: 'memo', slots: [{ slot: 'C#1', seq: 1, u: [0.1235, 0], drag_dir: 0 }] })
    expect(nextPatternNumber([2, 3, 4], 4)).toBe(1)
    expect(nextPatternNumber([2, 3, 4], 5)).toBe(5)
    const np = newPattern([P3, P4])
    expect([np.pattern, np.od_min, np.od_max, np.slots.length]).toEqual([1, 814, 864, 1])
    expect(validatePattern(np, 'in', [P3, P4]).errors).toEqual([])
    const dup = duplicatePattern(P4, [P3, P4])
    expect([dup.pattern, dup.od_min, dup.slots.length]).toEqual([1, 814, 4])
    expect(duplicatePattern(P4, [P3]).pattern).toBe(4)
    dup.slots[0].drag_dir = 7
    expect(P4.slots[0].drag_dir).toBe(0) // deep copy
    expect(newPattern([]).od_min).toBe(0)
    expect(viewHalfExtent(P4.slots, 750, 700, 1600)).toBe(800)
    expect(viewHalfExtent([{ slot: 'C#1', seq: 1, u: [2, 0], drag_dir: 0 }], 750, 700, 1600)).toBe(1850)
  })
})
