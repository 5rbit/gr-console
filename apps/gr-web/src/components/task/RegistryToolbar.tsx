// 레지스트리 툴바 — 추가/편집/삭제 + PLC 읽기·쓰기·차이 + Excel 내보내기·가져오기.
//
// 세 레지스트리(품목·셀·스테이션)가 같은 띠를 쓴다. PLC 조작은 셀·스테이션에만 있다(품목은 콘솔 전용).
// 다이얼로그 상태(쓰기 확인·차이·가져오기 미리보기)는 이 컴포넌트가 들고, 결과는 토스트 + 다이얼로그로 낸다.
import type * as React from 'react'
import { useRef, useState } from 'react'
import {
  Download,
  FileDiff,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  UploadCloud,
} from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { Select } from '../../lib/ui/Select'
import { Toolbar } from '../../lib/ui/Toolbar'
import { toast } from '../../lib/ui/toast'
import type { DiffRow } from '../../lib/types'
import type { FileImportResult, PlcTarget, PushResult } from '../../lib/task/types'
import {
  DiffDialog,
  ImportDialog,
  PLC_TARGET_LABEL,
  PushDialog,
  pushSummary,
} from './registryDialogs'

/** 한 레지스트리의 PLC/Excel 백엔드 묶음. 품목은 `plc`가 없다. */
export interface RegistryIo<T> {
  plc?: {
    import: (plc: PlcTarget) => Promise<FileImportResult>
    push: (plc: PlcTarget, force: boolean) => Promise<PushResult>
    diff: (plc: PlcTarget) => Promise<DiffRow<T>[]>
    summarize: (v: T) => string
  }
  exportUrl: string
  importFile: (file: File, dryRun: boolean) => Promise<FileImportResult>
}

export interface RegistryToolbarProps<T> {
  /** 좁은 사이드바 — 버튼을 아이콘만으로(글자는 title). */
  compact?: boolean
  /** 행 추가·편집·삭제를 숨긴다(그리드 편집기가 맡을 때). */
  hideCrud?: boolean
  title: string
  icon: React.ReactNode
  what: '셀' | '스테이션' | '품목'
  rows: number
  dirty: number
  selected: boolean
  io: RegistryIo<T>
  onAdd: () => void
  onEdit: () => void
  onDelete: () => void
  /** 목록을 다시 받는다(읽기·가져오기·쓰기 뒤). */
  reload: () => Promise<void>
}

