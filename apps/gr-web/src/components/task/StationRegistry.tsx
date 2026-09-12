// 스테이션 레지스트리 — 셀과 같은 틀, 열이 더 많다(컨베이어·그룹·연결·센서).
import { useMemo, useState } from 'react'
import { Factory } from 'lucide-react'
import { api } from '../../lib/api'
import { taskApi } from '../../lib/task/api'
import type { Registry } from '../../lib/registry'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import type { Station, StationUpsert } from '../../lib/types'
import { RowBadge } from './CellRegistry'
import { EMPTY_STATION, StationForm } from './forms'
import { RegistryToolbar, type RegistryIo } from './RegistryToolbar'

const f1 = (n: number) => (Math.round(n * 10) / 10).toString()
export const stationSummary = (s: Station): string =>
  `CV${s.conv_no} T${s.task_type} G${s.group}-${s.group_index} ${s.info.use ? '' : '(미사용)'} · S${s.info.section} R${s.info.row} C${s.info.col} · ${s.info.position.map(f1).join('/')} · IO${s.io_block_no}`

export function toStationUpsert(s: Station): StationUpsert {
  return {
    id: s.id,
    conv_no: s.conv_no,
    task_type: s.task_type,
    rotate_type: s.rotate_type,
    group: s.group,
    group_index: s.group_index,
    connection_prev: s.connection_prev,
    connection_next: s.connection_next,
    info: { ...s.info, position: [...s.info.position] as [number, number, number] },
    sensor: { ...s.sensor },
    io_block_no: s.io_block_no,
  }
}

export function matchStation(s: Station, q: string): boolean {
  if (!q) return true
  const t =
    `${s.id} cv${s.conv_no} g${s.group}-${s.group_index} s${s.info.section} ${s.source}`.toLowerCase()
  return t.includes(q.toLowerCase())
}

const IO: RegistryIo<Station> = {
  plc: {
    import: taskApi.stationsImport,
    push: taskApi.stationsPush,
    diff: taskApi.stationsDiff,
    summarize: stationSummary,
  },
  exportUrl: taskApi.stationsExportUrl,
  importFile: taskApi.stationsImportFile,
}

export interface StationRegistryProps {
  reg: Registry<Station>
  q: string
  /** 바깥(레이아웃 편집 맵)이 선택을 쥘 때. */
  selectedId?: number | null
  onSelect?: (id: number | null) => void
  /** 좁은 사이드바 — 핵심 열만, 툴바 아이콘만. */
  compact?: boolean
}

