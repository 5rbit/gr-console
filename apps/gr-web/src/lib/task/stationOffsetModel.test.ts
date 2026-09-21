import { describe, expect, it } from 'vitest'
import {
  fmtAge,
  mm,
  odSource,
  odSourceHint,
  odSourceText,
  offsetHeadline,
  offsetTone,
  rotateLabel,
  signedMm,
  stationOffsetFields,
  xy,
} from './stationOffsetModel'
import type { StationOffsetAudit } from './types'

const audit = (patch: Partial<StationOffsetAudit> = {}): StationOffsetAudit => ({
  station_id: 2101,
  slot: 1,
  rotate_type: 1,
  od: 640,
  od_used: 640,
  od_source: 'tracking',
  now_tx: 35,
  now_ty: 0,
  tx_applied: 35,
  ty_applied: -320,
  base_xy: [20000, 1000],
  final_xy: [20035, 680],
  source: 'OPCUA',
  snapshot_at: '2026-09-14T10:00:00+09:00',
  age_ms: 420,
  mode: 'auto',
  warnings: [],
  blocked: null,
  gr2_expected_xy: [20000, 680],
  gr2_margin: 650,
  ...patch,
})

describe('stationOffsetModel', () => {
  it('formats millimetres', () => {
    expect(mm(12.345)).toBe('12.3')
    expect(mm(-0.01)).toBe('0.0')
    expect(mm(undefined)).toBe('')
    expect(signedMm(35)).toBe('+35.0')
    expect(signedMm(-320)).toBe('-320.0')
    expect(signedMm(0)).toBe('0.0')
    expect(xy([1, 2.25])).toBe('1.0, 2.3')
    expect(xy(null)).toBe('')
  })

  it('labels every rotate type', () => {
    expect(rotateLabel(1)).toContain('Y −OD/2')
    expect(rotateLabel(5)).toContain('Y −OD/2')
    expect(rotateLabel(2)).toContain('Y +OD/2')
    expect(rotateLabel(6)).toContain('Y +OD/2')
    expect(rotateLabel(3)).toContain('X +OD/2')
    expect(rotateLabel(7)).toContain('X +OD/2')
    expect(rotateLabel(4)).toContain('X −OD/2')
    expect(rotateLabel(8)).toContain('X −OD/2')
    expect(rotateLabel(0)).toContain('그대로')
  })

  it('formats snapshot age', () => {
    expect(fmtAge(420)).toBe('420 ms')
    expect(fmtAge(9000)).toBe('9.0 s')
    expect(fmtAge(180_000)).toBe('3 분')
    expect(fmtAge(-5)).toBe('0 ms')
    expect(fmtAge(null)).toBe('')
  })

  it('tone and headline follow blocked > warnings > mode', () => {
    expect(offsetTone(audit())).toBe('ok')
    expect(offsetHeadline(audit())).toBe('TX +35.0 · TY -320.0')
    expect(offsetTone(audit({ warnings: ['x'] }))).toBe('warn')
    expect(offsetTone(audit({ mode: 'off', warnings: ['꺼짐'] }))).toBe('info')
    expect(offsetHeadline(audit({ mode: 'override' }))).toContain('직접 지정')
    const b = audit({ blocked: '트래킹 없음', warnings: ['x'] })
    expect(offsetTone(b)).toBe('fault')
    expect(offsetHeadline(b)).toBe('제출 거부')
  })

  it('reads the od source and its hint', () => {
    expect(odSource(audit())).toBe('tracking')
    expect(odSourceText(audit())).toBe('Tracking · 640.0')
    expect(odSourceHint(audit())).toBeUndefined()
    const fb = audit({ od: 0, od_used: 700, od_source: 'item_spec', tx_applied: 0, ty_applied: -350 })
    expect(odSourceText(fb)).toBe('ItemSpec · 700.0')
    expect(odSourceHint(fb)).toContain('등록 품목 OuterDiameter 700.0')
    expect(offsetHeadline(fb)).toBe('TX 0.0 · TY -350.0 · ItemSpec')
    const none = audit({ od: 0, od_used: 0, od_source: 'none', tx_applied: 0, ty_applied: 0 })
    expect(odSourceText(none)).toBe('')
    expect(odSourceHint(none)).toBeUndefined()
    // 예전 원장 기록(od_source 없음) — 측정 OD 유무로 읽는다
    expect(odSource(audit({ od_source: undefined }))).toBe('tracking')
    expect(odSource(audit({ od: 0, od_source: undefined }))).toBe('none')
  })

  it('builds field items', () => {
    const f = stationOffsetFields(audit())
    const by = (l: string) => f.find((x) => x.label === l)
    expect(by('Mode')?.status).toBeUndefined()
    expect(by('Station (Id · Slot)')?.value).toBe('2101 · 1')
    expect(by('OD')?.value).toBe('640.0')
    expect(by('Applied TX / TY')?.value).toBe('+35.0 / -320.0')
    expect(by('Applied TX / TY')?.status).toBeUndefined()
    expect(by('OdSource')?.value).toBe('Tracking · 640.0')
    expect(by('Base → Final XY')?.value).toBe('20000.0, 1000.0 → 20035.0, 680.0')
    expect(by('Snapshot')?.value).toBe('GRM OPCUA · 420 ms 전')
    expect(by('GR2 Expected XY')?.value).toBe('20000.0, 680.0 ± 650')
    // 스냅샷 없음·OD 0·범위 밖 슬롯
    const g = stationOffsetFields(
      audit({
        source: null,
        age_ms: null,
        od: 0,
        od_used: 0,
        od_source: 'none',
        slot: 0,
        gr2_expected_xy: null,
        blocked: 'x',
      }),
    )
    const gb = (l: string) => g.find((x) => x.label === l)
    expect(gb('Snapshot')?.value).toBeNull()
    expect(gb('OD')?.value).toBeNull()
    expect(gb('Station (Id · Slot)')?.value).toBe('2101 · 범위 밖')
    expect(gb('GR2 Expected XY')?.value).toBeNull()
    expect(gb('OdSource')?.value).toBeNull()
    expect(gb('OdSource')?.missing).toContain('쓸 외경 없음')
    expect(gb('Mode')?.status).toBe('fault')
    // 품목 스펙 폴백은 Applied·OdSource 에 경고색 + 한 줄 설명
    const s = stationOffsetFields(audit({ od: 0, od_used: 700, od_source: 'item_spec' }))
    const sb = (l: string) => s.find((x) => x.label === l)
    expect(sb('OdSource')?.value).toBe('ItemSpec · 700.0')
    expect(sb('OdSource')?.status).toBe('warn')
    expect(sb('Applied TX / TY')?.status).toBe('warn')
    expect(sb('Applied TX / TY')?.tooltip).toContain('등록 품목')
    expect(sb('OD')?.value).toBeNull()
  })
})
