// **엑셀형 데이터 그리드** — 범위 셀 선택(클릭·Shift·드래그·방향키) · TSV 복사/붙여넣기(엑셀 왕복,
// focus-sink textarea라 평문 HTTP에서도 동작) · 인라인 편집(입력 단계 타입 필터·IME 복원) ·
// Undo/Redo · 셀 단위 시맨틱 강조. **완전 제어형**이라 값의 진실원은 부모가 갖는다(DataGridProps).
//
// 색은 프로젝트 톤 체계(`lib/status.ts`)를 따른다 — 라이트/다크 양쪽.
import type * as React from 'react'
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import type { DataGridColumn, DataGridProps } from './types'
import { GridController } from './gridController'
import { FLOAT_PARTIAL } from './inputFilters'

export type { DataGridColumn, DataGridProps, DataGridHandle, CellAlign, CellEditor } from './types'

export function DataGrid<T>({
  rows,
  columns,
  rowId,
  cellTestId,
  rowClass,
  onrowclick,
  onrowdblclick,
  readOnlyAll = false,
  empty = '—',
  zebra = false,
  persistKey,
  onedit,
  oneditmany,
  onpasteskipped,
  stickyHeader = true,
  layoutFixed = false,
  className: extraClass = '',
  maxHeight,
  cell,
  picker,
  ref,
}: DataGridProps<T>) {
  // ── 열 너비 드래그(리사이즈 · localStorage 영속) ────────────────────────────
  const MIN_COL_PX = 48
  const [resized, setResized] = useState<Record<string, number>>({})
  const [sortCol, setSortCol] = useState<string | null>(null)
  const [sortDir, setSortDir] = useState<1 | -1>(1)
  // Selection chrome (rings) only renders while the grid owns focus — a
  // resting (0,0) ring on every read-only overview table is pure noise.
  const [gridFocused, setGridFocused] = useState(false)

  const sink = useRef<HTMLTextAreaElement | null>(null)
  const viewport = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!persistKey) return
    try {
      const raw = localStorage.getItem(`mxdev.grid.${persistKey}`)
      if (raw) setResized(JSON.parse(raw) as Record<string, number>)
    } catch {
      /* corrupt — start fresh */
    }
  }, [persistKey])

  const persistResize = useCallback(
    (map: Record<string, number>) => {
      if (!persistKey) return
      try {
        localStorage.setItem(`mxdev.grid.${persistKey}`, JSON.stringify(map))
      } catch {
        /* quota */
      }
    },
    [persistKey],
  )

  function startResize(col: DataGridColumn<T>, e: React.PointerEvent<HTMLElement>) {
    if (e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()
    const th = (e.currentTarget as HTMLElement).parentElement as HTMLElement
    const startX = e.clientX
    const startW = th.getBoundingClientRect().width
    document.body.style.cursor = 'col-resize'
    let next = resized
    const onMove = (ev: PointerEvent) => {
      next = { ...next, [col.id]: Math.max(MIN_COL_PX, Math.round(startW + (ev.clientX - startX))) }
      setResized(next)
    }
    const onUp = () => {
      document.body.style.cursor = ''
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      persistResize(next)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }
  function clearResize(col: DataGridColumn<T>, e: React.MouseEvent) {
    e.stopPropagation()
    if (!(col.id in resized)) return
    const next = { ...resized }
    delete next[col.id]
    setResized(next)
    persistResize(next)
  }

  // ── sticky 열의 left: 앞선 sticky 열 폭을 calc()로 합산(그래서 width 필수) ──
  function stickyLeft(c: number): string | undefined {
    if (!columns[c].sticky) return undefined
    const widths: string[] = []
    for (let i = 0; i < c; i++) if (columns[i].sticky) widths.push(columns[i].width ?? '0')
    return widths.length ? `calc(${widths.join(' + ')})` : '0'
  }

  function thStyle(col: DataGridColumn<T>, c: number): React.CSSProperties {
    const s: React.CSSProperties = {}
    const o = resized[col.id]
    if (o !== undefined) {
      s.width = `${o}px`
      s.minWidth = `${o}px`
      s.maxWidth = `${o}px`
    } else {
      if (col.width) {
        s.minWidth = col.width
        s.width = col.width
      }
      if (col.maxWidth) s.maxWidth = col.maxWidth
    }
    if (col.sticky) s.left = stickyLeft(c)
    return s
  }
  function tdStyle(col: DataGridColumn<T>, c: number): React.CSSProperties {
    const s: React.CSSProperties = {}
    const o = resized[col.id]
    if (o !== undefined) s.maxWidth = `${o}px`
    else if (col.maxWidth) s.maxWidth = col.maxWidth
    if (col.sticky) s.left = stickyLeft(c)
    return s
  }

  // Resolve possibly-per-row column facets.
  const editorOf = (col: DataGridColumn<T>, row: T) =>
    typeof col.editor === 'function' ? col.editor(row) : col.editor
  const optionsOf = (col: DataGridColumn<T>, row: T) =>
    (typeof col.options === 'function' ? col.options(row) : col.options) ?? []
  const inputModeOf = (col: DataGridColumn<T>, row: T) => {
    const im = typeof col.inputMode === 'function' ? col.inputMode(row) : col.inputMode
    return im ?? (editorOf(col, row) === 'number' ? 'decimal' : 'text')
  }
  // Display-only numeric rounding: a 'number' column shows non-integer floats at
  // col.decimals (default 1) so a noisy live value (e.g. 199.99998) reads as
  // "200.0". Integers and non-numeric text pass through; edit/copy use raw text.
  function displayText(col: DataGridColumn<T>, row: T): string {
    const raw = col.text(row)
    if (editorOf(col, row) !== 'number') return raw
    const n = Number(raw)
    if (raw.trim() === '' || !Number.isFinite(n) || Number.isInteger(n)) return raw
    return n.toFixed(col.decimals ?? 1)
  }
  const colEditable = (c: number, row: T) => {
    if (readOnlyAll) return false
    const col = columns[c]
    if (col.editable) return col.editable(row)
    const ed = editorOf(col, row)
    return !!ed && ed !== 'none'
  }

  // ── 입력 단계 타입 필터(키 입력마다) ──────────────────────────────────────
  // Explicit per-column filter, else 'number' editors get the partial-float
  // default so letters can't be typed into numeric cells at all.
  const filterOf = (col: DataGridColumn<T>, row: T): ((next: string) => boolean) | undefined => {
    if (col.inputFilter) return (next) => col.inputFilter!(next, row)
    return editorOf(col, row) === 'number' ? (next) => FLOAT_PARTIAL.test(next) : undefined
  }

  // ── 클라이언트 정렬(편집 중 행이 튀지 않게 draft 무관 키를 쓴다) ──────────
  function sortKey(col: DataGridColumn<T>, row: T): string | number {
    return col.sortValue ? col.sortValue(row) : col.text(row)
  }
  const viewRows = useMemo(() => {
    if (!sortCol) return rows
    const col = columns.find((c) => c.id === sortCol)
    if (!col) return rows
    return [...rows].sort((a, b) => {
      const va = sortKey(col, a),
        vb = sortKey(col, b)
      const na = typeof va === 'number' ? va : Number(va)
      const nb = typeof vb === 'number' ? vb : Number(vb)
      // numeric collation: 'limits[2]'가 'limits[10]' 앞에 온다(사전순이면 뒤로 밀린다).
      const d =
        !Number.isNaN(na) && !Number.isNaN(nb)
          ? na - nb
          : String(va).localeCompare(String(vb), undefined, { numeric: true })
      return d * sortDir
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- sortKey is a pure render-local helper
  }, [rows, columns, sortCol, sortDir])
  function toggleSort(col: DataGridColumn<T>) {
    if (!col.sortable) return
    if (sortCol !== col.id) {
      setSortCol(col.id)
      setSortDir(1)
    } else if (sortDir === 1) setSortDir(-1)
    else setSortCol(null)
  }

  // 컨트롤러는 한 번만 만들고, 매 렌더의 클로저(행·열·핸들러)는 이 ref로 흘려 넣는다 —
  // 컨트롤러를 다시 만들면 선택·편집·Undo 이력이 통째로 날아간다.
  const live = useRef({
    viewRows,
    columns,
    rowId,
    onedit,
    oneditmany,
    onpasteskipped,
    colEditable,
    filterOf,
  })
  live.current = {
    viewRows,
    columns,
    rowId,
    onedit,
    oneditmany,
    onpasteskipped,
    colEditable,
    filterOf,
  }

  const [ctrl] = useState(
    () =>
      new GridController({
        rows: () => live.current.viewRows.length,
        cols: () => live.current.columns.length,
        cellText: (r, c) => live.current.columns[c].text(live.current.viewRows[r]),
        editText: (r, c) =>
          (live.current.columns[c].editText ?? live.current.columns[c].text)(
            live.current.viewRows[r],
          ),
        isEditable: (r, c) => live.current.colEditable(c, live.current.viewRows[r]),
        commit: (r, c, value) =>
          live.current.onedit?.(
            live.current.rowId(live.current.viewRows[r]),
            live.current.columns[c].id,
            value,
          ),
        commitMany: (cells) =>
          live.current.oneditmany?.(
            cells.map(({ r, c, value }) => ({
              rowId: live.current.rowId(live.current.viewRows[r]),
              colId: live.current.columns[c].id,
              value,
            })),
          ),
        pageSize: () => {
          const vp = viewport.current
          const n = live.current.viewRows.length
          const rh = n ? (vp?.clientHeight ?? 0) / Math.max(1, n) : 0
          return rh > 0 && vp ? Math.max(1, Math.floor(vp.clientHeight / rh) - 1) : 10
        },
        coercePaste: (r, c, value) => {
          const col = live.current.columns[c]
          return col.coercePaste ? col.coercePaste(value, live.current.viewRows[r]) : value
        },
        onPasteSkipped: (n) => live.current.onpasteskipped?.(n),
        inputFilter: (r, c, next) =>
          live.current.filterOf(live.current.columns[c], live.current.viewRows[r])?.(next) ?? true,
        // **preventScroll 필수** — sink는 스크롤 뷰포트 밖에 있어, 그냥 focus()하면 브라우저가 그것을
        // 보이게 하려고 표를 맨 위로 되감는다. 그러면 더블클릭의 두 번째 클릭이 같은 화면 좌표의 **다른
        // 행**에 떨어져 엉뚱한 셀이 편집된다(실제로 그 증상이 나왔다).
        focusSink: () => sink.current?.focus({ preventScroll: true }),
      }),
  )

  const snap = useSyncExternalStore(ctrl.subscribe, ctrl.getSnapshot, ctrl.getSnapshot)

  const prevLen = useRef(-1)
  const prevSort = useRef('')
  useEffect(() => {
    const n = viewRows.length
    const s = `${sortCol}:${sortDir}`
    // A changed row count (node switch, filter) or new sort order remaps (r,c),
    // so stale (r,c) history would target the wrong cells — drop it.
    if (
      (prevLen.current !== -1 && n !== prevLen.current) ||
      (prevSort.current && s !== prevSort.current)
    )
      ctrl.resetHistory()
    prevLen.current = n
    prevSort.current = s
    ctrl.clampToBounds()
  }, [viewRows, sortCol, sortDir, columns.length, ctrl])

  // ── 편집 input의 타입 필터 / IME ─────────────────────────────────────────
  // beforeinput → preventDefault rejects without caret jumps or rollbacks.
  // IME composition can't be canceled mid-flight (insertCompositionText is
  // non-cancelable), so composition is let through here and settled at
  // compositionend below.
  function editBeforeInput(e: React.InputEvent<HTMLInputElement>, col: DataGridColumn<T>, row: T) {
    // React synthesizes onBeforeInput from Chromium's `textInput` (a TextEvent): no `inputType`, no
    // `dataTransfer` — only `data`. Treat a missing inputType as a text insert so the filter still runs
    // (reading it unguarded threw on every keystroke and the filter never rejected anything).
    const ne = e.nativeEvent as Partial<InputEvent>
    const filter = filterOf(col, row)
    if (!filter || ne.isComposing) return
    const inputType = typeof ne.inputType === 'string' ? ne.inputType : 'insertText'
    if (!inputType.startsWith('insert')) return // deletes / history always pass
    const el = e.currentTarget
    const ins = e.data ?? ne.data ?? ne.dataTransfer?.getData('text/plain') ?? ''
    const next =
      el.value.slice(0, el.selectionStart ?? el.value.length) +
      ins +
      el.value.slice(el.selectionEnd ?? el.value.length)
    if (!filter(next)) e.preventDefault()
  }
  function editInput(e: React.InputEvent<HTMLInputElement>, col: DataGridColumn<T>, row: T) {
    // While composing on a filtered cell, hold the last accepted buffer —
    // compositionend accepts or restores. Unfiltered cells keep live updates
    // (a blur mid-composition still commits what's visible).
    if (filterOf(col, row) && e.nativeEvent.isComposing) return
    ctrl.editBuf = e.currentTarget.value
  }
  function editCompositionEnd(
    e: React.CompositionEvent<HTMLInputElement>,
    col: DataGridColumn<T>,
    row: T,
  ) {
    const filter = filterOf(col, row)
    const el = e.currentTarget
    // The only imperative DOM write in the filter path: value is uncontrolled,
    // so restoring the last accepted buffer must touch the element.
    if (filter && !filter(el.value)) el.value = ctrl.editBuf
    else ctrl.editBuf = el.value
  }

  // Seeded (type-to-replace) edits put the caret at the end so the next
  // keystroke APPENDS; opened edits (dblclick/Enter/F2) select the old
  // value so the next keystroke REPLACES — both Excel behaviours.
  const autofocus = useCallback(
    (node: HTMLInputElement | HTMLSelectElement | null) => {
      if (!node) return
      node.focus()
      if (!(node instanceof HTMLInputElement)) return
      if (ctrl.editSeeded) node.setSelectionRange(node.value.length, node.value.length)
      else node.select()
    },
    [ctrl],
  )

  // ── 포인터 조작(엑셀 모델) ───────────────────────────────────────────────
  // Mouse: press-drag selects a cell range (vertical or 2D); a plain click
  // only SELECTS — editing opens via double-click, Enter/F2, or typing
  // (type-to-replace, see gridController.onKeydown). Touch: a tap selects, a
  // double-tap edits, a swipe scrolls. Click must not open the editor: an
  // editor mounted mid-click has its select()-ed text collapsed by the
  // residual mouseup, so typing inserts instead of replacing.
  const down = useRef<{
    r: number
    c: number
    x: number
    y: number
    shift: boolean
    touch: boolean
  } | null>(null)
  const dragMoved = useRef(false)
  // 좌표는 DOM의 data-cell에서 읽는다 — 리스트 인덱스를 클로저로 넘기면 행이 재배열될 때
  // 마크업(data-cell)과 핸들러가 어긋난다.
  // 드래그가 이미 elementFromPoint로 같은 속성을 읽으므로 경로도 하나로 모인다.
  function coordOf(e: { currentTarget: EventTarget | null }): [number, number] | null {
    const el = (e.currentTarget as HTMLElement | null)?.closest('[data-cell]')
    const m = el?.getAttribute('data-cell')?.split('-').map(Number)
    return m && m.length === 2 && !Number.isNaN(m[0]) && !Number.isNaN(m[1]) ? [m[0], m[1]] : null
  }

  const cellPointerMove = useCallback(
    (e: PointerEvent) => {
      const d = down.current
      if (!d) return
      if (!dragMoved.current && Math.abs(e.clientX - d.x) + Math.abs(e.clientY - d.y) < 6) return
      dragMoved.current = true
      if (d.touch) return // touch move = scroll; leave it to the browser
      const el = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest(
        '[data-cell]',
      )
      const m = el?.getAttribute('data-cell')?.split('-').map(Number)
      if (m && !Number.isNaN(m[0])) ctrl.extendTo(m[0], m[1])
    },
    [ctrl],
  )

  const cellPointerUp = useCallback(() => {
    window.removeEventListener('pointermove', cellPointerMove)
    window.removeEventListener('pointerup', cellPointerUp)
    const d = down.current
    down.current = null
    if (!d || dragMoved.current) return // drag → range already set (mouse) / scroll (touch)
    if (d.touch) ctrl.selectCell(d.r, d.c, d.shift) // touch selects on tap (deferred so swipe scrolls)
  }, [ctrl, cellPointerMove])

  useEffect(
    () => () => {
      window.removeEventListener('pointermove', cellPointerMove)
      window.removeEventListener('pointerup', cellPointerUp)
    },
    [cellPointerMove, cellPointerUp],
  )

  function cellPointerDown(
    e: React.PointerEvent<HTMLTableCellElement>,
    rHint: number,
    cHint: number,
  ) {
    const co = coordOf(e) ?? [rHint, cHint]
    const r = co[0]
    const c = co[1]
    if (e.pointerType === 'mouse' && e.button !== 0) return
    if ((e.target as HTMLElement).closest('input,select,button,textarea,a,[role="separator"]'))
      return
    if (ctrl.isEditing(r, c)) return
    // Mouse only: cancel the native mousedown so a drag that starts on a cell
    // can never start a text selection (even one extending into neighbouring
    // selectable elements). Focus is unaffected — selectCell() focuses the
    // sink explicitly, and click/dblclick still fire after a canceled
    // pointerdown. Touch keeps default behaviour (tap semantics + scrolling).
    if (e.pointerType === 'mouse') e.preventDefault()
    down.current = {
      r,
      c,
      x: e.clientX,
      y: e.clientY,
      shift: e.shiftKey,
      touch: e.pointerType !== 'mouse',
    }
    dragMoved.current = false
    if (!down.current.touch) ctrl.selectCell(r, c, e.shiftKey) // mouse: anchor now so drag can extend
    window.addEventListener('pointermove', cellPointerMove)
    window.addEventListener('pointerup', cellPointerUp)
  }

  // Double-click (mouse) / double-tap (touch — `touch-manipulation` on the
  // table drops dbl-tap-zoom, so dblclick fires there too): editable cells open
  // the editor; a non-editable cell escalates to onrowdblclick (row detail).
  function cellDblClick(e: React.MouseEvent<HTMLTableCellElement>, rHint: number, cHint: number) {
    const co = coordOf(e) ?? [rHint, cHint]
    const r = co[0]
    const c = co[1]
    if ((e.target as HTMLElement).closest('input,select,button,textarea,a,[role="separator"]'))
      return
    if (ctrl.isEditing(r, c)) return
    if (colEditable(c, viewRows[r])) ctrl.beginEdit(r, c)
    else onrowdblclick?.(viewRows[r], r)
  }

  /** 툴바 복사 버튼용 진입점 — 선택 범위를 sink의 네이티브 copy로 내보낸다(키보드 불요).
   *  `execCommand`는 폐기 예정이지만 클립보드 권한 없이 도는 유일한 동기 경로다(평문 HTTP 배포) */
  const copySelection = useCallback((): boolean => {
    sink.current?.focus({ preventScroll: true })
    try {
      return document.execCommand('copy')
    } catch {
      return false
    }
  }, [])
  useImperativeHandle(ref, () => ({ copySelection }), [copySelection])

  // picker의 취소 경로 — Esc와 **포커스 이탈**. 입력이 없어 blur-commit이 없으므로 여기서 닫는다.
  const pickerChrome = useCallback(
    (node: HTMLDivElement | null) => {
      if (!node) return
      const first = node.querySelector<HTMLElement>('button,[tabindex]:not([tabindex="-1"])')
      first?.focus({ preventScroll: true })
      // 아래로 열면 스크롤 뷰포트 밖으로 잘리는 자리(표 하단 행)에서는 **위로** 연다.
      requestAnimationFrame(() => {
        const pop = node.firstElementChild as HTMLElement | null
        const vpEl = viewport.current
        if (!pop || !vpEl) return
        const vp = vpEl.getBoundingClientRect()
        const r = pop.getBoundingClientRect()
        if (r.bottom > vp.bottom && r.height < vp.height) {
          pop.style.top = 'auto'
          pop.style.bottom = '0'
        }
      })
      const onKey = (e: KeyboardEvent) => {
        if (e.key === 'Escape') {
          e.preventDefault()
          ctrl.cancelEdit()
        }
      }
      const onOut = (e: FocusEvent) => {
        const next = e.relatedTarget as Node | null
        if (!next || !node.contains(next)) ctrl.cancelEdit()
      }
      node.addEventListener('keydown', onKey)
      node.addEventListener('focusout', onOut)
      return () => {
        node.removeEventListener('keydown', onKey)
        node.removeEventListener('focusout', onOut)
      }
    },
    [ctrl],
  )

  return (
    /* min-w-0: as a grid/flex item the root must not adopt the table's
       min-content width — the inner viewport scrolls instead, so a wide grid
       can't wedge its host column open (narrow-viewport overflow). */
    <div className={`relative min-w-0 ${extraClass}`}>
      {/* Focus sink: keyboard + native copy/paste. inputMode="none" so focusing it
          on a tablet cell-tap does NOT raise the on-screen keyboard (selection/copy
          are desktop affordances; editing uses the cell <input>, which still pops
          the keyboard correctly). */}
      {/* **키보드로 표에 들어올 수 있어야 한다.** 싱크가 `tabindex="-1"`이라 Tab으로는 그리드에 진입 자체가
          불가능했고(컨피그·파라미터 표의 선택·복사·편집이 전부 마우스 선행 필수), `aria-hidden`이라 스크린
          리더에게는 존재하지도 않았다. 포커스를 받을 수 있게 하고 진입 시 첫 셀을 고른다. */}
      <textarea
        ref={sink}
        data-grid-sink=""
        tabIndex={0}
        aria-label="표 — 방향키로 셀 이동, 복사·붙여넣기, Enter로 편집"
        inputMode="none"
        autoCapitalize="off"
        autoComplete="off"
        spellCheck={false}
        className="absolute opacity-0 pointer-events-none h-0 w-0 -z-10"
        onKeyDown={ctrl.onKeydown}
        onCopy={ctrl.onCopy}
        onPaste={ctrl.onPaste}
        // 컨트롤러는 항상 활성 셀을 갖고 있다(기본 0,0) — Tab으로 들어오면 그 셀에 선택 테두리가 서고
        // 방향키가 곧바로 먹는다(진입은 됐는데 아무 데도 안 가는 상태를 만들지 않는다).
        onFocus={() => setGridFocused(true)}
        onBlur={() => setGridFocused(false)}
      />

      <div ref={viewport} className="overflow-auto" style={maxHeight ? { maxHeight } : undefined}>
        {/* touch-manipulation: drop the 300ms tap delay + double-tap-zoom so
            double-tap-to-edit is reliable on tablets (pan/scroll still work). */}
        <table
          className={`w-full text-xs border-collapse select-none touch-manipulation ${layoutFixed ? 'table-fixed' : ''}`}
          role="grid"
        >
          <thead
            className={`bg-surface-app ${stickyHeader ? 'sticky top-0 z-20 shadow-sm' : ''}`}
          >
            <tr className="border-b border-line-default text-content-faint">
              {columns.map((col, c) => (
                <th
                  key={col.id}
                  className={`relative px-2 py-1.5 font-medium ${col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'} ${col.nowrap ? 'whitespace-nowrap' : ''} ${col.sticky ? 'sticky z-30 bg-surface-app' : ''} ${col.sortable ? 'cursor-pointer select-none hover:text-content-secondary' : ''}`}
                  style={thStyle(col, c)}
                  aria-sort={
                    sortCol === col.id ? (sortDir === 1 ? 'ascending' : 'descending') : undefined
                  }
                  onClick={() => toggleSort(col)}
                >
                  {col.header ?? ''}
                  {col.headerSub ? <span className="text-content-faint"> {col.headerSub}</span> : null}
                  {sortCol === col.id ? (
                    <span className="text-accent-text"> {sortDir === 1 ? '▲' : '▼'}</span>
                  ) : null}
                  {col.resizable !== false ? (
                    <span
                      role="separator"
                      aria-orientation="vertical"
                      aria-label="resize column"
                      className="group absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize select-none touch-none"
                      onPointerDown={(e) => startResize(col, e)}
                      onDoubleClick={(e) => clearResize(col, e)}
                    >
                      <span className="absolute top-1 right-0 bottom-1 w-px bg-line-strong group-hover:bg-accent" />
                    </span>
                  ) : null}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {viewRows.length === 0 ? (
              <tr>
                <td className="px-3 py-3 text-center text-content-faint" colSpan={columns.length}>
                  {empty}
                </td>
              </tr>
            ) : null}
            {viewRows.map((row, r) => (
              <tr
                key={rowId(row)}
                className={`border-b border-line-subtle ${
                  zebra && r % 2 === 1 ? 'bg-surface-app' : ''
                } ${rowClass?.(row) ?? ''} ${onrowclick ? 'cursor-pointer hover:bg-surface-app' : ''}`}
                onClick={onrowclick ? () => onrowclick(row, r) : undefined}
              >
                {columns.map((col, c) => {
                  const editing = ctrl.isEditing(r, c)
                  const editable = colEditable(c, row)
                  const sel = ctrl.inSelection(r, c)
                  const active = ctrl.isActive(r, c)
                  const err = col.invalid?.(row) ?? null
                  return (
                    /* select-none: 드래그가 네이티브 텍스트 선택으로 새지 않게(포인터 핸들러의
                       preventDefault와 이중 방어). 편집 input은 그대로 선택 가능하다. */
                    <td
                      key={col.id}
                      data-cell={`${r}-${c}`}
                      data-testid={cellTestId?.(row, col)}
                      role="gridcell"
                      aria-selected={sel}
                      aria-readonly={!editable}
                      className={`px-0 py-0 relative align-middle select-none ${col.cellClass?.(row) ?? ''}
                        ${gridFocused && sel ? 'ring-1 ring-accent/60 ring-inset' : ''}
                        ${gridFocused && active ? 'ring-2 ring-accent ring-inset' : ''}
                        ${err ? '-outline-offset-1 outline outline-1 outline-red-500' : ''}
                        ${col.sticky ? 'sticky z-10 bg-surface-panel' : ''}`}
                      style={tdStyle(col, c)}
                      onPointerDown={(e) => cellPointerDown(e, r, c)}
                      onDoubleClick={(e) => cellDblClick(e, r, c)}
                    >
                      {editing ? (
                        editorOf(col, row) === 'picker' && picker ? (
                          /* 참조 셀: 값이 타이핑이 아니라 **고르기**에서 온다. 커밋 경로는 타이핑과
                             같으므로 드래프트→적용 파이프라인이 그대로 적용된다. */
                          <div className="relative" ref={pickerChrome}>
                            {picker({
                              row,
                              col,
                              value: snap.editBuf,
                              choose: (v: string) => ctrl.commitValue(v),
                              cancel: () => ctrl.cancelEdit(),
                            })}
                          </div>
                        ) : editorOf(col, row) === 'select' ? (
                          // 편집기는 **비제어**다 — 제어형이면 React가 이벤트 뒤에 DOM 값을 되돌려
                          // IME 조합 중 버퍼를 붙잡는 필터 경로(compositionend 복원)와 충돌한다.
                          <select
                            defaultValue={snap.editBuf}
                            onChange={(e) => (ctrl.editBuf = e.currentTarget.value)}
                            onKeyDown={ctrl.onInputKeydown}
                            onBlur={ctrl.onBlur}
                            ref={autofocus}
                            className="w-full border border-accent bg-surface-panel px-2 py-1 font-mono outline-none"
                          >
                            {optionsOf(col, row).map((opt) => (
                              <option key={opt} value={String(opt)}>
                                {String(opt)}
                              </option>
                            ))}
                          </select>
                        ) : (
                          <input
                            type="text"
                            autoComplete="off"
                            defaultValue={snap.editBuf}
                            inputMode={inputModeOf(col, row)}
                            onBeforeInput={(e) => editBeforeInput(e, col, row)}
                            onInput={(e) => editInput(e, col, row)}
                            onCompositionEnd={(e) => editCompositionEnd(e, col, row)}
                            onKeyDown={ctrl.onInputKeydown}
                            onBlur={ctrl.onBlur}
                            ref={autofocus}
                            className={`w-full border border-accent bg-surface-panel px-2 py-1 font-mono outline-none ${
                              col.align === 'right' ? 'text-right' : ''
                            }`}
                          />
                        )
                      ) : (
                        /* flex items-center: cells carry mixed-height content
                           (text, inputs, svg, checkboxes, icon buttons) whose
                           inline baselines otherwise leave each column vertically
                           offset. Centre them all so a row reads on one line. */
                        <div
                          className={`flex items-center min-h-[1.6rem] px-2 py-1 ${col.align === 'right' ? 'justify-end' : col.align === 'center' ? 'justify-center' : 'justify-start'} ${col.mono ? 'font-mono' : ''} ${col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'} ${editable ? 'cursor-cell' : ''}`}
                          title={col.title?.(row) ?? err ?? undefined}
                        >
                          {cell ? (
                            cell({ row, col, rowIndex: r })
                          ) : (
                            <span className={`min-w-0 ${col.nowrap ? 'truncate' : 'break-all'}`}>
                              {displayText(col, row) || ' '}
                            </span>
                          )}
                        </div>
                      )}
                    </td>
                  )
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
