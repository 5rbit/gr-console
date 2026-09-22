// Excel 가져오기 — 파일 고르기(또는 끌어다 놓기) → dry-run 미리보기 → 적용.
//
// 미리보기는 **줄 결과 넷**(추가 · 갱신 · 동일 · 건너뜀)과 문제 줄 표다. 적용은 쓸 줄이 있을 때만 되고,
// 막힐 때는 왜 막혔는지 말한다(`lib/task/importPreview.ts`). 토스트는 한 줄 요약이고 상세는 여기다.
//
// 파일 없이도 열린다: 메뉴의 "Excel 가져오기"는 이 창과 파일 고르기 창을 같이 연다 — 고르기를 취소해도
// 창은 남아서 탐색기에서 끌어다 놓을 자리가 된다. 예전에는 파일을 골라야만 창이 떴고, 그래서 끌어다
// 놓을 곳이 없었다.
import { useState, type DragEvent, type ReactNode } from 'react'
import { FileSpreadsheet, Upload } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { HelpTip, type HelpSection } from '../../lib/ui/HelpTip'
import { StatRow } from '../../lib/ui/StatRow'
import type { Column } from '../../lib/ui/table'
import {
  IMPORT_SCOPE_HELP,
  applyBlockedReason,
  importOutcome,
  isSheetFile,
  outcomeStats,
  problemRows,
  sheetCountsText,
  type ProblemRow,
} from '../../lib/task/importPreview'
import type { FileImportResult } from '../../lib/task/types'
import { cn } from '../../lib/utils'

export interface ImportDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  /** 제목에 붙는 대상(`품목` · `셀` · `스테이션`). */
  what?: string
  file: File | null
  /** dry-run 결과(`null` = 아직 없음). */
  preview: FileImportResult | null
  error: string | null
  applying: boolean
  /** 파일 고르기 창을 연다. */
  onPick: () => void
  /** 끌어다 놓은 파일 — 표 파일일 때만 부른다. */
  onFile: (f: File) => void
  onApply: () => void
  /** 빈 양식 주소 — 있으면 창 안에 `양식 받기`가 선다(처음 채우는 사람이 창을 닫고 메뉴로 돌아가지 않게). */
  templateUrl?: string
  /** `?` 본문 — 없으면 적용 범위 설명만. */
  help?: readonly HelpSection[]
  /** 파일 칸 아래에 서는 추가 조작(재고의 병합/교체 고르기 등). */
  extra?: ReactNode
  /** 바닥 띠 왼쪽 문구(기본 `로컬에만 적용`). */
  scopeLabel?: string
}

const PROBLEM_COLS: Column<ProblemRow>[] = [
  { key: 'row', label: 'Row', get: (e) => e.row, numeric: true, class: 'font-mono' },
  { key: 'sheet', label: 'Sheet', get: (e) => e.sheet },
  { key: 'message', label: 'Message', get: (e) => e.message },
]

const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer.types).includes('Files')

