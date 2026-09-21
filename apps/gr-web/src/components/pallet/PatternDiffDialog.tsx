// 사양서 비교 창 — `GET /api/pallet/flows/{id}/diff`. 바뀐 패턴마다 사양서/현재 그림을 나란히 두고
// 바뀐 행(필드·슬롯)을 표로 보인다. 같은 패턴은 번호만.
//
// 읽기용 팝업이라 `Dialog`(제출 없음)를 쓴다 — 여기서 고치는 것은 없고, 표는 킷의 `DataTable` 하나로
// 선다(좁은 폭에서 `spec` 열이 먼저 접히고, 행을 펼치면 라벨+값으로 돌아온다).
import { useEffect, useState } from 'react'
import { palletApi } from '../../lib/pallet/api'
import type { DragKind, FieldChange, FlowDiff, ScreenAxes } from '../../lib/pallet/model'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import type { Column } from '../../lib/ui/table'
import { PatternMini } from './PatternMini'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

const FIELD_LABEL: Record<string, string> = {
  od_min: 'OdMin',
  od_max: 'OdMax',
  note: 'Note',
  seq: 'Seq',
  u: 'Offset (norm)',
  drag_dir: 'DragDir',
  name: 'Name',
  drag_kind: 'DragKind',
}

const STATUS_LABEL: Record<string, string> = {
  same: '같음',
  changed: '바뀜',
  added: '추가',
  removed: '삭제',
}

function show(v: unknown): string {
  if (Array.isArray(v)) return v.join(', ')
  if (v === null || v === undefined || v === '') return '—'
  return String(v)
}

/** 비교 표의 한 행 — 흐름 필드·패턴 필드·슬롯 필드를 같은 모양으로 만든다. */
type DiffRow = { key: string; where: string; field: string; spec: string; current: string }

const DIFF_COLUMNS: Column<DiffRow>[] = [
  { key: 'where', label: 'Slot', get: (r) => r.where, priority: 1 },
  { key: 'field', label: 'Field', get: (r) => r.field, priority: 1 },
  { key: 'spec', label: 'spec', get: (r) => r.spec, priority: 2 },
  { key: 'current', label: 'current', get: (r) => r.current, priority: 1 },
]

function fieldRows(fields: FieldChange[], where: string): DiffRow[] {
  return fields.map((f) => ({
    key: `${where}-${f.field}`,
    where,
    field: FIELD_LABEL[f.field] ?? f.field,
    spec: show(f.spec),
    current: show(f.current),
  }))
}

export interface PatternDiffDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  flowId: string
  axes: ScreenAxes
  dragKind: DragKind
  od: number
  gap: number
  palletSize: number
  /** 저장 뒤 다시 읽도록 바뀌는 값. */
  revision: number
}

export function PatternDiffDialog({ open, onOpenChange, flowId, axes, dragKind, od, gap, palletSize, revision }: PatternDiffDialogProps) {
  const [diff, setDiff] = useState<FlowDiff | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    let alive = true
    setDiff(null)
    setError(null)
    palletApi.diff(flowId).then(
      (d) => {
        if (alive) setDiff(d)
      },
      (e) => {
        if (alive) setError(errMsg(e))
      },
    )
    return () => {
      alive = false
    }
  }, [open, flowId, revision])

  const changed = diff?.patterns.filter((p) => p.status !== 'same') ?? []
  const same = diff?.patterns.filter((p) => p.status === 'same').map((p) => p.pattern) ?? []

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`사양서 비교 — ${flowId}`}
      meta={
        diff && diff.status !== 'custom' ? (
          <span className="flex flex-wrap gap-x-3" data-testid="pallet-diff-status">
            <span>
              기준 <span className="font-mono">{diff.based_on}</span>
            </span>
            <span>바뀐 패턴 {changed.length}</span>
            {same.length > 0 && <span>같은 패턴 {same.length}</span>}
          </span>
        ) : undefined
      }
      size="lg"
      testid="pallet-diff"
    >
      <div className="flex flex-col gap-3 text-2xs">
        {error && <p className="text-danger-text">비교를 읽지 못했습니다 — {error}</p>}
        {!diff && !error && <p className="text-content-faint">읽는 중…</p>}
        {diff && diff.status === 'custom' && <p>이 흐름은 사양서 흐름에서 오지 않아 비교할 기준이 없습니다.</p>}
        {diff && diff.status !== 'custom' && (
          <>
            {diff.fields.length > 0 && (
              <DataTable rows={fieldRows(diff.fields, 'Flow')} columns={DIFF_COLUMNS} rowKey={(r) => r.key} density="compact" testid="pallet-diff-flow" />
            )}
            {changed.map((p) => {
              const hot = p.slots.filter((s) => s.status !== 'same').map((s) => s.slot)
              const rows = [
                ...fieldRows(p.fields, `P${p.pattern}`),
                ...p.slots
                  .filter((s) => s.status !== 'same')
                  .flatMap((s) =>
                    s.fields.length > 0
                      ? fieldRows(s.fields, s.slot)
                      : [{ key: `${s.slot}-status`, where: s.slot, field: '상태', spec: '—', current: STATUS_LABEL[s.status] ?? s.status }],
                  ),
              ]
              return (
                <section key={p.pattern} className="flex flex-col gap-2 border-t border-line-default pt-2" data-testid="pallet-diff-pattern">
                  <h4 className="font-semibold text-content-secondary">{`Pattern ${p.pattern} — ${STATUS_LABEL[p.status]}`}</h4>
                  <div className="grid grid-cols-2 gap-3">
                    <PatternMini pattern={p.spec} axes={axes} od={od} gap={gap} palletSize={palletSize} dragKind={dragKind} label="사양서 (spec_r4)" highlight={hot} />
                    <PatternMini pattern={p.current} axes={axes} od={od} gap={gap} palletSize={palletSize} dragKind={dragKind} label="현재 (편집 저장소)" highlight={hot} />
                  </div>
                  <DataTable rows={rows} columns={DIFF_COLUMNS} rowKey={(r) => r.key} density="compact" empty="바뀐 값 없음" />
                </section>
              )
            })}
          </>
        )}
      </div>
    </Dialog>
  )
}
