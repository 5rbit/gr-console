// 저장된 트레이스 세션 목록 — 최신이 위(백엔드가 `started_at` 내림차순으로 준다).
//
// `overrun` 은 여기서도 따로 색을 받는다: 재생으로 뭔가를 판단하기 전에 **그 세션이 무손실이었는지**
// 부터 봐야 한다.
import { Download, Trash2 } from 'lucide-react'
import { traceApi, type TraceMeta } from '../../lib/trace/api'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import type { Column } from '../../lib/ui/table'

export interface SessionListProps {
  sessions: readonly TraceMeta[]
  /** 지금 재생 중인 세션 id. */
  openId: string
  loadingId: string | null
  onOpen: (m: TraceMeta) => void
  onDelete: (id: string) => void
}

export function clock(s: string | null | undefined): string {
  if (!s) return '-'
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? s : d.toLocaleString('ko-KR', { hour12: false })
}

/** 경과 초 — 단위는 값이 아니라 **열 머리글**에 붙는다(자릿수가 흔들리지 않게). */
export function durationSec(m: TraceMeta): number {
  const end = m.stopped_at ? Date.parse(m.stopped_at) : Date.now()
  const s = Math.max(0, (end - Date.parse(m.started_at)) / 1000)
  return Number.isFinite(s) ? Number(s.toFixed(1)) : 0
}

export function SessionList({ sessions, openId, loadingId, onOpen, onDelete }: SessionListProps) {
  // 행을 누르면 재생(드릴다운)이고, 오른쪽 끝에는 **고정 너비 둘**만 선다(`docs/DESIGN.md` 5절).
  const columns: Column<TraceMeta>[] = [
    { key: 'started_at', label: 'StartedAt', get: (m) => m.started_at, cell: (m) => clock(m.started_at), priority: 1 },
    { key: 'label', label: 'Label', get: (m) => m.label || m.id, priority: 1 },
    { key: 'plc', label: 'Plc', get: (m) => m.plc, priority: 3 },
    { key: 'duration', label: 'Duration (s)', get: durationSec, numeric: true, priority: 2 },
    { key: 'channels', label: 'Channels', get: (m) => m.channels.length, numeric: true, priority: 3 },
    { key: 'rows', label: 'Rows', get: (m) => m.rows, numeric: true, priority: 1 },
    {
      key: 'overrun',
      label: 'Overrun',
      get: (m) => m.overrun,
      cell: (m) => <span className={m.overrun > 0 ? 'text-fault-fg' : ''}>{m.overrun}</span>,
      numeric: true,
      priority: 1,
    },
  ]

  return (
    <DataTable
      rows={[...sessions]}
      columns={columns}
      rowKey={(m) => m.id}
      selected={openId || null}
      onPick={onOpen}
      loading={loadingId !== null && sessions.length === 0}
      density="compact"
      stickyHeader
      className="max-h-80 overflow-y-auto"
      empty="저장된 세션이 없습니다"
      emptyHint="채널을 고르고 시작하면 정지할 때 여기에 남습니다"
      testid="trace-sessions"
      actions={(m) => (
        <span className="flex justify-end gap-1 whitespace-nowrap">
          <Button
            size="sm"
            intent="ghost"
            className="shrink-0"
            icon={<Download size={12} />}
            title="표본을 CSV 파일로 받습니다"
            onClick={() => window.location.assign(traceApi.csvUrl(m.id))}
          >
            CSV
          </Button>
          <Button
            size="sm"
            intent="ghost"
            className="shrink-0"
            icon={<Trash2 size={12} />}
            title="세션 파일 삭제"
            onClick={() => onDelete(m.id)}
            data-testid={`trace-delete-${m.id}`}
          >
            삭제
          </Button>
        </span>
      )}
    />
  )
}