export function ImportDialog({
  open,
  onOpenChange,
  what,
  file,
  preview,
  error,
  applying,
  onPick,
  onFile,
  onApply,
  templateUrl,
  help = IMPORT_SCOPE_HELP,
  extra,
  scopeLabel = '로컬에만 적용',
}: ImportDialogProps) {
  const [over, setOver] = useState(false)
  const [dropError, setDropError] = useState<string | null>(null)
  const outcome = preview ? importOutcome(preview) : null
  const blocked = applyBlockedReason(preview, { file: !!file, failed: !!error })
  const problems = problemRows(preview)
  const checking = !!file && !preview && !error

  function drop(e: DragEvent<HTMLDivElement>) {
    if (!hasFiles(e)) return
    e.preventDefault()
    setOver(false)
    const f = e.dataTransfer.files[0]
    if (!f) return
    if (!isSheetFile(f.name)) {
      setDropError(`${f.name} — xlsx · xls · csv 파일만 받습니다`)
      return
    }
    setDropError(null)
    onFile(f)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`${what ? `${what} ` : ''}Excel 가져오기`}
      size="lg"
      meta={
        preview ? (
          <span className="whitespace-nowrap tabular-nums">{sheetCountsText(preview.counts)}</span>
        ) : null
      }
      footer={
        <>
          <span className="mr-auto flex items-center gap-1 text-2xs text-content-muted">
            {scopeLabel}
            <HelpTip title="Excel 가져오기" sections={help} align="right" />
          </span>
          <Button size="sm" intent="ghost" onClick={() => onOpenChange(false)}>
            취소
          </Button>
          <Button
            intent="primary"
            size="sm"
            icon={<Upload className="h-3.5 w-3.5" />}
            disabled={blocked !== null}
            title={blocked ?? undefined}
            loading={applying}
            onClick={onApply}
            data-testid="import-apply"
          >
            적용
          </Button>
        </>
      }
      testid="import-dialog"
    >
      <div
        className="flex flex-col gap-3 text-xs"
        onDragOver={(e) => {
          if (!hasFiles(e)) return
          e.preventDefault()
          setOver(true)
        }}
        onDragLeave={(e) => {
          // 자식 사이를 지날 때마다 leave 가 온다 — 창 밖으로 나갈 때만 끈다.
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOver(false)
        }}
        onDrop={drop}
      >
        <div
          className={cn(
            'flex flex-wrap items-center gap-2 rounded-md border border-dashed px-3 py-2',
            over ? 'border-accent bg-accent-soft' : 'border-line-strong',
          )}
          data-testid="import-drop"
        >
          <FileSpreadsheet className="h-4 w-4 flex-none text-content-muted" aria-hidden="true" />
          <span
            className={cn(
              'min-w-0 flex-1 truncate',
              file ? 'font-mono text-content-primary' : 'text-content-muted',
            )}
            title={file?.name}
          >
            {file ? file.name : over ? '놓으면 미리보기' : '파일을 여기로 끌어다 놓기'}
          </span>
          <Button
            size="sm"
            intent="outline"
            onClick={() => {
              setDropError(null)
              onPick()
            }}
            data-testid="import-pick"
          >
            {file ? '다른 파일' : '파일 고르기'}
          </Button>
          {templateUrl ? (
            <a
              href={templateUrl}
              download
              className="inline-flex h-control-sm items-center rounded-md px-2 text-xs font-medium text-content-muted hover:bg-surface-inset hover:text-content-primary focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none"
              title="Items · ItemBeadProfile 머리글만 든 빈 파일"
              data-testid="import-template"
            >
              양식 받기
            </a>
          ) : null}
        </div>

        {extra}

        {dropError || error ? (
          <span role="alert" className="text-fault-fg" data-testid="import-error">
            {dropError ?? error}
          </span>
        ) : null}
        {checking ? <span className="text-content-faint">미리보기 계산 중…</span> : null}

        {outcome ? (
          <StatRow
            bordered={false}
            className="px-0"
            testid="import-preview"
            items={outcomeStats(outcome).map((s) => ({
              label: s.label,
              value: s.value,
              tone: s.key === 'skipped' && s.value > 0 ? 'warn' : undefined,
              testid: `import-count-${s.key}`,
            }))}
          />
        ) : null}

        {preview?.warnings && preview.warnings.length > 0 ? (
          <ul
            className="m-0 list-disc rounded-md border border-warn bg-warn-soft py-1 pr-2 pl-6 text-2xs text-warn-fg"
            data-testid="import-warnings"
          >
            {preview.warnings.slice(0, 6).map((w) => (
              <li key={w}>{w}</li>
            ))}
            {preview.warnings.length > 6 ? <li>… 외 {preview.warnings.length - 6}건</li> : null}
          </ul>
        ) : null}

        {problems.length > 0 ? (
          <DataTable
            rows={problems}
            columns={PROBLEM_COLS}
            rowKey={(e) => e.key}
            density="compact"
            stickyHeader
            className="max-h-64 overflow-y-auto"
            testid="import-errors"
          />
        ) : null}
      </div>
    </Dialog>
  )
}
