// 기본값 다이얼로그 — 파라미터 키가 행, `Base | PICK·Cell … DROP·Station | 상황 4개`가 열인 DataGrid.
// 층은 왼쪽에서 오른쪽으로 얹힌다(Base ← 종류·대상 ← 상황 ← 작성 카드). **빈 칸 = 아래 층 값을 그대로 쓴다**
// — 그 값을 흐리게(↳) 보여 주어 "비면 상속"을 칸마다 적지 않는다. 적어 둔 값이 상속값과 같으면 경고색이고,
// "같은 값 비우기"가 그런 칸을 한 번에 비운다(어떤 작업의 최종 값도 바뀌지 않는다).
// 제어형 초안이라 저장 전엔 서버에 아무것도 가지 않는다.
import { useEffect, useMemo, useState } from 'react'
import { Save } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULTS_COLS,
  DEFAULTS_COL_SCOPE,
  applyDefaultsEdit,
  colHeader,
  defaultsChanged,
  defaultsRows,
  inheritedOf,
  inheritedText,
  isRedundant,
  pruneRedundant,
  redundantCells,
  type DefaultsCol,
  type DefaultsRow,
  type ParamKey,
} from '../../lib/task/compose'
import { Button } from '../../lib/ui/Button'
import { DataGrid } from '../../lib/ui/datagrid/DataGrid'
import type { DataGridColumn } from '../../lib/ui/datagrid/types'
import { Dialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { toast } from '../../lib/ui/toast'
import type { Defaults } from '../../lib/types'

export interface DefaultsDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  defaults: Defaults | null
  /** 저장 성공 — 부모가 새 값을 받는다. */
  onSaved: (d: Defaults) => void
}

const fmt = (v: number | boolean | undefined): string => (v === undefined ? '' : String(v))

