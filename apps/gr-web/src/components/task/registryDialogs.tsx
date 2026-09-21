// 레지스트리 툴바가 띄우는 다이얼로그 셋 — PLC 쓰기 확인, 차이 보기, Excel 가져오기 미리보기.
//
// 셋 다 **결과를 화면에 남긴다**: 쓰기는 재읽기 검증(바이트 비교) 결과를, 가져오기는 dry-run 카운트와
// 행별 오류를 보인 뒤에야 적용 버튼을 준다. 토스트는 한 줄 요약이고 상세는 여기다.
import { useEffect, useState } from 'react'
import { Upload } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Column } from '../../lib/ui/table'
import type { Status } from '../../lib/ui/status'
import type { DiffRow } from '../../lib/types'
import type { FileImportResult, PushResult } from '../../lib/task/types'

// ── PLC 쓰기 확인 ──────────────────────────────────────────────────────────────

export interface PushDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  what: '셀' | '스테이션'
  /** 대상 표시 이름(`GR2` · `GR1 + GR2 + GRM`) — `lib/task/plcTarget.ts`의 `targetLabel`. */
  plc: string
  rows: number
  dirty: number
  /** `force`(AUTO 모드여도 쓰기)와 함께 실행한다. */
  onConfirm: (force: boolean) => void
}

export function PushDialog({
  open,
  onOpenChange,
  what,
  plc,
  rows,
  dirty,
  onConfirm,
}: PushDialogProps) {
  const [force, setForce] = useState(false)
  useEffect(() => {
    if (open) setForce(false)
  }, [open])
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      scope="single-robot"
      danger
      title={`${what} 테이블 쓰기`}
      confirmLabel="PLC에 쓰기"
      onConfirm={() => onConfirm(force)}
    >
      <div className="flex flex-col gap-2 text-xs">
        <p className="m-0 flex items-center gap-1">
          {plc} 에 쓸까요?
          <HelpTip
            title="쓰는 순서"
            text={`로컬 ${what} 를 Id 오름차순으로 PLC DB 에 쓰고 나머지 슬롯은 0 으로 채웁니다. Count 는 마지막에 쓰고, 쓴 뒤 다시 읽어 바이트를 대조합니다.`}
          />
        </p>
        <FieldList
          columns={2}
          dense
          labelWidth={48}
          items={[
            { label: '대상', value: plc },
            { label: '종류', value: what },
            { label: '행', value: `${rows}건` },
            { label: '수정', value: `${dirty}건` },
          ]}
        />
        <p className="m-0 text-fault-fg">PLC 의 기존 테이블은 되돌릴 수 없습니다.</p>
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={force}
            onChange={(e) => setForce(e.currentTarget.checked)}
            data-testid="push-force"
          />
          AUTO 모드여도 강제로 쓰기
          <HelpTip
            title="강제 쓰기"
            text="GR PLC 가 AUTO 모드일 때도 씁니다 — 작업 검증 중에 테이블이 바뀔 수 있습니다."
          />
        </label>
      </div>
    </ConfirmDialog>
  )
}

/** 쓰기 결과 요약 문장(토스트·배너 공용). */
export function pushSummary(r: PushResult): string {
  const parts = r.results && r.results.length > 1 ? r.results : [r]
  return parts
    .map((p) =>
      p.verified
        ? `${p.plc} ${p.db} ${p.count}건 ${p.written_bytes}B 검증 OK`
        : `${p.plc} ${p.db} 검증 실패 (offset ${p.mismatch_at ?? '?'})`,
    )
    .join(' · ')
}

// ── 차이 보기 ──────────────────────────────────────────────────────────────────

const DIFF_LABEL: Record<DiffRow<unknown>['status'], { text: string; tone: Status }> = {
  same: { text: 'PLC와 동일', tone: 'ok' },
  changed: { text: '값 다름', tone: 'warn' },
  local_only: { text: '로컬에만', tone: 'info' },
  plc_only: { text: 'PLC에만', tone: 'degraded' },
}

export function DiffBadge({ status }: { status: DiffRow<unknown>['status'] }) {
  const l = DIFF_LABEL[status]
  if (status === 'same') return <span className="text-2xs text-content-faint">{l.text}</span>
  return <StatusBadge status={l.tone}>{l.text}</StatusBadge>
}

export interface DiffDialogProps<T> {
  open: boolean
  onOpenChange: (o: boolean) => void
  title: string
  rows: DiffRow<T>[] | null
  error: string | null
  /** 한 쪽 값을 한 줄로 — 셀이면 `S2 R1 C1 · 12000/3000/1500`. */
  summarize: (v: T) => string
}