export function StationRegistry({
  reg,
  q,
  selectedId,
  onSelect,
  compact = false,
}: StationRegistryProps) {
  const [selLocal, setSelLocal] = useState<number | null>(null)
  const selected = selectedId !== undefined ? selectedId : selLocal
  const setSelected = (id: number | null) => {
    if (selectedId === undefined) setSelLocal(id)
    onSelect?.(id)
  }
  const [form, setForm] = useState<{
    open: boolean
    editing: boolean
    initial: StationUpsert
    key: number
  }>({ open: false, editing: false, initial: EMPTY_STATION, key: 0 })
  const [del, setDel] = useState(false)
  const rows = useMemo(() => reg.items.filter((s) => matchStation(s, q)), [reg.items, q])
  const sel = reg.items.find((s) => s.id === selected) ?? null
  const dirty = reg.items.filter((s) => s.dirty).length

  // `priority` — 스테이션은 **어디가 무슨 작업을 하나**로 훑는다: Id·상태가 1, 작업·컨베이어·사용이
  // 2, 그룹·회전·연결·구역·좌표·IO·센서가 3이다.
  const columns: Column<Station>[] = [
    { key: 'id', label: 'Id', get: (s) => s.id, numeric: true, class: 'font-mono', priority: 1 },
    {
      key: 'state',
      label: '상태',
      get: (s) => (s.dirty ? 1 : s.source === 'plc' ? 0 : 2),
      cell: (s) => <RowBadge source={s.source} dirty={s.dirty} />,
      priority: 1,
    },
    { key: 'conv', label: '컨베이어', get: (s) => s.conv_no, numeric: true, priority: 2 },
    { key: 'type', label: '작업', get: (s) => s.task_type, numeric: true, priority: 2 },
    { key: 'rot', label: '회전', get: (s) => s.rotate_type, numeric: true, priority: 3 },
    { key: 'grp', label: '그룹', get: (s) => `${s.group}-${s.group_index}`, priority: 3 },
    {
      key: 'conn',
      label: '연결',
      get: (s) => `${s.connection_prev}→${s.connection_next}`,
      class: 'font-mono',
      priority: 3,
    },
    {
      key: 'use',
      label: '사용',
      get: (s) => (s.info.use ? 1 : 0),
      cell: (s) => (s.info.use ? 'Y' : <span className="text-content-faint">N</span>),
      priority: 2,
    },
    { key: 'sec', label: '구역', get: (s) => s.info.section, numeric: true, priority: 3 },
    {
      key: 'x',
      label: 'X',
      get: (s) => s.info.position[0],
      numeric: true,
      cell: (s) => f1(s.info.position[0]),
      priority: 3,
    },
    {
      key: 'y',
      label: 'Y',
      get: (s) => s.info.position[1],
      numeric: true,
      cell: (s) => f1(s.info.position[1]),
      priority: 3,
    },
    {
      key: 'z',
      label: 'Z',
      get: (s) => s.info.position[2],
      numeric: true,
      cell: (s) => f1(s.info.position[2]),
      priority: 3,
    },
    { key: 'io', label: 'IO블록', get: (s) => s.io_block_no, numeric: true, priority: 3 },
    {
      key: 'sensor',
      label: '센서',
      get: (s) =>
        `${s.sensor.io_link_master_module}/${s.sensor.io_link_master_port_l}/${s.sensor.io_link_master_port_r}`,
      class: 'font-mono text-content-muted',
      priority: 3,
    },
  ]

  const COMPACT_KEYS = ['id', 'state', 'conv', 'grp', 'x', 'y', 'z']
  const shown = compact ? columns.filter((c) => COMPACT_KEYS.includes(c.key)) : columns

  async function save(v: StationUpsert) {
    if (form.editing) await api.stationUpdate(v.id, v)
    else await api.stationCreate(v)
    setForm((s) => ({ ...s, open: false }))
    await reg.reload()
    toast.ok(`스테이션 #${v.id} 저장됨 (로컬)`)
  }
  async function remove() {
    if (!sel) return
    try {
      await api.stationDelete(sel.id)
      toast.ok(`스테이션 #${sel.id} 삭제됨 (로컬)`)
      setSelected(null)
      await reg.reload()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="station-registry">
      <RegistryToolbar<Station>
        compact={compact}
        title="스테이션"
        icon={<Factory size={14} />}
        what="스테이션"
        rows={reg.items.length}
        dirty={dirty}
        selected={!!sel}
        io={IO}
        reload={reg.reload}
        onAdd={() =>
          setForm({ open: true, editing: false, initial: EMPTY_STATION, key: Date.now() })
        }
        onEdit={() =>
          sel &&
          setForm({ open: true, editing: true, initial: toStationUpsert(sel), key: Date.now() })
        }
        onDelete={() => setDel(true)}
      />
      <div className="min-h-0 flex-1 overflow-auto">
        {reg.error ? <p className="p-2 text-xs text-fault-fg">{reg.error}</p> : null}
        <DataTable
          rows={rows}
          columns={shown}
          rowKey={(s) => String(s.id)}
          selected={selected === null ? null : String(selected)}
          onPick={(s) => setSelected(s.id === selected ? null : s.id)}
          loading={reg.loading}
          empty={q ? '검색 결과 없음' : '스테이션 없음'}
          emptyHint={
            q ? undefined : 'PLC 읽기로 STATION 테이블을 가져오거나 추가/Excel 가져오기로 만드세요.'
          }
          testid="station-table"
        />
      </div>
      {form.open ? (
        <StationForm
          key={form.key}
          open={form.open}
          onOpenChange={(o) => setForm((s) => ({ ...s, open: o }))}
          initial={form.initial}
          editing={form.editing}
          onSave={save}
        />
      ) : null}
      <ConfirmDialog
        open={del}
        onOpenChange={setDel}
        scope="single"
        title="스테이션 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => void remove()}
      >
        <p className="text-sm">
          로컬 스테이션 <b>#{sel?.id}</b>을 지웁니다. PLC 테이블은 다음 <b>PLC 쓰기</b> 때 바뀝니다.
        </p>
      </ConfirmDialog>
    </div>
  )
}
