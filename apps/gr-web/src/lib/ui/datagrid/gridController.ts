// 그리드 조작 엔진 — 2D 선택·인라인 편집·클립보드. 좌표(행·열 인덱스)만 다루고 데이터는 소유하지
// 않는다: 렌더와 좌표↔id 변환은 DataGrid가, 값의 진실원은 부모(드래프트·스토어)가 갖는다.
//
// 클립보드는 숨은 focus-sink textarea의 네이티브 copy/paste 이벤트로 처리한다 —
// `navigator.clipboard`는 보안 컨텍스트(HTTPS·localhost)에서만 동작해 평문 HTTP에서는 실패한다.
//
// 프레임워크 중립: 반응 상태는 listener Set + 캐시된 불변 스냅샷으로 낸다(DataGrid가
// `useSyncExternalStore`로 구독). 스냅샷은 **변이 때만** 다시 만든다 — 매번 새 객체를 돌려주면
// `useSyncExternalStore`가 무한 루프에 빠진다.
import { copyText } from '../../clipboard'

export interface GridControllerOpts {
  rows: () => number
  cols: () => number
  /** display text of a cell (for copy + edit seed) */
  cellText: (r: number, c: number) => string
  /** value to seed the editor with (defaults to cellText) */
  editText?: (r: number, c: number) => string
  isEditable: (r: number, c: number) => boolean
  /** commit one edited cell */
  commit: (r: number, c: number, value: string) => void
  /** commit many cells at once (paste) */
  commitMany: (cells: { r: number; c: number; value: string }[]) => void
  /** rows per page step (PageUp/PageDown); defaults to 10 */
  pageSize?: () => number
  /** Type-safety on paste: normalise/accept a pasted string for (r,c), or
   *  return null to REJECT it (e.g. a non-numeric value pasted into a numeric
   *  cell). Rejected cells are skipped — a wrong field can't land in a value */
  coercePaste?: (r: number, c: number, value: string) => string | null
  /** Notified with how many pasted cells were rejected by coercePaste */
  onPasteSkipped?: (count: number) => void
  /** Live type filter for (r,c): false rejects `next` as the prospective edit
   *  buffer. Gates the type-to-edit seed key here; the editor input applies
   *  the same filter per keystroke (DataGrid beforeinput) */
  inputFilter?: (r: number, c: number, next: string) => boolean
  focusSink: () => void
}

/** Keyboard event shape the controller needs — native and React synthetic both fit */
export interface GridKeyEvent {
  key: string
  keyCode: number
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  isComposing?: boolean
  nativeEvent?: { isComposing?: boolean }
  preventDefault: () => void
  stopPropagation: () => void
}

/** Clipboard event shape the controller needs — native and React synthetic both fit */
export interface GridClipboardEvent {
  clipboardData: DataTransfer | null
  preventDefault: () => void
}

/** Immutable view of the controller's reactive state */
export interface GridSnapshot {
  readonly active: { r: number; c: number }
  readonly anchor: { r: number; c: number }
  readonly editing: { r: number; c: number } | null
  readonly editBuf: string
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi)
/** True while an IME composition is active (Hangul etc.) — never commit then */
const composing = (e: GridKeyEvent) =>
  (e.isComposing ?? e.nativeEvent?.isComposing ?? false) || e.keyCode === 229

export class GridController {
  private _active: { r: number; c: number } = { r: 0, c: 0 }
  private _anchor: { r: number; c: number } = { r: 0, c: 0 }
  private _editing: { r: number; c: number } | null = null
  private _editBuf = ''
  /** True when the current edit was seeded by type-to-replace — the editor
   *  then places the caret at the end instead of selecting the old value */
  editSeeded = false

  private listeners = new Set<() => void>()
  private snap: GridSnapshot | null = null

  get active(): { r: number; c: number } {
    return this._active
  }
  set active(v: { r: number; c: number }) {
    this._active = v
    this.emit()
  }
  get anchor(): { r: number; c: number } {
    return this._anchor
  }
  set anchor(v: { r: number; c: number }) {
    this._anchor = v
    this.emit()
  }
  get editing(): { r: number; c: number } | null {
    return this._editing
  }
  set editing(v: { r: number; c: number } | null) {
    this._editing = v
    this.emit()
  }
  get editBuf(): string {
    return this._editBuf
  }
  set editBuf(v: string) {
    this._editBuf = v
    this.emit()
  }

  private emit() {
    this.snap = null
    for (const l of this.listeners) l()
  }

