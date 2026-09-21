// 워크스페이스 스토어 — 현재 레이아웃 · 저장한 배치 · 모드(워크스페이스/단일 화면) · 드래그 중인 탭.
//
// 규칙은 하나다: **상태를 바꾸는 모든 길이 `model.ts`의 순수 함수를 지난다.** 여기는 그 결과를 들고
// localStorage에 적고 `notify()`할 뿐이다. 드래그가 지나가는 길과 명령 팔레트가 지나가는 길이 갈리면
// 한쪽에서만 불변식이 깨진다.
//
// 저장은 localStorage다(서버가 아니라) — 배치는 **그 사람 그 모니터**의 성질이다. 같은 계정이
// 24인치와 벽 모니터를 오가며 같은 배치를 강요받으면 매번 다시 짜게 된다. 이름 붙인 배치는 내보내기로
// 옮긴다(JSON 한 덩어리).
import { Store } from '../store'
import {
  activatePane,
  closePane,
  findZone,
  movePane,
  normalize,
  openPane,
  parseLayout,
  renderedPanes,
  resizeZone,
  serializeLayout,
  toggleMaximize,
  toggleZone,
  type Layout,
  type ZoneId,
} from './model'
import { DEFAULT_PRESET, presetLayout } from './presets'

const LS_LAYOUT = 'gr-ws-layout'
const LS_SAVED = 'gr-ws-saved'
const LS_MODE = 'gr-ws-mode'

/** 이름 붙여 저장한 배치 1건(Unity의 `Save Layout`). */
export interface SavedLayout {
  name: string
  layout: Layout
}

/** 드래그 중인 탭 — 존 머리띠가 "여기 놓을 수 있다"를 보이기 위해 전역으로 안다. */
export interface DragState {
  pane: string
  from: ZoneId
}

function read(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null // private mode — 런타임 상태는 유지하고 저장만 포기한다.
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* 저장 실패는 조용히 넘긴다 — 배치를 못 적는 것이 화면을 멈출 이유는 아니다. */
  }
}

function parseSaved(text: string | null): SavedLayout[] {
  if (!text) return []
  try {
    const raw = JSON.parse(text) as unknown
    if (!Array.isArray(raw)) return []
    return raw.flatMap((e) => {
      const rec = e as { name?: unknown; layout?: unknown }
      if (typeof rec.name !== 'string') return []
      const l = parseLayout(JSON.stringify({ v: 1, layout: rec.layout }))
      return l ? [{ name: rec.name, layout: l }] : []
    })
  } catch {
    return []
  }
}

class Workspace extends Store {
  /** 노출 가능한 패널 id — 셸이 `console/info.tabs`를 받은 뒤 넣는다. 비면 아직 모른다(전부 허용). */
  #available: string[] = []
  /** 중앙이 비었을 때 넣을 패널(백엔드 기본 탭). */
  #fallback = 'task'
  #layout: Layout = presetLayout(DEFAULT_PRESET) as Layout
  #saved: SavedLayout[] = parseSaved(read(LS_SAVED))
  /** 워크스페이스(도킹) 모드인가. 끄면 지금까지의 단일 화면 셸이다. */
  #ws = read(LS_MODE) === 'ws'
  #drag: DragState | null = null
  /**
   * **활성 패널** — 마지막으로 만진 패널. 최대화·창 명령이 이것을 대상으로 삼는다.
   *
   * 없으면 안 되는 이유: 예전에는 `Alt+Enter`가 늘 중앙의 활성 탭을 최대화했다. 오른쪽 상태 패널을
   * 보다가 최대화를 누르면 **엉뚱한 면이 전체로 커진다** — 눌렀는데 다른 것이 반응하는 것은
   * 조작이 아니라 사고다. 화면에도 표시된다(활성 존의 탭 띠가 밝고 활성 탭에 accent 밑줄).
   */
  #focused: string | null = null
  /** 마지막으로 적용한 프리셋 id(메뉴 체크 표시용). 사용자가 배치를 고치면 `null`. */
  #preset: string | null = DEFAULT_PRESET

  constructor() {
    super()
    const stored = parseLayout(read(LS_LAYOUT))
    if (stored) {
      this.#layout = stored
      this.#preset = null
    }
  }

