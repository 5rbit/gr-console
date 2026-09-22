// 기본값 다이얼로그 — 파라미터 키가 행, `Base | PICK·Cell … DROP·Station | 상황 4개`가 열인 DataGrid.
// 층은 왼쪽에서 오른쪽으로 얹힌다(Base ← 종류·대상 ← 상황 ← 작성 카드). **빈 칸 = 아래 층 값을 그대로 쓴다**
// — 그 값을 흐리게(↳) 보여 주어 "비면 상속"을 칸마다 적지 않는다. 적어 둔 값이 상속값과 같으면 경고색이고,
// "같은 값 비우기"가 그런 칸을 한 번에 비운다(어떤 작업의 최종 값도 바뀌지 않는다).
// 제어형 초안이라 저장 전엔 서버에 아무것도 가지 않는다.
//
// 저장은 버전을 본다(다른 탭·세션이 먼저 저장했으면 409 → 최신을 다시 읽는다). `⋯` 메뉴: JSON 내보내기 ·
// 가져오기(미리보기 → 적용) · 변경 이력(되돌리기). 서버가 읽다 걷어 낸 값(load_warnings)이 있으면 위에 알린다.
import { useEffect, useMemo, useRef, useState } from 'react'
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
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Dialog } from '../../lib/ui/Dialog'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { HelpTip } from '../../lib/ui/HelpTip'
import { toast } from '../../lib/ui/toast'
import type { Defaults, DefaultsHistoryRow, DefaultsImportResult } from '../../lib/types'

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
  const [histOpen, setHistOpen] = useState(false)
  const [hist, setHist] = useState<DefaultsHistoryRow[] | null>(null)
  const [restoreRow, setRestoreRow] = useState<DefaultsHistoryRow | null>(null)
  const [importBody, setImportBody] = useState<unknown>(null)
  const [importRes, setImportRes] = useState<DefaultsImportResult | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const warnings = defaults?.load_warnings ?? []

  async function reload(msg: string) {
    try {
      const fresh = await api.defaults()
      onSaved(fresh)
      setDraft(fresh)
      toast.warn(msg)
    } catch (e) {
      toast.error(`기본값 다시 읽기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function openHistory() {
    setHistOpen(true)
    setHist(null)
    try {
      setHist(await api.defaultsHistory(30))
    } catch (e) {
      toast.error(`이력 읽기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function restore(row: DefaultsHistoryRow) {
    try {
      const d = await api.defaultsRestore(row.id)
      onSaved(d)
      setDraft(d)
      setHistOpen(false)
      toast.ok(`v${row.version} 상태로 되돌림 (새 v${d.version})`)
    } catch (e) {
      toast.error(`되돌리기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function pickImport(file: File) {
    try {
      const body: unknown = JSON.parse(await file.text())
      setImportBody(body)
      setImportRes(await api.defaultsImport(body, true))
    } catch (e) {
      toast.error(`가져오기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function applyImport() {
    try {
      const r = await api.defaultsImport(importBody, false)
      if (!r.saved) {
        setImportRes(r)
        toast.warn('가져오지 않았습니다 — 문제가 있습니다')
        return
      }
      const d = await api.defaults()
      onSaved(d)
      setDraft(d)
      toast.ok(`가져옴 — ${r.changes.length}곳 바뀜 (v${r.version})`)
      setImportRes(null)
    } catch (e) {
      toast.error(`가져오기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

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
      const m = e instanceof Error ? e.message : String(e)
      // 다른 탭·세션이 먼저 저장 — 옛 초안으로 덮지 않고 최신을 불러온다(고친 칸은 다시 넣어야 한다).
      if (m.includes('→ 409'))
        await reload(
          '다른 곳에서 먼저 저장했습니다 — 최신 기본값을 불러왔습니다. 고친 칸을 다시 넣으세요',
        )
      else toast.error(`기본값 저장 실패 — ${m}`)
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
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
              text="저장은 버전을 봅니다 — 다른 곳에서 먼저 저장했으면 최신을 다시 불러옵니다. ⋯ 메뉴에서 JSON 내보내기·가져오기(바뀌는 곳을 먼저 보여 줌)·변경 이력(되돌리기)을 합니다. 층은 왼쪽부터 얹힙니다: Base ← 종류·대상 ← 상황 ← 작성 카드의 덮어쓰기. 빈 칸은 아래 층 값을 그대로 쓰고, 그 값을 흐리게(↳) 보여 줍니다. 칸을 지우면(Delete) 다시 상속입니다. 경고색 칸은 아래 층과 같은 값이라 비워도 결과가 같습니다. 상황은 서버가 고릅니다: Measure Item·SKU = MEASURE 의 측정 플래그(플래그가 없으면 auto — 대상 재고 1개 이하 Item, 2개 이상 SKU), Pallet Station = 팔렛 스테이션에 팔렛 슬롯을 단 작업, Multi-Pick = 스테이션 PICK/DROP 뒤에 같은 스테이션 그룹(셀 id 의 백의 자리) 작업이 이어질 때(순차 계획·시나리오는 자동, 단일 명령은 작성 카드 스위치). Multi-Pick 기본값은 LiftUpPartial = true 입니다. 칸은 더블클릭하거나 바로 타이핑해 고치고 엑셀에서 붙여넣을 수도 있습니다. DragInDist·DragOutDist 는 0 으로 두면 드래그를 켤 때 서버가 150 mm 를 넣고, DragInHeight·DragOutHeight 는 0 이면 지정 없음입니다."
            />
            <OverflowMenu
              title="내보내기 · 가져오기 · 이력"
              testid="defaults-more"
              items={[
                {
                  label: 'JSON 내보내기',
                  run: () => {
                    const a = document.createElement('a')
                    a.href = api.defaultsExportUrl
                    a.download = 'gr-console-defaults.json'
                    a.click()
                  },
                },
                {
                  label: 'JSON 가져오기…',
                  disabled: changed
                    ? '저장하지 않은 변경이 있습니다 — 저장하거나 되돌린 뒤'
                    : undefined,
                  run: () => fileRef.current?.click(),
                },
                { label: '변경 이력…', run: () => void openHistory() },
              ]}
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
          {warnings.length ? (
            <div
              className="rounded-md border border-warn bg-warn-soft px-2 py-1 text-xs text-warn-fg"
              title={warnings.join('\n')}
              data-testid="defaults-load-warnings"
            >
              저장된 기본값에서 읽지 못한 값 {warnings.length}개를 버렸습니다(원본은 서버에 보관) —{' '}
              {warnings[0]}
              {warnings.length > 1 ? ` 외 ${warnings.length - 1}` : ''}. 확인 후 저장하면 경고가
              사라집니다.
            </div>
          ) : null}
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

      <input
        ref={fileRef}
        type="file"
        accept=".json,application/json"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (file) void pickImport(file)
        }}
      />

      <ConfirmDialog
        open={importRes !== null}
        onOpenChange={(o) => {
          if (!o) setImportRes(null)
        }}
        scope="fleet"
        title="기본값 가져오기"
        confirmLabel={
          importRes && (importRes.problems.length || importRes.dropped.length) ? '닫기' : '적용'
        }
        onConfirm={() => {
          if (importRes && !importRes.problems.length && !importRes.dropped.length)
            void applyImport()
        }}
      >
        {importRes ? (
          <div
            className="flex max-h-80 flex-col gap-1 overflow-auto text-xs"
            data-testid="defaults-import-preview"
          >
            {importRes.problems.length || importRes.dropped.length ? (
              <div className="text-fault-fg">
                파일에 문제가 있어 가져올 수 없습니다 —{' '}
                {[...importRes.problems, ...importRes.dropped].join(' · ')}
              </div>
            ) : (
              <div>모든 로봇의 작업 파라미터 기본값이 바뀝니다 — {importRes.changes.length}곳</div>
            )}
            {importRes.changes.map((c) => (
              <div key={c} className="font-mono text-2xs">
                {c}
              </div>
            ))}
          </div>
        ) : null}
      </ConfirmDialog>

      <Dialog
        open={histOpen}
        onOpenChange={setHistOpen}
        title="기본값 변경 이력"
        size="lg"
        closeLabel="닫기"
        testid="defaults-history"
      >
        {hist === null ? (
          <div className="text-xs text-content-faint">읽는 중…</div>
        ) : hist.length === 0 ? (
          <div className="text-xs text-content-faint">
            이력이 없습니다(이 버전부터 저장마다 남습니다).
          </div>
        ) : (
          <div className="flex max-h-96 flex-col gap-2 overflow-auto">
            {hist.map((h) => (
              <div key={h.id} className="rounded-md border border-line-default px-2 py-1 text-xs">
                <div className="flex items-center gap-2">
                  <span className="font-mono tabular-nums">v{h.version}</span>
                  <span className="tabular-nums text-content-muted">
                    {h.at.replace('T', ' ').slice(0, 19)}
                  </span>
                  {h.note ? <span className="text-content-faint">{h.note}</span> : null}
                  <span className="flex-1" />
                  <Button
                    size="sm"
                    intent="ghost"
                    onClick={() => setRestoreRow(h)}
                    data-testid={`defaults-restore-${h.id}`}
                  >
                    이 상태로 되돌리기
                  </Button>
                </div>
                {h.changes.map((c) => (
                  <div key={c} className="font-mono text-2xs text-content-muted">
                    {c}
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}
      </Dialog>

      <ConfirmDialog
        open={restoreRow !== null}
        onOpenChange={(o) => {
          if (!o) setRestoreRow(null)
        }}
        scope="fleet"
        title={restoreRow ? `v${restoreRow.version} 상태로 되돌리기` : ''}
        confirmLabel="되돌리기"
        onConfirm={() => {
          if (restoreRow) void restore(restoreRow)
        }}
      >
        <div className="text-xs">
          모든 로봇의 작업 파라미터 기본값을 이 저장 직후 상태로 되돌립니다(새 버전으로 저장되고
          이력에 남습니다).
        </div>
      </ConfirmDialog>
    </>
  )
}