export function RegistryToolbar<T>({
  title,
  icon,
  what,
  rows,
  dirty,
  selected,
  io,
  onAdd,
  onEdit,
  onDelete,
  reload,
  compact = false,
  hideCrud = false,
}: RegistryToolbarProps<T>) {
  const [plc, setPlc] = useState<PlcTarget>('GR2')
  const [busy, setBusy] = useState<'import' | 'push' | 'diff' | 'file' | null>(null)
  const [pushOpen, setPushOpen] = useState(false)
  const [diff, setDiff] = useState<{
    open: boolean
    rows: DiffRow<T>[] | null
    error: string | null
    plc: PlcTarget
  }>({ open: false, rows: null, error: null, plc: 'GR2' })
  const [imp, setImp] = useState<{
    open: boolean
    file: File | null
    preview: FileImportResult | null
    error: string | null
    applying: boolean
  }>({ open: false, file: null, preview: null, error: null, applying: false })
  const fileRef = useRef<HTMLInputElement>(null)
  /** 읽기·차이는 PLC 한 대만 — '둘 다'면 GR2로 본다. */
  const one: PlcTarget = plc === 'both' ? 'GR2' : plc

  async function doImport() {
    if (!io.plc) return
    setBusy('import')
    const tid = toast.pending(`${one}에서 ${what} 읽는 중…`)
    try {
      const r = await io.plc.import(one)
      await reload()
      toast.resolve(
        tid,
        'ok',
        `${one} ${what} 읽기 — 추가 ${r.imported} · 갱신 ${r.updated} · 동일 ${r.skipped}`,
      )
    } catch (e) {
      toast.resolve(
        tid,
        'error',
        `${what} 읽기 실패 — ${e instanceof Error ? e.message : String(e)}`,
      )
    } finally {
      setBusy(null)
    }
  }

  async function doPush(force: boolean) {
    if (!io.plc) return
    setBusy('push')
    const tid = toast.pending(`${PLC_TARGET_LABEL[plc]}에 ${what} 쓰는 중…`)
    try {
      const r = await io.plc.push(plc, force)
      await reload()
      toast.resolve(tid, r.verified ? 'ok' : 'warn', pushSummary(r))
    } catch (e) {
      toast.resolve(
        tid,
        'error',
        `${what} 쓰기 실패 — ${e instanceof Error ? e.message : String(e)}`,
      )
    } finally {
      setBusy(null)
    }
  }

  async function doDiff() {
    if (!io.plc) return
    setBusy('diff')
    setDiff({ open: true, rows: null, error: null, plc: one })
    try {
      const rows = await io.plc.diff(one)
      setDiff({ open: true, rows, error: null, plc: one })
    } catch (e) {
      setDiff({ open: true, rows: [], error: e instanceof Error ? e.message : String(e), plc: one })
    } finally {
      setBusy(null)
    }
  }

  async function pickFile(file: File | null) {
    if (!file) return
    setImp({ open: true, file, preview: null, error: null, applying: false })
    setBusy('file')
    try {
      const preview = await io.importFile(file, true)
      setImp((s) => ({ ...s, preview }))
    } catch (e) {
      setImp((s) => ({ ...s, error: e instanceof Error ? e.message : String(e) }))
    } finally {
      setBusy(null)
    }
  }

  async function applyFile() {
    if (!imp.file) return
    setImp((s) => ({ ...s, applying: true }))
    try {
      const r = await io.importFile(imp.file, false)
      await reload()
      toast.ok(
        `${what} 가져오기 — 추가 ${r.imported} · 갱신 ${r.updated} · 동일 ${r.skipped} · 오류 ${r.errors.length}`,
      )
      setImp({ open: false, file: null, preview: null, error: null, applying: false })
    } catch (e) {
      setImp((s) => ({ ...s, applying: false, error: e instanceof Error ? e.message : String(e) }))
    }
  }

  return (
    <>
      <Toolbar
        icon={icon}
        title={title}
        dense
        meta={
          <span className="tabular-nums">
            {rows}건{dirty ? ` · 수정 ${dirty}` : ''}
          </span>
        }
      >
        {hideCrud ? null : (
          <Button
            size="sm"
            intent="ghost"
            icon={<Plus className="h-3.5 w-3.5" />}
            onClick={onAdd}
            data-testid="reg-add"
            title="추가"
          >
            {compact ? null : '추가'}
          </Button>
        )}
        {hideCrud ? null : (
          <Button
            size="sm"
            intent="ghost"
            icon={<Pencil className="h-3.5 w-3.5" />}
            disabled={!selected}
            onClick={onEdit}
            data-testid="reg-edit"
            title="편집"
          >
            {compact ? null : '편집'}
          </Button>
        )}
        {hideCrud ? null : (
          <Button
            size="sm"
            intent="ghost"
            icon={<Trash2 className="h-3.5 w-3.5" />}
            disabled={!selected}
            onClick={onDelete}
            data-testid="reg-delete"
            title="삭제"
          >
            {compact ? null : '삭제'}
          </Button>
        )}
        {io.plc ? (
          <>
            <span className="mx-1 h-4 w-px bg-line-strong" />
            <Select
              dense
              value={plc}
              onValueChange={(v) => setPlc(v as PlcTarget)}
              aria-label="PLC 대상"
              data-testid="reg-plc"
            >
              <option value="GR2">GR2</option>
              <option value="GRM">GRM</option>
              <option value="both">둘 다(쓰기)</option>
            </Select>
            <Button
              size="sm"
              icon={<Download className="h-3.5 w-3.5" />}
              loading={busy === 'import'}
              disabled={busy !== null}
              onClick={() => void doImport()}
              title={`${one} PLC 테이블을 로컬로 읽어 옵니다`}
              data-testid="reg-plc-read"
            >
              {compact ? null : 'PLC 읽기'}
            </Button>
            <Button
              size="sm"
              intent="danger"
              icon={<UploadCloud className="h-3.5 w-3.5" />}
              loading={busy === 'push'}
              disabled={busy !== null || rows === 0}
              onClick={() => setPushOpen(true)}
              title={`로컬 테이블을 ${PLC_TARGET_LABEL[plc]}에 씁니다`}
              data-testid="reg-plc-write"
            >
              {compact ? null : 'PLC 쓰기'}
            </Button>
            <Button
              size="sm"
              icon={<FileDiff className="h-3.5 w-3.5" />}
              loading={busy === 'diff'}
              disabled={busy !== null}
              onClick={() => void doDiff()}
              data-testid="reg-diff"
              title="차이 보기"
            >
              {compact ? null : '차이 보기'}
            </Button>
          </>
        ) : null}
        <span className="mx-1 h-4 w-px bg-line-strong" />
        <a
          href={io.exportUrl}
          download
          className="inline-flex h-7 items-center gap-1.5 rounded-md bg-surface-inset px-2 text-xs font-medium text-content-primary hover:bg-surface-active"
          data-testid="reg-export"
          title="Excel 내보내기"
        >
          <Download className="h-3.5 w-3.5" />
          {compact ? null : 'Excel 내보내기'}
        </a>
        <Button
          size="sm"
          icon={<Upload className="h-3.5 w-3.5" />}
          loading={busy === 'file'}
          disabled={busy !== null}
          onClick={() => fileRef.current?.click()}
          data-testid="reg-import"
          title="Excel 가져오기"
        >
          {compact ? null : 'Excel 가져오기'}
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".xlsx,.xlsm,.xls,.csv"
          className="hidden"
          onChange={(e) => {
            const f = e.currentTarget.files?.[0] ?? null
            e.currentTarget.value = ''
            void pickFile(f)
          }}
        />
        <Button
          size="icon-sm"
          intent="ghost"
          aria-label="새로고침"
          title="목록 새로고침"
          onClick={() => void reload()}
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </Button>
      </Toolbar>

      {io.plc ? (
        <>
          <PushDialog
            open={pushOpen}
            onOpenChange={setPushOpen}
            what={what === '품목' ? '셀' : what}
            plc={plc}
            rows={rows}
            dirty={dirty}
            onConfirm={(force) => void doPush(force)}
          />
          <DiffDialog
            open={diff.open}
            onOpenChange={(o) => setDiff((s) => ({ ...s, open: o }))}
            title={`${what} — 로컬 vs ${diff.plc}`}
            rows={diff.rows}
            error={diff.error}
            summarize={io.plc.summarize}
          />
        </>
      ) : null}
      <ImportDialog
        open={imp.open}
        onOpenChange={(o) => setImp((s) => ({ ...s, open: o }))}
        file={imp.file}
        preview={imp.preview}
        error={imp.error}
        applying={imp.applying}
        onApply={() => void applyFile()}
      />
    </>
  )
}
