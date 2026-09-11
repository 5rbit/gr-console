// 테마(라이트/다크) 수동 토글 — index.html 인라인 스크립트가 최초 페인트 전 data-theme를 세팅
// (localStorage||prefers)하고, 여기서는 런타임 토글만 담당한다. `dark:` 변형은 app.css의
// @custom-variant로 [data-theme="dark"]에 바인딩돼 있다.

import { Store } from './store'

const KEY = 'gr-theme'

function initial(): 'light' | 'dark' {
  if (typeof document === 'undefined') return 'light'
  return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light'
}

class Theme extends Store {
  #current: 'light' | 'dark' = initial()

  get value(): 'light' | 'dark' {
    return this.#current
  }
  get isDark(): boolean {
    return this.#current === 'dark'
  }

  set(next: 'light' | 'dark'): void {
    this.#current = next
    document.documentElement.setAttribute('data-theme', next)
    try {
      localStorage.setItem(KEY, next)
    } catch {
      /* private mode 등 — 무시(런타임 상태는 유지) */
    }
    this.notify()
  }

  toggle(): void {
    this.set(this.#current === 'dark' ? 'light' : 'dark')
  }
}

/** 앱 전역 테마 핸들. `theme.value`로 읽고 `theme.toggle()`로 전환한다. */
export const theme = new Theme()
