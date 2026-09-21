// 작업 명령 레일의 맵+표 나눠 보기 — 저장 값 해석·비율 clamp·px 환산(DOM 없는 순수 함수).
//
// 비율을 px 가 아니라 0..1 로 저장하는 이유: 창 크기·도킹 존 크기가 바뀌어도 "맵이 절반 남짓"이라는
// 뜻이 남는다. px 로 두면 큰 모니터에서 맞춘 값이 노트북에서 표를 화면 밖으로 밀어낸다.

export const RAIL_TABS = ['split', 'layout', 'stock', 'offset', 'cell', 'station', 'item'] as const
export type RailTab = (typeof RAIL_TABS)[number]

/** 나눠 보기의 표 쪽 토글. */
export const SPLIT_TABLES = ['cell', 'station', 'stock', 'offset', 'item'] as const
export type SplitTable = (typeof SPLIT_TABLES)[number]

/**
 * 맵 모드가 정하는 표 묶음. **레이아웃 편집**은 배치 파라미터(셀·스테이션의 위치·크기·연결)를,
 * **모니터링·명령 생성**은 운용 상태(셀별 적재 수 · 스테이션 보정 · 품목)를 본다 — 명령을 만들 때
 * 필요한 것은 셀 좌표가 아니라 "몇 개 쌓였나"와 "스테이션에 얼마를 더하나"다.
 */
export type RailMode = 'edit' | 'ops'
export const TABLES_BY_MODE: Record<RailMode, readonly SplitTable[]> = {
  edit: ['cell', 'station'],
  ops: ['stock', 'offset', 'item'],
}

/** 이 모드에서 보일 표 — 저장된 표가 그 모드 묶음에 없으면 묶음의 첫 표. */
export function tableFor(s: SplitState, mode: RailMode): SplitTable {
  const t = mode === 'edit' ? s.table : s.opsTable
  return TABLES_BY_MODE[mode].includes(t) ? t : TABLES_BY_MODE[mode][0]
}

/** 이 모드의 표를 바꾼 상태(다른 모드의 선택은 그대로 둔다). */
export function withTable(s: SplitState, mode: RailMode, t: SplitTable): SplitState {
  return mode === 'edit' ? { ...s, table: t } : { ...s, opsTable: t }
}

/** `stack` = 맵 위 · 표 아래, `side` = 맵 왼쪽 · 표 오른쪽. */
export type SplitOrient = 'stack' | 'side'

export interface SplitState {
  /** 맵 쪽 몫(0..1). */
  ratio: number
  orient: SplitOrient
  /** 레이아웃 편집 모드의 표. */
  table: SplitTable
  /** 모니터링·명령 생성 모드의 표. */
  opsTable: SplitTable
}

export const MIN_RATIO = 0.2
export const MAX_RATIO = 0.8
/** 한쪽 면이 이보다 작아지면 머리띠만 남아 쓸 수 없다. */
export const MIN_PANE_PX = 160
export const DEFAULT_SPLIT: SplitState = {
  ratio: 0.55,
  orient: 'stack',
  table: 'cell',
  opsTable: 'stock',
}

export function clampRatio(r: unknown): number {
  if (typeof r !== 'number' || !Number.isFinite(r)) return DEFAULT_SPLIT.ratio
  return Math.min(MAX_RATIO, Math.max(MIN_RATIO, r))
}

/** 옛 값·모르는 값은 기본으로 떨어진다(`gr-rail-tab` 에는 split 이전 값이 남아 있다). */
export function parseRailTab(raw: string | null | undefined, def: RailTab = 'split'): RailTab {
  return raw && (RAIL_TABS as readonly string[]).includes(raw) ? (raw as RailTab) : def
}

export function parseSplit(raw: string | null | undefined): SplitState {
  if (!raw) return { ...DEFAULT_SPLIT }
  let v: unknown
  try {
    v = JSON.parse(raw)
  } catch {
    return { ...DEFAULT_SPLIT }
  }
  if (!v || typeof v !== 'object') return { ...DEFAULT_SPLIT }
  const o = v as Record<string, unknown>
  const pick = (x: unknown, mode: RailMode): SplitTable | null =>
    typeof x === 'string' && (TABLES_BY_MODE[mode] as readonly string[]).includes(x)
      ? (x as SplitTable)
      : null
  return {
    ratio: clampRatio(o.ratio),
    orient: o.orient === 'side' || o.orient === 'stack' ? o.orient : DEFAULT_SPLIT.orient,
    table: pick(o.table, 'edit') ?? DEFAULT_SPLIT.table,
    // 모드별 표 이전에 저장된 값은 `table` 하나였다 — 그것이 운용 표(재고·품목)면 그대로 옮긴다.
    opsTable: pick(o.opsTable, 'ops') ?? pick(o.table, 'ops') ?? DEFAULT_SPLIT.opsTable,
  }
}

export function serializeSplit(s: SplitState): string {
  return JSON.stringify({
    ratio: clampRatio(s.ratio),
    orient: s.orient,
    table: s.table,
    opsTable: s.opsTable,
  })
}

/** 스플리터가 끌 수 있는 맵 쪽 px 범위. 컨테이너가 작으면 양쪽이 반씩까지 줄어든다. */
export function splitBounds(total: number): { min: number; max: number } {
  if (!(total > 0)) return { min: 0, max: 0 }
  const min = Math.round(Math.min(Math.max(total * MIN_RATIO, MIN_PANE_PX), total / 2))
  return { min, max: Math.round(total - min) }
}

export function ratioToPx(ratio: number, total: number): number {
  if (!(total > 0)) return 0
  const { min, max } = splitBounds(total)
  return Math.min(max, Math.max(min, Math.round(clampRatio(ratio) * total)))
}

/** 드래그한 px 를 저장할 비율로. 컨테이너를 못 쟀으면(0) 지금 비율을 지킨다. */
export function pxToRatio(px: number, total: number, fallback: number): number {
  if (!(total > 0) || !Number.isFinite(px)) return clampRatio(fallback)
  const { min, max } = splitBounds(total)
  return clampRatio(Math.min(max, Math.max(min, px)) / total)
}

/**
 * 맵에서 셀·스테이션을 눌렀을 때 표 토글을 어디로 옮기나 — 그 모드 묶음 안에서.
 * 편집: 셀 → 셀 표, 스테이션 → 스테이션 표. 운용: 셀 → 재고 표(품목 표를 보던 중이면 그대로 — 강조한
 * 셀을 눌러 보는 중이다), 스테이션 → 스테이션 보정 표.
 */
export function tableForPick(
  current: SplitTable,
  kind: 'cell' | 'station',
  mode: RailMode,
): SplitTable {
  if (mode === 'edit') return kind === 'cell' ? 'cell' : 'station'
  if (kind === 'station') return 'offset'
  return current === 'item' ? 'item' : 'stock'
}
