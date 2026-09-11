// 표시(presentation) 인프라 — 임의 컴포넌트를 **팝업·사이드바·새페이지** 중 자유 지정 모드로 띄우는 스택 스토어.
// mxdev의 어느 뷰든 panels.open(...)으로 상세·보조 패널을 열고 닫는다(첫 소비자: Watch 상세 trace 모니터 팝업).
// 스택이라 popup이 page 위에 겹칠 수 있고, Escape/백드롭은 최상단만 닫는다.
import type { ComponentType } from 'react'
import { Store } from './store'

/** 패널 표시 모드 — 팝업(모달)·사이드바(우측 드로어)·새페이지(전체 오버레이). */
export type PanelMode = 'popup' | 'sidebar' | 'page'

/** 열린 패널 1건. */
export interface PanelSpec {
  id: string
  mode: PanelMode
  title: string
  /** 렌더할 컴포넌트(props와 함께 동적 렌더 — 소비자별 props 타입이 달라 느슨하게 받는다). */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  component: ComponentType<any>
  /** 컴포넌트 props. */
  props?: Record<string, unknown>
}

let seq = 0

class Panels extends Store {
  /** 열린 패널 스택(뒤가 최상단). */
  #stack: PanelSpec[] = []
  get stack(): readonly PanelSpec[] {
    return this.#stack
  }

  /** 패널을 연다(같은 `id`면 교체 — 재클릭 시 중복 안 쌓임). 부여 id를 반환. */
  open(spec: Omit<PanelSpec, 'id'> & { id?: string }): string {
    const id = spec.id ?? `panel-${++seq}`
    this.#stack = [...this.#stack.filter((p) => p.id !== id), { ...spec, id }]
    this.notify()
    return id
  }
  /** 지정 패널을 닫는다. */
  close(id: string): void {
    this.#stack = this.#stack.filter((p) => p.id !== id)
    this.notify()
  }
  /** 최상단(가장 최근) 패널을 닫는다(Escape·백드롭용). */
  closeTop(): void {
    this.#stack = this.#stack.slice(0, -1)
    this.notify()
  }
  /** 최상단 패널(없으면 null). */
  get top(): PanelSpec | null {
    return this.#stack.at(-1) ?? null
  }
}

export const panels = new Panels()
