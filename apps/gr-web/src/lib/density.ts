// 표시 밀도 — 셸 전역 한 축(표준/조밀). VSCode의 `Customize Layout > Default | Compact`에 해당한다.
//
// 왜 전역 한 축인가: 예전에는 사이드바만 자기 `gr-sidebar-dense`를 들고 있었다. 밀도가 화면마다
// 따로 있으면 같은 표가 자리에 따라 다른 높이로 서고, "좁게 보기"를 켠 사람은 화면마다 그 스위치를
// 다시 찾아야 한다. 밀도는 **그 사람이 화면을 얼마나 빽빽하게 읽는지**라서 화면의 성질이 아니다.
//
// 적용은 CSS 변수로 한다(`app.css`의 `[data-density='compact']`) — 컴포넌트가 `dense` prop을
// 릴레이하지 않아야 새 화면이 저절로 따라온다.
import { Store } from './store'

const KEY = 'gr-density'

/** 표시 밀도 — `comfortable` 표준 · `compact` 조밀. */
export type DensityValue = 'comfortable' | 'compact'

function initial(): DensityValue {
  try {
    // 옛 사이드바 전용 키(`gr-sidebar-dense`)를 물려받는다 — 이미 조밀하게 쓰던 사람의 설정을 버리지 않는다.
    if (localStorage.getItem(KEY) === 'compact' || localStorage.getItem('gr-sidebar-dense') === '1')
      return 'compact'
  } catch {
    /* private mode — 기본값 */
  }
  return 'comfortable'
}

class Density extends Store {
  #value: DensityValue = 'comfortable'

  /** 부팅 시 1회 — DOM에 붙이고 저장값을 반영한다. 모듈 로드 시점에 document를 만지지 않기 위해 따로 둔다. */
  start(): void {
    this.set(initial())
  }

  get value(): DensityValue {
    return this.#value
  }
  get isCompact(): boolean {
    return this.#value === 'compact'
  }

  set(next: DensityValue): void {
    this.#value = next
    if (typeof document !== 'undefined')
      document.documentElement.setAttribute('data-density', next)
    try {
      localStorage.setItem(KEY, next)
    } catch {
      /* 저장 실패는 무시 — 런타임 상태는 유지된다. */
    }
    this.notify()
  }

  toggle(): void {
    this.set(this.#value === 'compact' ? 'comfortable' : 'compact')
  }
}

/** 앱 전역 밀도 핸들. `density.isCompact`로 읽고 `density.toggle()`로 전환한다. */
export const density = new Density()