export function DefaultsDialog({ open, onOpenChange, defaults, onSaved }: DefaultsDialogProps) {
  const [draft, setDraft] = useState<Defaults | null>(defaults)
  const [saving, setSaving] = useState(false)
  useEffect(() => {
    if (open) setDraft(defaults)
  }, [open, defaults])

  const rows = useMemo(() => (draft ? defaultsRows(draft) : []), [draft])
  const changed = !!draft && !!defaults && defaultsChanged(draft, defaults)
  const redundant = useMemo(() => (draft ? redundantCells(draft).length : 0), [draft])

  /** 빈 칸이 쓰는 값 — 표시(↳)와 툴팁에 같이 쓴다. */
  const inherited = (r: DefaultsRow, col: DefaultsCol) =>
    draft ? inheritedOf(draft, r.key, col) : []
  const empty = (r: DefaultsRow, col: DefaultsCol) => col !== 'base' && r.values[col] === undefined
  const redundantCell = (r: DefaultsRow, col: DefaultsCol) =>
    !!draft && isRedundant(draft, r.key, col)

  const columns: DataGridColumn<DefaultsRow>[] = [
    {
      id: 'label',
      header: 'Param',
      width: '11rem',
      sticky: true,
      editor: 'none',
      text: (r) => r.label,
      title: (r) => r.key,
      sortable: true,
    },
    ...DEFAULTS_COLS.map((c): DataGridColumn<DefaultsRow> => ({
      id: c.id,
      header: c.header,
      headerSub: DEFAULTS_COL_SCOPE[c.id],
      width: '6.5rem',
      align: 'right',
      mono: true,
      editor: (r) => (r.bool ? 'select' : 'number'),
      options: (r) => (r.bool ? (c.id === 'base' ? ['true', 'false'] : ['', 'true', 'false']) : []),
      decimals: 0,
      text: (r) => fmt(r.values[c.id]),
      cellClass: (r) =>
        empty(r, c.id) ? 'text-content-disabled' : redundantCell(r, c.id) ? 'text-warn-fg' : '',
      title: (r) => {
        if (empty(r, c.id)) {
          const inh = inherited(r, c.id)
          return `비어 있음 — ${inh.map((v) => `${colHeader(v.from)} 값 ${String(v.value)}`).join(', ')} 을(를) 씁니다`
        }
        if (redundantCell(r, c.id)) return '아래 층 값과 같습니다 — 비워도 결과가 같습니다'
        return ''
      },
      coercePaste: (v, r) => {
        const s = v.trim()
        if (s === '') return c.id === 'base' ? null : ''
        if (r.bool) return ['true', 'false', '1', '0'].includes(s.toLowerCase()) ? s : null
        return Number.isFinite(Number(s)) ? s : null
      },
    })),
  ]

  function edit(rowId: string, colId: string, value: string) {
    if (!draft || colId === 'label') return
    const next = applyDefaultsEdit(draft, rowId as ParamKey, colId as DefaultsCol, value)
    if (!next) {
      toast.warn(`${rowId}: '${value}'은(는) 이 칸에 넣을 수 없습니다`)
      return
    }
    setDraft(next)
  }

  function prune() {
    if (!draft) return
    const { next, removed } = pruneRedundant(draft)
    setDraft(next)
    toast.ok(`같은 값 ${removed}칸을 비웠습니다 — 결과 값은 그대로입니다`)
  }

  async function save() {
    if (!draft) return
    setSaving(true)
    try {
      const saved = await api.defaultsSave(draft)
      onSaved(saved)
      toast.ok(`기본값 저장됨 (v${saved.version})`)
      onOpenChange(false)
    } catch (e) {
      toast.error(`기본값 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="작업 파라미터 기본값"
      size="xl"
      dirty={changed}
      meta={
        <>
          <span className="tabular-nums">{defaults ? `v${defaults.version}` : '버전 없음'}</span>
          {/* 기본값은 로봇별이 아니다(`/api/defaults` 하나) — 로봇 칩 대신 범위를 말해 호기별로 오해하지 않게. */}
          <span data-testid="defaults-scope">모든 로봇 공통</span>
          <HelpTip
            title="기본값 규칙"
            text="층은 왼쪽부터 얹힙니다: Base ← 종류·대상 ← 상황 ← 작성 카드의 덮어쓰기. 빈 칸은 아래 층 값을 그대로 쓰고, 그 값을 흐리게(↳) 보여 줍니다. 칸을 지우면(Delete) 다시 상속입니다. 경고색 칸은 아래 층과 같은 값이라 비워도 결과가 같습니다. 상황은 서버가 고릅니다: Measure Item·SKU = MEASURE 의 측정 플래그(플래그가 없으면 Item), Pallet Station = 팔렛 스테이션에 팔렛 슬롯을 단 작업, Multi-Pick = 스테이션 PICK/DROP 뒤에 같은 스테이션 그룹(셀 id 의 백의 자리) 작업이 이어질 때(순차 계획·시나리오는 자동, 단일 명령은 작성 카드 스위치). Multi-Pick 기본값은 LiftUpPartial = true 입니다. 칸은 더블클릭하거나 바로 타이핑해 고치고 엑셀에서 붙여넣을 수도 있습니다. DragInDist·DragOutDist 는 0 으로 두면 드래그를 켤 때 서버가 150 mm 를 넣고, DragInHeight·DragOutHeight 는 0 이면 지정 없음입니다."
          />
        </>
      }
      footer={
        <>
          <Button
            size="sm"
            intent="ghost"
            disabled={redundant === 0}
            title={
              redundant === 0
                ? '아래 층과 같은 값을 적은 칸이 없습니다'
                : '아래 층과 같은 값을 적은 칸을 비웁니다(결과 값은 그대로)'
            }
            onClick={prune}
            data-testid="defaults-prune"
          >
            {`같은 값 비우기${redundant > 0 ? ` (${redundant})` : ''}`}
          </Button>
          <span className="flex-1" />
          <Button size="sm" intent="ghost" disabled={!changed} onClick={() => setDraft(defaults)}>
            되돌리기
          </Button>
          <Button
            size="sm"
            intent="primary"
            icon={<Save className="h-3.5 w-3.5" />}
            disabled={!changed}
            title={changed ? undefined : '바뀐 값이 없습니다'}
            loading={saving}
            onClick={() => void save()}
            data-testid="defaults-save"
          >
            저장
          </Button>
        </>
      }
      testid="defaults-dialog"
    >
      <div className="flex flex-col gap-2">
        {draft ? (
          <DataGrid<DefaultsRow>
            rows={rows}
            columns={columns}
            rowId={(r) => r.key}
            onedit={edit}
            oneditmany={(ups) => {
              let cur = draft
              for (const u of ups) {
                if (u.colId === 'label') continue
                const next = applyDefaultsEdit(
                  cur,
                  u.rowId as ParamKey,
                  u.colId as DefaultsCol,
                  u.value,
                )
                if (next) cur = next
              }
              setDraft(cur)
            }}
            onpasteskipped={(n) => toast.warn(`붙여넣기 ${n}칸 거부됨(형식 불일치)`)}
            // 빈 칸은 쓰이는 값을 흐리게 — 복사·편집 값은 여전히 빈 문자열(상속)이라 붙여넣어도 상속이 유지된다.
            cell={({ row, col }) => {
              if (col.id === 'label' || !empty(row, col.id as DefaultsCol)) return col.text(row)
              const t = inheritedText(inherited(row, col.id as DefaultsCol))
              return t === '' ? '' : `↳ ${t}`
            }}
            zebra
            layoutFixed
            maxHeight="55vh"
            persistKey="gr.defaults"
            cellTestId={(r, c) => `def-${r.key}-${c.id}`}
          />
        ) : (
          <p className="m-0 text-xs text-content-faint">기본값을 아직 못 받았습니다.</p>
        )}
      </div>
    </Dialog>
  )
}
