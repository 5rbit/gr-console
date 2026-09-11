// vitest 설정 — **DOM 무관 순수 모듈만**(src/**/*.test.ts). 컴포넌트 테스트는 만들지 않는다(e2e-pw가
// 실 백엔드로 검증 — 중복 금지). include를 좁게 잡아 e2e-pw의 playwright *.spec.ts를 절대 줍지 않는다.
// vite.config.ts와 분리해 둔다 — react/tailwind 플러그인 없이 순수 TS만 돌리는 게 목적이라 설정이 겹치지 않는다.
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