  get layout(): Layout {
    return this.#layout
  }
  get saved(): readonly SavedLayout[] {
    return this.#saved
  }
  get enabled(): boolean {
    return this.#ws
  }
  get drag(): DragState | null {
    return this.#drag
  }
  get presetId(): string | null {
    return this.#preset
  }
  /** 활성 패널 — 닫혔거나 아직 없으면 중앙의 활성 탭으로 떨어진다(항상 대상이 하나는 있다). */
  get focused(): string | null {
    const f = this.#focused
    if (f && findZone(this.#layout, f)) return f
    return this.#layout.zones.center.active
  }
  /** 이 존이 활성인가 — 껍데기가 면을 밝히는 데 쓴다. */
  isFocusedZone(zone: ZoneId): boolean {
    const f = this.focused
    return f !== null && findZone(this.#layout, f) === zone
  }
  /** 지금 그려지는 패널 — 비활성 탭은 마운트하지 않는다. */
  get rendered(): string[] {
    return renderedPanes(this.#layout)
  }

  /** 이 패널이 도킹된 존(없으면 `null`). */
  zoneOf(pane: string): ZoneId | null {
    return findZone(this.#layout, pane)
  }

  /**
   * 노출 가능한 패널 집합을 알린다 — 백엔드가 안 내는 화면의 탭이 남지 않게 한 번 걷어 낸다.
   * 셸 부팅에서만 부른다.
   */
  setAvailable(ids: readonly string[], fallback?: string): void {
    this.#available = [...ids]
    if (fallback && ids.includes(fallback)) this.#fallback = fallback
    this.#apply(this.#layout, this.#preset)
  }

  /** 워크스페이스 모드 전환. */
  setEnabled(on: boolean): void {
    if (this.#ws === on) return
    this.#ws = on
    write(LS_MODE, on ? 'ws' : 'single')
    this.notify()
  }
  toggleEnabled(): void {
    this.setEnabled(!this.#ws)
  }

  // ── 레이아웃 조작(전부 순수 함수를 지난다) ────────────────────────────────
  open(pane: string, zone: ZoneId = 'center'): void {
    this.#focused = pane
    this.#apply(openPane(this.#layout, pane, zone))
  }
  /** 열려 있으면 활성화, 없으면 기본 존에 연다 — 명령 팔레트·다른 화면의 유일한 진입점. */
  reveal(pane: string, zone: ZoneId = 'center'): void {
    this.#focused = pane
    this.#apply(
      findZone(this.#layout, pane)
        ? activatePane(this.#layout, pane)
        : openPane(this.#layout, pane, zone),
    )
  }
  activate(pane: string): void {
    this.#focused = pane
    this.#apply(activatePane(this.#layout, pane))
  }
  /** 만진 패널을 활성으로 — 탭 클릭·패널 안 클릭·포커스가 부른다. 바뀔 때만 알린다(클릭마다 전체 재렌더 금지). */
  focus(pane: string | null): void {
    if (pane === null || this.#focused === pane || !findZone(this.#layout, pane)) return
    this.#focused = pane
    this.notify()
  }
  close(pane: string): void {
    this.#apply(closePane(this.#layout, pane))
  }
  move(pane: string, to: ZoneId, index?: number): void {
    this.#focused = pane
    this.#apply(movePane(this.#layout, pane, to, index))
  }
  toggleZone(zone: ZoneId, next?: boolean): void {
    this.#apply(toggleZone(this.#layout, zone, next))
  }
  resize(zone: ZoneId, size: number): void {
    this.#apply(resizeZone(this.#layout, zone, size))
  }
  /**
   * 최대화 토글 — 최대화하면 그 패널을 활성 탭으로도 올린다(껍데기가 그 존 하나만 그린다).
   * 대상 기본값은 **활성 패널**이다(`focused`) — 호출부가 매번 무엇을 최대화할지 고르지 않게.
   */
  maximize(pane: string | null): void {
    const next = toggleMaximize(this.#layout, pane)
    this.#apply(next.maximized && pane ? activatePane(next, pane) : next)
  }

  /** 프리셋 적용 — 지금 배치를 버린다(되돌리기는 없다. 프리셋 자체가 되돌릴 자리다). */
  applyPreset(id: string): void {
    const l = presetLayout(id)
    if (!l) return
    this.#apply(l, id)
  }

  /** 이름 붙여 저장(같은 이름이면 덮어쓴다). */
  saveAs(name: string): void {
    const trimmed = name.trim()
    if (!trimmed) return
    this.#saved = [
      ...this.#saved.filter((s) => s.name !== trimmed),
      { name: trimmed, layout: this.#layout },
    ]
    write(LS_SAVED, JSON.stringify(this.#saved))
    this.notify()
  }
  /** 저장한 배치를 적용. */
  load(name: string): void {
    const hit = this.#saved.find((s) => s.name === name)
    if (hit) this.#apply(structuredClone(hit.layout))
  }
  /** 저장한 배치를 지운다. */
  remove(name: string): void {
    const next = this.#saved.filter((s) => s.name !== name)
    if (next.length === this.#saved.length) return
    this.#saved = next
    write(LS_SAVED, JSON.stringify(next))
    this.notify()
  }

  // ── 탭 드래그 ─────────────────────────────────────────────────────────────
  beginDrag(pane: string): void {
    const from = findZone(this.#layout, pane)
    if (!from) return
    this.#drag = { pane, from }
    this.notify()
  }
  endDrag(): void {
    if (!this.#drag) return
    this.#drag = null
    this.notify()
  }
  /** 드롭 — 드래그 중인 탭을 그 존 그 자리에 넣는다. */
  drop(to: ZoneId, index?: number): void {
    const d = this.#drag
    this.#drag = null
    if (!d) {
      this.notify()
      return
    }
    this.#apply(movePane(this.#layout, d.pane, to, index))
  }

  /** 레이아웃을 세우고 적는다 — 불변식 복원은 여기 한 곳에서만 일어난다. */
  #apply(next: Layout, presetId: string | null = null): void {
    const known = this.#available.length > 0 ? this.#available : ALL_PANE_IDS
    const sane = normalize(next, known, this.#fallback)
    this.#layout = sane
    this.#preset = presetId
    write(LS_LAYOUT, serializeLayout(sane))
    this.notify()
  }
}

/**
 * 패널 id 전부 — `setAvailable` 전(백엔드 조회 실패·부팅 중)의 허용 집합.
 * 라벨·컴포넌트는 `components/workspace/paneRegistry`가 들고, 여기는 id만 안다(lib이 화면을 모르게).
 */
export const ALL_PANE_IDS: readonly string[] = [
  'task',
  'items',
  'taskmgr',
  'measure',
  'scenario',
  'pallet',
  'robots',
  'plcs',
  'status',
]

export const workspace = new Workspace()
