/**
 * Reusable Excel-like DataGrid — public types.
 *
 * The grid is generic over a row type `T` and fully *controlled*: it never
 * mutates row data. Consumers supply column accessors (text / editability /
 * semantic styling) and receive edit intents via `onedit` / `oneditmany`,
 * keeping the source of truth (drafts, stores, …) in the parent. This is what
 * lets the same component back the parameter grid today and other tables later.
 */
import type { ReactNode, Ref } from 'react'

export type CellAlign = 'left' | 'right' | 'center'
/** 'none' = display-only cell (still selectable & copyable, never editable) */
export type CellEditor = 'text' | 'number' | 'select' | 'picker' | 'none'

export interface DataGridColumn<T> {
  id: string
  header?: string
  /** small secondary header line (e.g. a unit) */
  headerSub?: string
  /** preferred / base CSS width, e.g. '12rem' / '80px' */
  width?: string
  /** maximum width — caps runaway growth (content then wraps/truncates) */
  maxWidth?: string
  align?: CellAlign
  /** monospace cell text (numbers, hex, ids, paths) */
  mono?: boolean
  /** never wrap; truncate with ellipsis */
  nowrap?: boolean
  /** opt out of the drag-resize handle (default resizable) */
  resizable?: boolean
  /** pin to the left; stays put on horizontal scroll */
  sticky?: boolean
  /** editor kind — may vary per row (e.g. a value column that is enum on some
   *  rows and numeric on others). 'picker' hands the editing cell to the
   *  `picker` render prop: the value comes from choosing a row in a preview
   *  table, not from typing (references — "which speed slot?") */
  editor?: CellEditor | ((row: T) => CellEditor)
  /** options for the 'select' editor (may vary per row) */
  options?: readonly (string | number)[] | ((row: T) => readonly (string | number)[])
  inputMode?: 'numeric' | 'decimal' | 'text' | ((row: T) => 'numeric' | 'decimal' | 'text')
  /** Round numeric DISPLAY to this many decimals (non-integers only). 'number'
   *  columns default to 1; set explicitly to override (e.g. 0, 3). Editing and
   *  clipboard copy always use the full unrounded value */
  decimals?: number
  /** display text — also the value copied to the clipboard */
  text: (row: T) => string
  /** value placed in the editor when editing starts (defaults to `text`) */
  editText?: (row: T) => string
  /** per-cell editability override (defaults to `editor !== 'none'`) */
  editable?: (row: T) => boolean
  /** extra CSS classes for the cell — used for semantic highlight */
  cellClass?: (row: T) => string
  /** validation: return an error message to mark the cell invalid, else null */
  invalid?: (row: T) => string | null
  /** tooltip text for the cell */
  title?: (row: T) => string
  /** Paste type-safety: normalise an incoming pasted string, or return null to
   *  REJECT it for this column (e.g. a non-numeric value pasted into a numeric
   *  cell). Rejected cells are skipped so a wrong field can't overwrite a value */
  coercePaste?: (value: string, row: T) => string | null
  /** Live type filter: called with the WHOLE prospective edit buffer (not the
   *  keystroke) on beforeinput and on the type-to-edit seed key; returning
   *  false rejects the keystroke. Must accept in-progress partial states
   *  ("", "-", "1.", "1e-") — see inputFilters.ts. 'number' editors default
   *  to the partial-float filter when omitted */
  inputFilter?: (next: string, row: T) => boolean
  /** allow click-to-sort on this column's header */
  sortable?: boolean
  /** stable sort key (defaults to `text`). Use a draft-independent value so
   *  editing a sorted column doesn't reorder rows under the cursor */
  sortValue?: (row: T) => string | number
}

/** Imperative handle — toolbar entry point for copying the current selection */
export interface DataGridHandle {
  copySelection: () => boolean
}

export interface DataGridProps<T> {
  rows: T[]
  columns: DataGridColumn<T>[]
  /** stable identity for a row (used as the list key) */
  rowId: (row: T) => string
  /** optional `data-testid` per cell — lets a consumer address cells by domain
   *  identity (profile + key) instead of display coordinates, which shift when
   *  rows are filtered or sorted */
  cellTestId?: (row: T, col: DataGridColumn<T>) => string | undefined
  /** extra CSS classes for a row (e.g. semantic tone) */
  rowClass?: (row: T) => string
  /** row click handler — adds cursor-pointer + hover */
  onrowclick?: (row: T, rowIndex: number) => void
  /** double-click / double-tap on a NON-editable cell (editable cells open the
   *  editor instead). Use to escalate a truncated row to a detail view */
  onrowdblclick?: (row: T, rowIndex: number) => void
  /** disable all editing (still selectable/copyable/sortable) */
  readOnlyAll?: boolean
  /** message shown when there are no rows */
  empty?: string
  /** zebra striping */
  zebra?: boolean
  /** localStorage key for persisting column-resize widths */
  persistKey?: string
  /** commit one edited cell (omit for read-only grids) */
  onedit?: (rowId: string, colId: string, value: string) => void
  /** commit many cells (paste) */
  oneditmany?: (updates: { rowId: string; colId: string; value: string }[]) => void
  /** notified with how many pasted cells were rejected by a column's coercePaste */
  onpasteskipped?: (count: number) => void
  /** sticky header row (default true) */
  stickyHeader?: boolean
  /** fixed table layout — columns honor their declared width exactly and cell
   *  content truncates instead of stretching the column. Pair with per-cell
   *  truncation for a deterministic, single-line row height */
  layoutFixed?: boolean
  className?: string
  /** max height for the scroll viewport (e.g. '70vh'); enables vertical scroll */
  maxHeight?: string
  /** custom display content per cell (falls back to `col.text`). Editing always
   *  uses the built-in editor; this renders the non-editing display only */
  cell?: (arg: { row: T; col: DataGridColumn<T>; rowIndex: number }) => ReactNode
  /** editor body for `editor: 'picker'` cells. Call `choose` to commit a value
   *  (it flows through the same edit pipeline as typing — drafts included) or
   *  `cancel` to leave the cell untouched. Escape and focus-out cancel too */
  picker?: (arg: {
    row: T
    col: DataGridColumn<T>
    value: string
    choose: (v: string) => void
    cancel: () => void
  }) => ReactNode
  /** imperative handle — `copySelection()` for a toolbar copy button */
  ref?: Ref<DataGridHandle>
}
