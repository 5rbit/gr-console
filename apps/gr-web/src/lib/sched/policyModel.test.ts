import { describe, expect, it } from 'vitest'
import { DEFAULT_POLICY, type StationProfile, type StationProfileRow } from '../sched'
import {
  cellPickText,
  guessHint,
  isConflict,
  mergePolicy,
  migratePlanLines,
  pascal,
  policyDiff,
  profileDirty,
  profileDraftOf,
  profilePayload,
  samePick,
  validatePolicy,
  validateProfile,
  withPolicy,
  withRole,
  dropsHere,
} from './policyModel'

const prof = (p: Partial<StationProfile> = {}): StationProfile => ({
  station_id: 2101,
  role: 'pick',
  drop_mode: 'single',
  max_height_mm: 0,
  weight: 0,
  max_wait_s: 0,
  note: '',
  updated_at: '',
  ...p,
})

const row = (
  profile: StationProfile | null,
  guess: Partial<StationProfile> = {},
): StationProfileRow => ({
  id: 2101,
  conv_no: 1,
  profile,
  guess: prof(guess),
  guess_why: 'line end',
  pallet: false,
})

describe('policy', () => {
  it('merges missing policy and partial picks with defaults', () => {
    expect(mergePolicy(undefined)).toEqual(DEFAULT_POLICY)
    const m = mergePolicy({ outbound_auto: true, inbound_dest: { section: 2 } })
    expect(m.outbound_auto).toBe(true)
    expect(m.inbound_dest).toEqual({ order: 'nearest', section: 2 })
    expect(m.outbound_source).toEqual({ order: 'oldest' })
    expect(m.request_priority).toBe(1000)
  })

  it('withPolicy fills policy and rules', () => {
    const c = withPolicy({
      version: 3,
      auto: false,
      weights: { target: {}, item: {}, robot: {} },
      rules: [],
    })
    expect(c.policy).toEqual(DEFAULT_POLICY)
    expect(c.version).toBe(3)
  })

  it('diffs policy fields, treating empty pick fields as absent', () => {
    const base = mergePolicy(null)
    expect(policyDiff(base, { ...base })).toEqual([])
    expect(
      policyDiff(base, { ...base, inbound_dest: { ...base.inbound_dest, section: null } }),
    ).toEqual([])
    expect(
      policyDiff(base, { ...base, consolidate: true, outbound_source: { order: 'nearest' } }),
    ).toEqual(['outbound_source', 'consolidate'])
    expect(samePick({ same_item_first: false }, {})).toBe(true)
    expect(samePick({ near: 2101 }, {})).toBe(false)
  })

  it('validates numbers and ranges', () => {
    const p = mergePolicy(null)
    expect(validatePolicy(p)).toBeUndefined()
    expect(validatePolicy({ ...p, inbound_priority: NaN })).toMatch('InboundPriority')
    expect(validatePolicy({ ...p, consolidate_idle_s: -1 })).toMatch('ConsolidateIdleS')
    expect(validatePolicy({ ...p, inbound_dest: { row_min: 5, row_max: 2 } })).toMatch('RowMin')
  })

  it('formats labels and picks', () => {
    expect(pascal('inbound_near_drop')).toBe('InboundNearDrop')
    expect(cellPickText({ order: 'oldest' })).toBe('oldest')
    expect(cellPickText({ section: 2, row_min: 1, row_max: 3, col_max: 5, order: 'nearest' })).toBe(
      'Section 2 · Row 1..3 · Col ..5 · nearest',
    )
    expect(cellPickText({})).toBe('All')
  })

  it('detects 409 conflicts', () => {
    expect(isConflict(new Error('version mismatch (PUT /api/taskgen/config → 409)'))).toBe(true)
    expect(isConflict(new Error('bad (PUT /api/taskgen/config → 400)'))).toBe(false)
  })
})

describe('station profiles', () => {
  it('drafts from profile or blank', () => {
    expect(profileDraftOf(row(null)).role).toBeNull()
    expect(profileDraftOf(row(prof({ role: 'drop', max_height_mm: 1200 })))).toEqual({
      role: 'drop',
      drop_mode: 'single',
      max_height_mm: 1200,
      weight: 0,
      max_wait_s: 0,
      merge_into: null,
    })
    expect(profileDraftOf(row(prof({ role: 'pick', merge_into: 2101 }))).merge_into).toBe(2101)
  })

  it('dirty only when something changes', () => {
    const r = row(prof())
    const d = profileDraftOf(r)
    expect(profileDirty(r, d)).toBe(false)
    expect(profileDirty(r, { ...d, weight: 5 })).toBe(true)
    expect(profileDirty(r, { ...d, merge_into: 2101 })).toBe(true)
    const empty = row(null)
    expect(profileDirty(empty, { ...profileDraftOf(empty), weight: 3 })).toBe(false)
    expect(profileDirty(empty, { ...profileDraftOf(empty), role: 'both' })).toBe(true)
  })

  it('first role takes the guess, later role changes keep fields', () => {
    const r = row(null, { role: 'drop', drop_mode: 'stack', max_height_mm: 1400 })
    expect(withRole(r, profileDraftOf(r), 'drop')).toMatchObject({
      role: 'drop',
      drop_mode: 'stack',
      max_height_mm: 1400,
    })
    const saved = row(prof({ drop_mode: 'single', weight: 7 }), { drop_mode: 'pallet' })
    expect(withRole(saved, profileDraftOf(saved), 'both')).toMatchObject({
      role: 'both',
      drop_mode: 'single',
      weight: 7,
    })
    expect(dropsHere('both')).toBe(true)
    expect(dropsHere('pick')).toBe(false)
    expect(dropsHere(null)).toBe(false)
  })

  it('validates and builds payloads', () => {
    const d = profileDraftOf(row(prof()))
    expect(validateProfile(d)).toBeUndefined()
    expect(validateProfile({ ...d, max_height_mm: -1 })).toMatch('MaxHeight')
    expect(validateProfile({ ...d, role: null, max_height_mm: -1 })).toBeUndefined()
    expect(profilePayload({ ...d, role: null })).toEqual({ role: null })
    expect(profilePayload(d)).toMatchObject({ role: 'pick', drop_mode: 'single', note: '' })
  })

  it('shows guess only when profile missing or different', () => {
    expect(guessHint(row(null, { role: 'drop', drop_mode: 'pallet' }))?.text).toBe(
      '추정: drop/pallet',
    )
    expect(guessHint(row(prof(), {}))).toBeNull()
    expect(guessHint(row(prof({ role: 'both' }), {}))?.title).toContain('line end')
  })

  it('summarizes a migrate plan', () => {
    const l = migratePlanLines({
      profiles: [prof({ role: 'drop', drop_mode: 'stack', max_height_mm: 1500 })],
      disabled_rules: ['ship-2101'],
    })
    expect(l.profiles).toEqual(['Station 2101: drop/stack · MaxHeight 1500 mm'])
    expect(l.rules).toEqual(['ship-2101'])
    expect(migratePlanLines({ profiles: null, disabled_rules: undefined })).toEqual({
      profiles: [],
      rules: [],
    })
  })
})
