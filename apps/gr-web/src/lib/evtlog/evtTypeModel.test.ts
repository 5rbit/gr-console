import { describe, expect, it } from 'vitest'
import { EVT_TYPES, isErrorCode, parseTypes, rowText, typeTone } from './evtTypeModel'

describe('ErrorList types', () => {
  it('five types, alarm colors only on Alarm / Warn / Operator', () => {
    expect(EVT_TYPES.map((t) => t.id)).toEqual(['Alarm', 'Warn', 'Operator', 'Info', 'Task'])
    expect([typeTone('Alarm'), typeTone('Warn'), typeTone('Task'), typeTone(null)]).toEqual([
      'fault',
      'warn',
      'neutral',
      'neutral',
    ])
  })
  it('parses names case-insensitively and drops unknown ones and repeats', () => {
    expect(parseTypes(['alarm', ' TASK', 'loud', 'Alarm'])).toEqual(['Alarm', 'Task'])
    expect(parseTypes([])).toEqual([])
  })
  it('knows an ErrorList code', () => {
    expect(['F0101', 'w1101', 'O0105', 'I5101'].every(isErrorCode)).toBe(true)
    expect(['X0101', 'F101', '0101', 'GRIP_REQ'].some(isErrorCode)).toBe(false)
  })
  it('shows English only where the server sent it', () => {
    const r = {
      text: '작업 수락 (Buff) · Cell 101',
      text_en: 'Task - Accepted to Buffer · Cell 101',
    }
    expect(rowText(r, 'en')).toBe('Task - Accepted to Buffer · Cell 101')
    expect(rowText(r, 'ko')).toBe('작업 수락 (Buff) · Cell 101')
    expect(rowText({ text: 'Task 200→300' }, 'en')).toBe('Task 200→300')
  })
})