export function DiffDialog<T>({
  open,
  onOpenChange,
  title,
  rows,
  error,
  summarize,
}: DiffDialogProps<T>) {
  const [onlyDiff, setOnlyDiff] = useState(true)
  const shown = (rows ?? []).filter((r) => !onlyDiff || r.status !== 'same')
  const counts = (rows ?? []).reduce(
    (a, r) => ({ ...a, [r.status]: (a[r.status] ?? 0) + 1 }),
    {} as Partial<Record<DiffRow<T>['status'], number>>,
  )
  const columns: Column<DiffRow<T>>[] = [
    { key: 'id', label: 'Id', get: (r) => r.id, numeric: true },
    {
      key: 'status',
      label: 'Status',
      get: (r) => r.status,
      cell: (r) => <DiffBadge status={r.status} />,
    },
    {
      key: 'local',
      label: 'Local',
      get: (r) => (r.local ? summarize(r.local) : null),
      cell: (r) =>
        r.local ? (
          <span className="font-mono text-2xs">{summarize(r.local)}</span>
        ) : (
          <span className="text-content-faint">—</span>
        ),
    },
    {
      key: 'plc',
      label: 'PLC',
      get: (r) => (r.plc ? summarize(r.plc) : null),
      cell: (r) =>
        r.plc ? (
          <span className="font-mono text-2xs">{summarize(r.plc)}</span>
        ) : (
          <span className="text-content-faint">—</span>
        ),
    },
  ]
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="lg"
      meta={`${(rows ?? []).length}행`}
      footer={
        <Button size="sm" intent="ghost" onClick={() => onOpenChange(false)}>
          닫기
        </Button>
      }
      testid="diff-dialog"
    >
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2 text-xs">
          {(['same', 'changed', 'local_only', 'plc_only'] as const).map((s) => (
            <span key={s} className="inline-flex items-center gap-1">
              <StatusDot status={DIFF_LABEL[s].tone} size="sm" label={DIFF_LABEL[s].text} />
              <span className="tabular-nums text-content-muted">{counts[s] ?? 0}</span>
            </span>
          ))}
          <span className="flex-1" />
          <label className="flex items-center gap-1">
            <input
              type="checkbox"
              checked={onlyDiff}
              onChange={(e) => setOnlyDiff(e.currentTarget.checked)}
            />
            다른 것만
          </label>
        </div>
        {error ? <p className="m-0 text-xs text-fault-fg">{error}</p> : null}
        <DataTable
          rows={shown}
          columns={columns}
          rowKey={(r) => String(r.id)}
          loading={rows === null && !error}
          empty={onlyDiff ? '차이 없음' : '행 없음'}
          emptyHint={
            onlyDiff ? '로컬과 PLC 가 같습니다 — 체크를 끄면 같은 행도 봅니다.' : undefined
          }
          testid="diff-table"
        />
      </div>
    </Dialog>
  )
}

// ── Excel 가져오기 미리보기 ──────────────────────────────────────────────────

export interface ImportDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  file: File | null
  /** dry-run 결과(`null` = 아직 조회 중). */
  preview: FileImportResult | null
  error: string | null
  applying: boolean
  onApply: () => void
}

export function ImportDialog({
  open,
  onOpenChange,
  file,
  preview,
  error,
  applying,
  onApply,
}: ImportDialogProps) {
  const canApply = !!preview && preview.imported + preview.updated > 0
  const errCols: Column<FileImportResult['errors'][number]>[] = [
    { key: 'row', label: 'Row', get: (e) => e.row, numeric: true },
    { key: 'sheet', label: 'Sheet', get: (e) => e.sheet ?? '' },
    { key: 'message', label: 'Message', get: (e) => e.message },
  ]
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Excel 가져오기 — 미리보기"
      size="lg"
      meta={
        <>
          <span className="truncate font-mono">{file?.name ?? ''}</span>
          {preview?.counts ? (
            <span className="whitespace-nowrap tabular-nums">
              셀 {preview.counts.cells} · 스테이션 {preview.counts.stations} · 품목{' '}
              {preview.counts.items}
            </span>
          ) : null}
        </>
      }
      footer={
        <>
          <Button size="sm" intent="ghost" onClick={() => onOpenChange(false)}>
            취소
          </Button>
          <Button
            intent="primary"
            size="sm"
            icon={<Upload className="h-3.5 w-3.5" />}
            disabled={!canApply}
            title={canApply ? undefined : '적용할 행이 없습니다'}
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
      <div className="flex flex-col gap-3 text-xs">
        {error ? <p className="m-0 text-fault-fg">{error}</p> : null}
        {preview ? (
          <div className="grid grid-cols-4 gap-2" data-testid="import-preview">
            <span className="sr-only">dry-run 결과</span>
            {[
              ['Imported', preview.imported],
              ['Updated', preview.updated],
              ['Skipped', preview.skipped],
              ['Errors', preview.errors.length],
            ].map(([l, v]) => (
              <div key={String(l)} className="rounded-md border border-line-default px-2 py-1">
                <div className="text-3xs text-content-faint">{l}</div>
                <div className="text-sm font-semibold tabular-nums">{v}</div>
              </div>
            ))}
          </div>
        ) : !error ? (
          <p className="m-0 text-content-faint">미리보기 계산 중…</p>
        ) : null}
        {preview && preview.errors.length > 0 ? (
          <DataTable
            rows={preview.errors}
            columns={errCols}
            rowKey={(e) => `${e.sheet ?? ''}:${e.row}:${e.message}`}
            testid="import-errors"
          />
        ) : null}
        <span className="flex items-center gap-1 text-2xs text-content-muted">
          로컬에만 적용
          <HelpTip
            title="적용 범위"
            text="오류 행은 건너뛰고 나머지만 로컬 사본에 적용합니다. PLC 에는 쓰지 않습니다 — 적용 뒤 PLC 쓰기로 반영하세요."
          />
        </span>
      </div>
    </Dialog>
  )
}
