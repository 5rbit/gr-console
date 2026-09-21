import { describe, expect, it } from 'vitest'
import { DIALOG_WIDTH, shouldConfirmDiscard } from './dialogModel'

describe('shouldConfirmDiscard', () => {
  it('제출은 값이 살아 나가므로 절대 되묻지 않는다', () => {
    expect(shouldConfirmDiscard(true, 'submit')).toBe(false)
    expect(shouldConfirmDiscard(false, 'submit')).toBe(false)
  })

  it('편집이 없으면 어느 경로로 닫아도 그냥 닫는다', () => {
    for (const intent of ['esc', 'backdrop', 'cancel'] as const) {
      expect(shouldConfirmDiscard(false, intent)).toBe(false)
    }
  })

  it('편집이 남아 있으면 Escape·백드롭·취소 셋 다 한 번 되묻는다', () => {
    for (const intent of ['esc', 'backdrop', 'cancel'] as const) {
      expect(shouldConfirmDiscard(true, intent)).toBe(true)
    }
  })
})

describe('DIALOG_WIDTH', () => {
  it('폭 단계는 넷뿐이고 모두 최대 폭 클래스다', () => {
    const keys = Object.keys(DIALOG_WIDTH)
    expect(keys).toEqual(['sm', 'md', 'lg', 'xl'])
    for (const v of Object.values(DIALOG_WIDTH)) expect(v.startsWith('max-w-')).toBe(true)
  })
})
