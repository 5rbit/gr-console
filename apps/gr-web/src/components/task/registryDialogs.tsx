// 레지스트리 툴바가 띄우는 다이얼로그 셋 — PLC 쓰기 확인, 차이 보기, Excel 가져오기 미리보기.
//
// 셋 다 **결과를 화면에 남긴다**: 쓰기는 재읽기 검증(바이트 비교) 결과를, 가져오기는 dry-run 카운트와
// 행별 오류를 보인 뒤에야 적용 버튼을 준다. 토스트는 한 줄 요약이고 상세는 여기다.
import { useEffect, useState } from 'react'
import { Upload } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Modal } from '../../lib/ui/Modal'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Column } from '../../lib/ui/table'
import type { Status } from '../../lib/ui/status'
import type { DiffRow } from '../../lib/types'
import type { FileImportResult, PlcTarget, PushResult } from '../../lib/task/types'

export const PLC_TARGET_LABEL: Record<PlcTarget, string> = {
  GR2: 'GR2',
  GRM: 'GRM',
  both: 'GR2 + GRM',
}

// ── PLC 쓰기 확인 ──────────────────────────────────────────────────────────────

export interface PushDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  what: '셀' | '스테이션'
  plc: PlcTarget
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
      title={`${what} 테이블을 ${PLC_TARGET_LABEL[plc]}에 쓰기`}
      confirmLabel="PLC에 쓰기"
      onConfirm={() => onConfirm(force)}
    >
      <div className="flex flex-col gap-2 text-xs">
        <p>
          로컬 {what} <b>{rows}</b>건(수정 {dirty}건)을 Id 오름차순으로 PLC DB에 쓰고 나머지 슬롯은
          0으로 채웁니다.
          <b> Count</b>는 마지막에 씁니다. 쓴 뒤 다시 읽어 바이트를 대조합니다.
        </p>
        <p className="text-content-muted">
          PLC의 기존 테이블은 되돌릴 수 없습니다 — 먼저 <b>PLC 읽기</b>로 백업하거나 Excel로 내보내
          두세요.
        </p>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={force}
            onChange={(e) => setForce(e.currentTarget.checked)}
            data-testid="push-force"
          />
          <span>GR2가 AUTO 모드여도 강제로 씁니다 (작업 검증 중 테이블이 바뀔 수 있음)</span>
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
      label: '상태',
      get: (r) => r.status,
      cell: (r) => <DiffBadge status={r.status} />,
    },
    {
      key: 'local',
      label: '로컬',
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
    <Modal open={open} onOpenChange={onOpenChange} title={title} wide>
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
        {error ? <p className="text-xs text-fault-fg">{error}</p> : null}
        <DataTable
          rows={shown}
          columns={columns}
          rowKey={(r) => String(r.id)}
          loading={rows === null && !error}
          empty={onlyDiff ? '차이 없음 — 로컬과 PLC가 같습니다' : '행 없음'}
          testid="diff-table"
        />
      </div>
    </Modal>
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
    { key: 'row', label: '행', get: (e) => e.row, numeric: true },
    { key: 'sheet', label: '시트', get: (e) => e.sheet ?? '' },
    { key: 'message', label: '문제', get: (e) => e.message },
  ]
  return (
    <Modal open={open} onOpenChange={onOpenChange} title="Excel 가져오기 — 미리보기" wide>
      <div className="flex flex-col gap-3 text-xs">
        <p className="text-content-muted">
          파일 <span className="font-mono">{file?.name ?? ''}</span>
          {preview?.counts
            ? ` · 셀 ${preview.counts.cells} · 스테이션 ${preview.counts.stations} · 품목 ${preview.counts.items}`
            : ''}
        </p>
        {error ? <p className="text-fault-fg">{error}</p> : null}
        {preview ? (
          <div className="grid grid-cols-4 gap-2" data-testid="import-preview">
            {[
              ['추가', preview.imported],
              ['갱신', preview.updated],
              ['동일(건너뜀)', preview.skipped],
              ['오류 행', preview.errors.length],
            ].map(([l, v]) => (
              <div key={String(l)} className="rounded-md border border-line-default px-2 py-1">
                <div className="text-3xs text-content-faint">{l}</div>
                <div className="text-base font-semibold tabular-nums">{v}</div>
              </div>
            ))}
          </div>
        ) : !error ? (
          <p className="text-content-faint">미리보기 계산 중…</p>
        ) : null}
        {preview && preview.errors.length > 0 ? (
          <DataTable
            rows={preview.errors}
            columns={errCols}
            rowKey={(e) => `${e.sheet ?? ''}:${e.row}:${e.message}`}
            testid="import-errors"
          />
        ) : null}
        <p className="text-content-muted">
          오류 행은 건너뛰고 나머지만 로컬에 적용합니다(PLC에는 쓰지 않습니다 — 적용 뒤{' '}
          <b>PLC 쓰기</b>로 반영).
        </p>
        <div className="flex justify-end">
          <Button
            intent="primary"
            size="sm"
            icon={<Upload className="h-3.5 w-3.5" />}
            disabled={!canApply}
            loading={applying}
            onClick={onApply}
            data-testid="import-apply"
          >
            적용
          </Button>
        </div>
      </div>
    </Modal>
  )
}
