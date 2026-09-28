import { describe, expect, it } from 'vitest'
import { loadFailure, loadFailureText } from './api'

describe('loadFailure', () => {
  it('연결 실패와 서버가 낸 사유를 가른다', () => {
    expect(loadFailure(new TypeError('Failed to fetch')).transport).toBe(true)
    expect(loadFailure(new TypeError('NetworkError when attempting to fetch')).transport).toBe(true)
    expect(
      loadFailure(new Error('task generator not started (GET /api/taskgen → 500)')).transport,
    ).toBe(false)
    // 연결 한도에 막혀 끊긴 조회도 연결 문제다(브라우저는 한 호스트에 6 연결까지만).
    expect(
      loadFailure(new Error('응답 없음 8초 (GET /api/taskgen) — 콘솔 연결이 막혔습니다')).transport,
    ).toBe(true)
  })

  it('연결 실패는 엔진이 아니라 콘솔 연결을 말한다', () => {
    const t = loadFailureText(new TypeError('Failed to fetch'), '생성 규칙', true)
    expect(t).toContain('콘솔에 연결하지 못해')
    expect(t).toContain('생성 규칙 갱신이 멈췄습니다')
    expect(t).toContain('마지막으로 읽은 값')
    expect(t).not.toContain('엔진을 읽지 못함')

    const s = loadFailureText(new Error('task generator not started'), '생성 규칙', false)
    expect(s).toBe('생성 규칙을 읽지 못함 — task generator not started')
  })
})