  /** Subscribe to state changes — returns the unsubscribe */
  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn)
    return () => {
      this.listeners.delete(fn)
    }
  }

  /** Referentially-stable snapshot — a fresh object per call would loop the store */
  getSnapshot = (): GridSnapshot => {
    if (!this.snap)
      this.snap = {
        active: this._active,
        anchor: this._anchor,
        editing: this._editing,
        editBuf: this._editBuf,
      }
    return this.snap
  }

  constructor(private opts: GridControllerOpts) {}

  /** Commit cells through the parent, recording history (record=false replays) */
  private applyEdits(cells: { r: number; c: number; value: string }[], record = true) {
    if (cells.length === 0) return
    if (record) {
      const batch = cells
        .map((x) => ({ r: x.r, c: x.c, prev: this.opts.cellText(x.r, x.c), next: x.value }))
        .filter((b) => b.prev !== b.next)
      if (batch.length) {
        this.undoStack.push(batch)
        this.redoStack = []
      }
    }
    if (cells.length === 1) this.opts.commit(cells[0].r, cells[0].c, cells[0].value)
    else this.opts.commitMany(cells)
  }

  // Undo/redo history of committed edits. Each entry is one user action
  // (single edit, paste, or clear) as a batch of per-cell before/after values.
  private undoStack: { r: number; c: number; prev: string; next: string }[][] = []
  private redoStack: { r: number; c: number; prev: string; next: string }[][] = []

  private replay(
    batch: { r: number; c: number; prev: string; next: string }[],
    key: 'prev' | 'next',
  ) {
    this.applyEdits(
      batch.map((b) => ({ r: b.r, c: b.c, value: b[key] })),
      false,
    )
    const f = batch[0]
    this.active = { r: f.r, c: f.c }
    this.anchor = { ...this.active }
    this.scrollActiveIntoView()
  }
  /** Drop history — call when the row set's identity changes (node/filter) */
  resetHistory() {
    this.undoStack = []
    this.redoStack = []
  }
  undo() {
    const batch = this.undoStack.pop()
    if (!batch) return
    this.redoStack.push(batch)
    this.replay(batch, 'prev')
  }
  redo() {
    const batch = this.redoStack.pop()
    if (!batch) return
    this.undoStack.push(batch)
    this.replay(batch, 'next')
  }

  private rangeR(): [number, number] {
    return [Math.min(this.anchor.r, this.active.r), Math.max(this.anchor.r, this.active.r)]
  }
  private rangeC(): [number, number] {
    return [Math.min(this.anchor.c, this.active.c), Math.max(this.anchor.c, this.active.c)]
  }
  inSelection(r: number, c: number): boolean {
    const [aR, bR] = this.rangeR()
    const [aC, bC] = this.rangeC()
    return r >= aR && r <= bR && c >= aC && c <= bC
  }
  isActive(r: number, c: number): boolean {
    return this.active.r === r && this.active.c === c
  }
  isEditing(r: number, c: number): boolean {
    return this.editing?.r === r && this.editing?.c === c
  }

  // Re-clamp after the underlying data shrinks. Only writes when a coord
  // actually moves — a fresh {r,c} every run would loop the caller's effect.
  clampToBounds() {
    const maxR = Math.max(0, this.opts.rows() - 1)
    const maxC = Math.max(0, this.opts.cols() - 1)
    const ar = clamp(this.active.r, 0, maxR)
    const ac = clamp(this.active.c, 0, maxC)
    if (ar !== this.active.r || ac !== this.active.c) this.active = { r: ar, c: ac }
    const nr = clamp(this.anchor.r, 0, maxR)
    const nc = clamp(this.anchor.c, 0, maxC)
    if (nr !== this.anchor.r || nc !== this.anchor.c) this.anchor = { r: nr, c: nc }
  }

  private scrollActiveIntoView() {
    queueMicrotask(() => {
      document
        .querySelector(`[data-cell="${this.active.r}-${this.active.c}"]`)
        ?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
    })
  }

  selectCell = (r: number, c: number, extend: boolean) => {
    this.active = { r, c }
    if (!extend) this.anchor = { r, c }
    this.opts.focusSink()
  }

  /** Extend the selection to (r,c) keeping the anchor — used by mouse drag */
  extendTo = (r: number, c: number) => {
    const nr = clamp(r, 0, this.opts.rows() - 1)
    const nc = clamp(c, 0, this.opts.cols() - 1)
    if (nr !== this.active.r || nc !== this.active.c) this.active = { r: nr, c: nc }
  }

  private move(dr: number, dc: number, extend: boolean) {
    const r = clamp(this.active.r + dr, 0, this.opts.rows() - 1)
    const c = clamp(this.active.c + dc, 0, this.opts.cols() - 1)
    this.active = { r, c }
    if (!extend) this.anchor = { r, c }
    this.scrollActiveIntoView()
  }

  // ── inline edit ──────────────────────────────────────────────────────────
  beginEdit(r: number, c: number, initial?: string) {
    if (!this.opts.isEditable(r, c)) return
    this.active = { r, c }
    this.anchor = { r, c }
    this.editing = { r, c }
    this.editSeeded = initial !== undefined
    this.editBuf =
      initial !== undefined ? initial : (this.opts.editText ?? this.opts.cellText)(r, c)
  }
  // Set whenever an edit is closed programmatically (commit/cancel) so the
  // input's unmount-triggered blur doesn't double-commit or override an Esc.
  private editClosing = false
  private doCommit(moveDir: 0 | 1 | -1) {
    if (!this.editing) return
    const { r, c } = this.editing
    this.editClosing = true
    this.applyEdits([{ r, c, value: this.editBuf }])
    this.editing = null
    this.opts.focusSink()
    if (moveDir) this.move(moveDir, 0, false)
  }
  /** Commit an explicit value (picker/選択 UI) — the buffer is set by choosing,
   *  not by typing, so the editor never had focus to blur-commit from */
  commitValue(value: string) {
    this.editBuf = value
    this.doCommit(0)
  }

  /** Esc: discard the in-progress edit and restore the cell's prior value */
  cancelEdit() {
    this.editClosing = true
    this.editing = null
    this.opts.focusSink()
  }
  /** Editor blur (click/tab away): commit — unless we're closing via Enter/Esc */
  onBlur = () => {
    if (this.editClosing) {
      this.editClosing = false
      return
    }
    this.doCommit(0)
  }
  onInputKeydown = (e: GridKeyEvent) => {
    if (composing(e)) return // let the IME finish; Enter confirms composition, not the cell
    if (e.key === 'Enter') {
      e.preventDefault()
      this.doCommit(1)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      this.cancelEdit()
    } else if (e.key === 'Tab') {
      e.preventDefault()
      this.doCommit(0)
      this.move(0, e.shiftKey ? -1 : 1, false)
    }
    e.stopPropagation()
  }

  // ── keyboard (focus-sink textarea) ───────────────────────────────────────
  onKeydown = (e: GridKeyEvent) => {
    if (this.editing || composing(e)) return
    const mod = e.ctrlKey || e.metaKey
    if (mod) {
      // Undo/redo; Ctrl/Cmd+Home/End jump to grid extremes; copy/paste fall
      // through to the native clipboard events on the textarea.
      const k = e.key.toLowerCase()
      if (k === 'z' && !e.shiftKey) {
        e.preventDefault()
        this.undo()
        return
      }
      if (k === 'y' || (k === 'z' && e.shiftKey)) {
        e.preventDefault()
        this.redo()
        return
      }
      if (e.key === 'Home') {
        e.preventDefault()
        this.active = { r: 0, c: 0 }
        if (!e.shiftKey) this.anchor = { r: 0, c: 0 }
        this.scrollActiveIntoView()
      } else if (e.key === 'End') {
        e.preventDefault()
        const r = this.opts.rows() - 1,
          c = this.opts.cols() - 1
        this.active = { r, c }
        if (!e.shiftKey) this.anchor = { r, c }
        this.scrollActiveIntoView()
      }
      return
    }
    const page = this.opts.pageSize?.() ?? 10
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        this.move(1, 0, e.shiftKey)
        break
      case 'ArrowUp':
        e.preventDefault()
        this.move(-1, 0, e.shiftKey)
        break
      case 'ArrowRight':
        e.preventDefault()
        this.move(0, 1, e.shiftKey)
        break
      case 'ArrowLeft':
        e.preventDefault()
        this.move(0, -1, e.shiftKey)
        break
      case 'PageDown':
        e.preventDefault()
        this.move(page, 0, e.shiftKey)
        break
      case 'PageUp':
        e.preventDefault()
        this.move(-page, 0, e.shiftKey)
        break
      case 'Home':
        e.preventDefault()
        this.move(0, -this.opts.cols(), e.shiftKey)
        break
      case 'End':
        e.preventDefault()
        this.move(0, this.opts.cols(), e.shiftKey)
        break
      case 'Enter':
      case 'F2':
        e.preventDefault()
        this.beginEdit(this.active.r, this.active.c)
        break
      case 'Escape':
        e.preventDefault()
        this.anchor = { ...this.active }
        break
      case 'Delete':
      case 'Backspace':
        e.preventDefault()
        this.clearSelection()
        break
      default:
        if (e.key.length === 1 && !e.altKey) {
          e.preventDefault()
          // Type-to-replace — but a seed key the cell's type filter rejects
          // (a letter into a numeric cell) must not even open the editor.
          if (this.opts.inputFilter && !this.opts.inputFilter(this.active.r, this.active.c, e.key))
            return
          this.beginEdit(this.active.r, this.active.c, e.key)
        }
    }
  }

  /** Clear every editable cell in the selection (Delete/Backspace) */
  private clearSelection() {
    const [aR, bR] = this.rangeR()
    const [aC, bC] = this.rangeC()
    const cells: { r: number; c: number; value: string }[] = []
    for (let r = aR; r <= bR; r++)
      for (let c = aC; c <= bC; c++) if (this.opts.isEditable(r, c)) cells.push({ r, c, value: '' })
    this.applyEdits(cells)
  }

  // ── clipboard (Excel rectangular TSV) ────────────────────────────────────
  private selectionTsv(): string {
    const [aR, bR] = this.rangeR()
    const [aC, bC] = this.rangeC()
    const lines: string[] = []
    for (let r = aR; r <= bR; r++) {
      const cells: string[] = []
      for (let c = aC; c <= bC; c++) cells.push(this.opts.cellText(r, c))
      lines.push(cells.join('\t'))
    }
    return lines.join('\n')
  }
  onCopy = (e: GridClipboardEvent) => {
    if (this.editing) return
    e.preventDefault()
    const tsv = this.selectionTsv()
    if (e.clipboardData) e.clipboardData.setData('text/plain', tsv)
    else void copyText(tsv)
  }
  onPaste = (e: GridClipboardEvent) => {
    if (this.editing) return
    e.preventDefault()
    this.applyPaste(e.clipboardData?.getData('text/plain') ?? '')
  }
  private applyPaste(text: string) {
    const matrix = text
      .replace(/\r\n?/g, '\n')
      .split('\n')
      .map((l) => l.split('\t'))
    while (matrix.length > 1 && matrix[matrix.length - 1].every((s) => s === '')) matrix.pop()
    const nRows = this.opts.rows()
    const nCols = this.opts.cols()
    const cells: { r: number; c: number; value: string }[] = []
    let skipped = 0
    // Type-safety gate: only write a pasted value into an editable cell if it
    // passes that cell's coercePaste (e.g. a name string is REJECTED for a
    // numeric value cell), so a stray/misaligned copy can't corrupt a field.
    const accept = (r: number, c: number, raw: string) => {
      if (!this.opts.isEditable(r, c)) return
      const v = this.opts.coercePaste ? this.opts.coercePaste(r, c, raw) : raw
      if (v === null) {
        skipped++
        return
      }
      cells.push({ r, c, value: v })
    }

    // A single copied value fills the whole current selection (Excel behaviour).
    const single = matrix.length === 1 && matrix[0].length === 1
    if (single) {
      const [aR, bR] = this.rangeR()
      const [aC, bC] = this.rangeC()
      for (let r = aR; r <= bR; r++) for (let c = aC; c <= bC; c++) accept(r, c, matrix[0][0])
      if (cells.length) this.applyEdits(cells)
      if (skipped) this.opts.onPasteSkipped?.(skipped)
      return
    }

    // Multi-cell block pastes from the selection's top-left (Excel behaviour),
    // not the active cell (which, after a drag, sits at the drag's end).
    const startR = Math.min(this.anchor.r, this.active.r)
    const startC = Math.min(this.anchor.c, this.active.c)
    let lastR = startR
    let lastC = startC
    for (let dr = 0; dr < matrix.length; dr++) {
      const r = startR + dr
      if (r >= nRows) break
      for (let dc = 0; dc < matrix[dr].length; dc++) {
        const c = startC + dc
        if (c >= nCols) break
        lastR = r
        lastC = Math.max(lastC, c)
        accept(r, c, matrix[dr][dc])
      }
    }
    if (skipped) this.opts.onPasteSkipped?.(skipped)
    if (cells.length === 0) return
    this.applyEdits(cells)
    this.anchor = { r: startR, c: startC }
    this.active = { r: clamp(lastR, 0, nRows - 1), c: clamp(lastC, 0, nCols - 1) }
    this.scrollActiveIntoView()
  }
}
