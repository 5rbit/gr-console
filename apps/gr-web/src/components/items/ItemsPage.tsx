// 화물 규격 화면 — 품목(타이어 코드) 규격 전체를 한 화면에서 찾고·고치고·복제하고·지운다.
//
// 작업 명령 화면 왼쪽 레일의 품목 탭은 명령을 내면서 곁눈질하는 자리라 좁다. 규격을 수십 개 들여오고
// 다듬는 일은 넓은 표가 필요해서 따로 섰다. 폼(`ItemForm`)·툴바(`RegistryToolbar`)·검증은 레일과 같은 것을 쓴다.
//
// 행을 누르면 오른쪽에 상세 패널(`ItemDetail`: Spec · Beads · Usage)이 스플리터로 붙는다 — 표를 보면서
// StackMax·단별 비드 표를 고친다. 체크한 행들은 위 일괄 편집 줄에서 StackMax / PalletMax 를 한 번에.
// 열 머리·필드 이름은 PLC/데이터 이름 그대로(영문)다.
//
// 재고 사용: 셀 재고 스트림(`lib/stock`)에서 코드별로 몇 칸이 그 코드를 들고 있는지 센다 — 지우기 전에
// "지금 쓰는 코드인가"가 보여야 한다. StackMax 를 넘은 칸이 있으면 그 칸 수를 경고색으로.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Check, CheckCheck, Copy, FileSpreadsheet, Gauge, Plus, Ruler, Undo2 } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULT_PICK_BEAD_OFFSET,
  bulkStackMaxErrors,
  compressionOf,
  overLimit,
  pickBeadOffset,
  round1,
  specOf,
} from '../../lib/items/levelsModel'
import { BULK_HELP } from '../../lib/items/panelInfo'
import { duplicateItem, matchesItem, stockUsage, usedCodes } from '../../lib/items/model'
import { useRegistry } from '../../lib/registry'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { taskApi } from '../../lib/task/api'
import { ITEMS_EXCEL_HELP } from '../../lib/task/importPreview'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { HelpTip } from '../../lib/ui/HelpTip'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Input } from '../../lib/ui/Input'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { Switch } from '../../lib/ui/Switch'
import type { Item, ItemUpsert } from '../../lib/types'
import { EMPTY_ITEM, ItemForm } from '../task/forms'
import { toItemUpsert } from '../task/ItemRegistry'
import { RegistryToolbar, type RegistryActions, type RegistryIo } from '../task/RegistryToolbar'
import { Splitter } from '../workspace/Splitter'

import { ItemDetail } from './ItemDetail'
import { DimsReviewDialog } from './DimsReviewDialog'
import { DimsSummary, MeasuredCell, confirmLine, dimKey, useDimsOverlay } from './DimsInline'
import {
  TONE_LABEL,
  WINDOWS,
  dimTone,
  dimsApi,
  safeTone,
  type DimChange,
  type DimField,
  type DimSuggestion,
} from '../../lib/items/dims'

// 양식이 있으면 툴바의 `추가`가 두 쪽 버튼(폼 · Excel 메뉴)이 된다 — 규격은 폼보다 Excel 로 수십 개씩 들어온다.
const IO: RegistryIo<Item> = {
  exportUrl: taskApi.itemsExportUrl,
  templateUrl: taskApi.itemsTemplateUrl,
  importHelp: ITEMS_EXCEL_HELP,
  importFile: taskApi.itemsImportFile,
}

const DETAIL_KEY = 'gr-items-detail-w'
const DETAIL_MIN = 360
const DETAIL_MAX = 1100
const DETAIL_DEFAULT = 620

const f1 = (n: number) => (Math.round(n * 10) / 10).toString()
const when = (s: string) => s.slice(0, 19).replace('T', ' ')
const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))
/** 좁은 숫자 열 — 머리글 · 칸 여백을 줄인다(DataTable 기본 px-2). */
const NARROW = 'px-1.5!'
/** 약어 머리글 — 전체 이름 · 단위는 툴팁과 읽어 주는 이름으로. */
const hdr = (short: string, full: string) => ({
  label: (
    <span title={full} className="cursor-help">
      {short}
    </span>
  ),
  name: full,
})
const faint = (s = '—') => <span className="text-content-faint">{s}</span>
/** 1단 타이어를 혼자(위에 아무것도 없이) 집는 높이 — UpperBidHeight − PickBeadOffset. 비드가 없으면 null. */
const pickZAtL1 = (i: Item): number | null =>
  i.upper_bead_height > 0 ? Math.max(i.upper_bead_height - pickBeadOffset(specOf(i)), 0) : null

const DIMS_KEY = 'gr-items-dims'
function readDimsOn(): boolean {
  try {
    return localStorage.getItem(DIMS_KEY) !== '0'
  } catch {
    return true
  }
}

/** 한 번의 측정 반영 묶음 — "방금 적용 되돌리기" 가 통째로 되돌린다. */
interface DimBatch {
  code: number
  field: string
  id: number
}

function readWidth(): number {
  try {
    const n = Number(localStorage.getItem(DETAIL_KEY))
    return n >= DETAIL_MIN && n <= DETAIL_MAX ? n : DETAIL_DEFAULT
  } catch {
    return DETAIL_DEFAULT
  }
}

/** 일괄 입력 칸 — 빈칸 = 그대로(`undefined`), 아니면 숫자(정수 아니면 NaN 그대로 넘겨 검증이 잡는다). */
const bulkValue = (s: string): number | undefined => (s.trim() === '' ? undefined : Number(s))

interface FormState {
  open: boolean
  editing: boolean
  initial: ItemUpsert
  key: number
}

export default function ItemsPage() {
  const reg = useRegistry<Item>(api.items)
  useStore(stockStore)
  useEffect(() => stockStore.start(), [])
  const usage = useMemo(() => stockUsage(stockStore.map.values()), [stockStore.map])
  const stockRows = useMemo(() => [...stockStore.map.values()], [stockStore.map])

  const [q, setQ] = useState('')
  const [selected, setSelected] = useState<number | null>(null)
  const [checked, setChecked] = useState<ReadonlySet<number>>(() => new Set())
  const [form, setForm] = useState<FormState>({
    open: false,
    editing: false,
    initial: EMPTY_ITEM,
    key: 0,
  })
  const [del, setDel] = useState<number[] | null>(null)
  const [detailW, setDetailW] = useState(readWidth)
  const [detailDirty, setDetailDirty] = useState(false)
  /** 저장 안 한 상세를 두고 다른 행을 고르려 할 때 — `undefined` = 묻는 중 아님. */
  const [pendingPick, setPendingPick] = useState<number | null | undefined>(undefined)
  const [dimsOpen, setDimsOpen] = useState(false)
  // ── 표 안의 측정 제안(DimsInline) ──
  const [dimsOn, setDimsOnState] = useState(readDimsOn)
  const [dimWin, setDimWin] = useState(5)
  const [dimOnly, setDimOnly] = useState(false)
  const [dimPick, setDimPick] = useState<ReadonlySet<string>>(() => new Set())
  const [dimBusy, setDimBusy] = useState(false)
  const [dimFlash, setDimFlash] = useState<ReadonlySet<string>>(() => new Set())
  const [dimConfirm, setDimConfirm] = useState<{
    title: string
    rows: { code: number; s: DimSuggestion }[]
  } | null>(null)
  const [lastBatch, setLastBatch] = useState<DimBatch[]>([])
  /** 칩에서 온 상세 요청 — 옆 패널을 Measured 탭으로 열고 그 필드 줄을 강조한다. */
  const [dimFocus, setDimFocus] = useState<{ code: number; field: DimField; n: number } | null>(
    null,
  )
  const [bulkStack, setBulkStack] = useState('')
  const [bulkPallet, setBulkPallet] = useState('')
  const [bulkBusy, setBulkBusy] = useState(false)
  /** 툴바의 가져오기 창을 빈 표 버튼에서도 연다. */
  const toolbar = useRef<RegistryActions | null>(null)

  const dims = useDimsOverlay(dimsOn, dimWin, reg.items)
  const rows = useMemo(
    () =>
      reg.items.filter(
        (i) =>
          matchesItem(i, q) && (!dimsOn || !dimOnly || dims.all.some((a) => a.code === i.code)),
      ),
    [reg.items, q, dimsOn, dimOnly, dims.all],
  )
  const sel = reg.items.find((i) => i.code === selected) ?? null
  // 목록이 갈리면(삭제·가져오기) 사라진 코드의 체크는 버린다.
  const picked = useMemo(
    () => reg.items.filter((i) => checked.has(i.code)).map((i) => i.code),
    [reg.items, checked],
  )
  const pickedItems = useMemo(
    () => reg.items.filter((i) => checked.has(i.code)),
    [reg.items, checked],
  )
  const allShownPicked = rows.length > 0 && rows.every((i) => checked.has(i.code))
  const inUse = reg.items.filter((i) => usage.has(i.code)).length

  /** 코드 → StackMax 를 넘은 재고 칸 수. */
  const overCells = useMemo(() => {
    const max = new Map(reg.items.map((i) => [i.code, specOf(i).stack_max]))
    const out = new Map<number, number>()
    for (const s of stockStore.map.values()) {
      if (!s.item_code || !overLimit(s.count, max.get(s.item_code) ?? 0)) continue
      out.set(s.item_code, (out.get(s.item_code) ?? 0) + 1)
    }
    return out
  }, [reg.items, stockStore.map])
  const overTotal = [...overCells.values()].reduce((a, b) => a + b, 0)

  const onDirtyChange = useCallback((d: boolean) => setDetailDirty(d), [])

  function toggle(code: number) {
    setChecked((prev) => {
      const next = new Set(prev)
      if (next.has(code)) next.delete(code)
      else next.add(code)
      return next
    })
  }
  function toggleShown() {
    setChecked((prev) => {
      const next = new Set(prev)
      for (const i of rows) {
        if (allShownPicked) next.delete(i.code)
        else next.add(i.code)
      }
      return next
    })
  }
  function pick(code: number | null) {
    if (code === selected) return
    if (detailDirty) setPendingPick(code)
    else setSelected(code)
  }

  // ── 측정 제안 ──
  function setDimsOn(on: boolean) {
    setDimsOnState(on)
    try {
      localStorage.setItem(DIMS_KEY, on ? '1' : '0')
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  useEffect(() => {
    if (!dimFlash.size) return
    const t = setTimeout(() => setDimFlash(new Set()), 1400)
    return () => clearTimeout(t)
  }, [dimFlash])
  /** 칩 누름 — 그냥 = 옆 패널에 자세히(Measured 탭 · 그 필드 강조), Ctrl = 여러 개 선택. */
  function chipPick(code: number, field: DimField, multi: boolean) {
    if (multi) {
      setDimPick((p) => {
        const n = new Set(p)
        const k = dimKey(code, field)
        if (n.has(k)) n.delete(k)
        else n.add(k)
        return n
      })
      return
    }
    setDimFocus({ code, field, n: Date.now() })
    if (code !== selected) pick(code)
  }
  const dimSelected = dims.all.filter((a) => dimPick.has(dimKey(a.code, a.s.field)))
  const dimSafe = dims.all.filter((a) => safeTone(dimTone(a.s)))
  const dimRisky = (list: { s: DimSuggestion }[]) => list.filter((a) => !safeTone(dimTone(a.s)))
  /** 저장하지 않은 상세가 그 품목이면 막는다 — 적용 뒤 상세가 새 값으로 다시 열리며 편집이 사라진다. */
  const dimBlocked = (codes: number[]) =>
    detailDirty && selected !== null && codes.includes(selected)
      ? `품목 ${selected} 상세에 저장하지 않은 편집이 있습니다 — 먼저 저장하거나 되돌리세요`
      : null

  async function dimApply(list: { code: number; s: DimSuggestion }[]) {
    const block = dimBlocked(list.map((a) => a.code))
    if (block) {
      toast.warn(block)
      return
    }
    const by = new Map<number, DimField[]>()
    for (const a of list) by.set(a.code, [...(by.get(a.code) ?? []), a.s.field])
    setDimBusy(true)
    try {
      const res = await dimsApi.applyBulk(
        [...by].map(([code, fields]) => ({ code, fields })),
        dimWin,
      )
      const batch: DimBatch[] = res.results.flatMap((r) =>
        (r.applied ?? []).map((a) => ({ code: r.code, field: a.field, id: a.id })),
      )
      setLastBatch(batch)
      setDimFlash(new Set(batch.map((b) => dimKey(b.code, b.field))))
      setDimPick(
        (p) => new Set([...p].filter((k) => !batch.some((b) => dimKey(b.code, b.field) === k))),
      )
      if (res.failed) {
        const first = res.results.find((r) => !r.ok)
        toast.error(
          `측정 반영 ${batch.length}건 · ${res.failed}개 품목 실패 (#${first?.code} ${first?.error ?? ''})`,
        )
      } else
        toast.ok(
          `측정 반영 ${batch.length}건 (${by.size}개 품목) — 띠의 "방금 적용 되돌리기" 로 되돌릴 수 있습니다`,
        )
      await reg.reload()
    } catch (e) {
      toast.error(`측정 반영 실패 — ${errMsg(e)}`)
    } finally {
      setDimBusy(false)
    }
  }
  /** 적용 요청 — 한 줄이고 바로 적용해도 되면 곧장, 아니면 확인 창. */
  function dimRequest(title: string, list: { code: number; s: DimSuggestion }[]) {
    if (!list.length) return
    if (list.length === 1 && safeTone(dimTone(list[0].s))) void dimApply(list)
    else setDimConfirm({ title, rows: list })
  }
  async function dimRevert(list: { code: number; id: number; field: string }[]) {
    const block = dimBlocked(list.map((b) => b.code))
    if (block) {
      toast.warn(block)
      return
    }
    setDimBusy(true)
    let ok = 0
    const failed: string[] = []
    // 나중에 적용한 것부터 — 같은 필드가 두 번 들었어도 원래 값으로 돌아간다
    for (const b of [...list].reverse()) {
      try {
        await dimsApi.revert(b.code, b.id)
        ok++
      } catch (e) {
        failed.push(`#${b.code} ${b.field}: ${errMsg(e)}`)
      }
    }
    setDimFlash(new Set(list.map((b) => dimKey(b.code, b.field))))
    setLastBatch((prev) => prev.filter((p) => !list.some((b) => b.id === p.id)))
    if (failed.length) toast.error(`되돌림 ${ok}건 · 실패 ${failed.length}건 — ${failed[0]}`)
    else toast.ok(`측정 반영 ${ok}건 되돌림`)
    setDimBusy(false)
    await reg.reload()
  }
  /** 측정 열(머리글 "측정") — 등록값 열 바로 옆, 색 글자 하나(`MeasuredCell`). 측정 제안을 켰을 때만. */
  const measCol = (after: string, field: DimField, full: string): Column<Item>[] =>
    dimsOn
      ? [
          {
            key: `m_${after}`,
            ...hdr('측정', `${full} 측정 제안 (mm) — 글자색 = 결정, 누르면 옆 패널에 자세히`),
            numeric: true,
            class: NARROW,
            priority: 2,
            get: (i) => dims.sugOf(i.code, field)?.suggested ?? -1,
            cell: (i) => (
              <MeasuredCell
                code={i.code}
                s={dims.sugOf(i.code, field)}
                live={dims.liveOf(i.code, field)}
                window={dimWin}
                picked={dimPick.has(dimKey(i.code, field))}
                flash={dimFlash.has(dimKey(i.code, field))}
                disabled={dimBusy}
                onPick={(multi) => chipPick(i.code, field, multi)}
                onRevert={(c: DimChange) =>
                  void dimRevert([{ code: i.code, id: c.id, field: c.field }])
                }
              />
            ),
          },
        ]
      : []

  // `priority` — 코드·이름·StackMax·재고 사용이 1(지울 수 있나·한도를 보는 열), 명령에 직접 쓰는 치수가 2, 나머지가 3.
  // 머리글은 줄인 이름(InnerDia · UpperBead …) — 전체 이름 · 단위는 머리글 툴팁(`hdr`). 숫자 열은 좁은 여백(`NARROW`).
  const columns: Column<Item>[] = [
    {
      key: 'pick',
      label: '',
      name: '선택',
      sortable: false,
      priority: 1,
      class: NARROW,
      cell: (i) => (
        <input
          type="checkbox"
          checked={checked.has(i.code)}
          aria-label={`품목 ${i.code} 선택`}
          onClick={(e) => e.stopPropagation()}
          onChange={() => toggle(i.code)}
          data-testid="items-pick"
        />
      ),
    },
    {
      key: 'code',
      label: 'Code',
      get: (i) => i.code,
      numeric: true,
      class: `font-mono ${NARROW}`,
      priority: 1,
    },
    { key: 'name', label: 'Name', get: (i) => i.name, class: NARROW, priority: 1 },
    ...(dimsOn
      ? [
          {
            key: 'dims',
            ...hdr(
              '측정',
              '측정 제안 — 결정별 개수(빨강 차이 큼 · 주황 흔들림 · 회색 표본 부족 · 파랑 새 값 · 초록 적용 가능), 체크 단추 = 이 품목 바로 적용, 되돌리기 단추 = 방금 적용 되돌리기',
            ),
            priority: 1 as const,
            sortable: false,
            class: NARROW,
            cell: (i: Item) => {
              const d = dims.byCode.get(i.code)
              const safe = dimSafe.filter((a) => a.code === i.code)
              const live = lastBatch.filter((b) => b.code === i.code)
              if (!d && !live.length) return null
              return (
                <span className="inline-flex items-center gap-1">
                  <DimsSummary d={d} />
                  {safe.length ? (
                    <button
                      type="button"
                      className="rounded-sm p-0.5 text-accent-text hover:bg-surface-hover disabled:opacity-50"
                      disabled={dimBusy}
                      title={`이 품목의 ${TONE_LABEL.ok} · ${TONE_LABEL.info} 제안 ${safe.length}건 적용`}
                      aria-label={`${i.code} 바로 적용`}
                      onClick={(e) => {
                        e.stopPropagation()
                        dimRequest(`#${i.code} 바로 적용`, safe)
                      }}
                      data-testid={`dims-row-apply-${i.code}`}
                    >
                      <Check className="h-3 w-3" />
                    </button>
                  ) : null}
                  {live.length ? (
                    <button
                      type="button"
                      className="rounded-sm p-0.5 text-content-muted hover:bg-surface-hover disabled:opacity-50"
                      disabled={dimBusy}
                      title={`이 품목에 방금 적용한 ${live.length}건 되돌리기`}
                      aria-label={`${i.code} 되돌리기`}
                      onClick={(e) => {
                        e.stopPropagation()
                        void dimRevert(live)
                      }}
                      data-testid={`dims-row-revert-${i.code}`}
                    >
                      <Undo2 className="h-3 w-3" />
                    </button>
                  ) : null}
                </span>
              )
            },
          },
        ]
      : []),
    {
      key: 'count',
      ...hdr('Count', 'Count'),
      get: (i) => i.count,
      numeric: true,
      class: NARROW,
      priority: 2,
    },
    {
      key: 'id',
      ...hdr('InnerDia', 'InnerDiameter (mm)'),
      get: (i) => i.inner_diameter,
      numeric: true,
      class: NARROW,
      cell: (i) => f1(i.inner_diameter),
      priority: 2,
    },
    ...measCol('id', 'inner_diameter', 'InnerDiameter'),
    {
      key: 'od',
      ...hdr('OuterDia', 'OuterDiameter (mm)'),
      get: (i) => i.outer_diameter,
      numeric: true,
      class: NARROW,
      cell: (i) => f1(i.outer_diameter),
      priority: 2,
    },
    {
      key: 'h',
      ...hdr('Height', 'Height (mm)'),
      get: (i) => i.height,
      numeric: true,
      class: NARROW,
      cell: (i) => f1(i.height),
      priority: 2,
    },
    ...measCol('h', 'height', 'Height'),
    {
      key: 'lb',
      ...hdr('LowerBead', 'LowerBidHeight (mm)'),
      get: (i) => i.lower_bead_height,
      numeric: true,
      class: NARROW,
      cell: (i) => f1(i.lower_bead_height),
      priority: 3,
    },
    {
      key: 'ub',
      ...hdr('UpperBead', 'UpperBidHeight (mm)'),
      get: (i) => i.upper_bead_height,
      numeric: true,
      class: NARROW,
      cell: (i) => f1(i.upper_bead_height),
      // 측정 제안을 켜면 좁은 폭에서도 남긴다(옆에 mUB 가 붙는다)
      priority: dimsOn ? 2 : 3,
    },
    ...measCol('ub', 'upper_bead_height', 'UpperBidHeight'),
    {
      key: 'df',
      ...hdr('Deflection', 'DeflectionFactor'),
      get: (i) => i.deflection_factor,
      numeric: true,
      class: NARROW,
      priority: 3,
    },
    {
      key: 'stack_max',
      ...hdr('StackMax', 'StackMax — 셀 최대 단수(0 = 제한 없음)'),
      get: (i) => specOf(i).stack_max,
      numeric: true,
      class: NARROW,
      priority: 1,
      cell: (i) => {
        const v = specOf(i).stack_max
        return v > 0 ? <span className="font-mono">{v}</span> : faint()
      },
    },
    {
      key: 'pallet_max',
      ...hdr('PalletMax', 'PalletMax — 팔레트당 최대 개수(0 = 제한 없음)'),
      get: (i) => specOf(i).pallet_max,
      numeric: true,
      class: NARROW,
      priority: 2,
      cell: (i) => {
        const v = specOf(i).pallet_max
        return v > 0 ? <span className="font-mono">{v}</span> : faint()
      },
    },
    {
      key: 'weight',
      ...hdr('Weight', 'WeightKg (kg)'),
      get: (i) => specOf(i).weight_kg ?? -1,
      numeric: true,
      class: NARROW,
      priority: 3,
      cell: (i) => {
        const v = specOf(i).weight_kg
        return v === null ? faint() : f1(v)
      },
    },
    {
      key: 'pick_bead_offset',
      ...hdr('PickOffset', 'PickBeadOffset (mm)'),
      get: (i) => specOf(i).pick_bead_offset ?? DEFAULT_PICK_BEAD_OFFSET,
      numeric: true,
      class: NARROW,
      priority: 2,
      cell: (i) => {
        const s = specOf(i)
        return s.pick_bead_offset === null ? (
          <span className="text-content-faint" title={`미입력 — 기본 ${DEFAULT_PICK_BEAD_OFFSET}`}>
            {DEFAULT_PICK_BEAD_OFFSET}
          </span>
        ) : (
          f1(s.pick_bead_offset)
        )
      },
    },
    {
      key: 'compression',
      ...hdr('Compress', 'Compression (mm) — 위 타이어 1개당 눌림'),
      get: (i) => compressionOf(specOf(i)),
      numeric: true,
      class: NARROW,
      priority: 2,
      cell: (i) => {
        const s = specOf(i)
        return s.compression === null || s.compression === 0 ? (
          <span className="text-content-faint" title="미입력 — 눌림 보정 없음(0)">
            0
          </span>
        ) : (
          <span title={`위에 타이어 1개당 ${round1(s.compression)} mm 씩 낮아짐`}>
            {f1(s.compression)}
          </span>
        )
      },
    },
    {
      key: 'pick_z1',
      ...hdr('PickZ L1', 'PickZ@L1 (mm) — 1단 혼자 집는 높이 = UpperBidHeight − PickBeadOffset'),
      get: (i) => pickZAtL1(i) ?? -1,
      numeric: true,
      class: NARROW,
      priority: 2,
      cell: (i) => {
        const v = pickZAtL1(i)
        return v === null ? (
          faint()
        ) : (
          <span
            title={`1단 타이어 혼자(위에 없음) 집는 높이 = UpperBidHeight ${round1(i.upper_bead_height)} − PickBeadOffset ${round1(pickBeadOffset(specOf(i)))}. 위에 타이어가 있으면 Compression 만큼 더 내려갑니다.`}
          >
            {f1(v)}
          </span>
        )
      },
    },
    {
      key: 'use',
      ...hdr('StockCells', 'StockCells — 이 코드를 쓰는 재고 칸 수'),
      get: (i) => usage.get(i.code)?.cells ?? 0,
      numeric: true,
      class: NARROW,
      priority: 1,
      cell: (i) => {
        const u = usage.get(i.code)
        const over = overCells.get(i.code) ?? 0
        if (!u) return faint('0')
        return over ? (
          <span
            className="rounded-sm bg-warn-soft px-1 text-warn-fg"
            title={`재고 ${u.cells}칸 · ${u.count}개 — StackMax 초과 ${over}칸`}
          >
            {u.cells} · 초과 {over}
          </span>
        ) : (
          <span title={`재고 ${u.cells}칸 · ${u.count}개`}>{u.cells}</span>
        )
      },
    },
    {
      key: 'note',
      label: 'Note',
      get: (i) => i.note,
      class: `text-content-muted ${NARROW}`,
      priority: 3,
    },
    {
      key: 'updated',
      ...hdr('Updated', 'UpdatedAt'),
      get: (i) => i.updated_at,
      class: NARROW,
      cell: (i) => <span className="font-mono text-content-muted">{when(i.updated_at)}</span>,
      priority: 3,
    },
  ]

  const openAdd = () =>
    setForm({ open: true, editing: false, initial: EMPTY_ITEM, key: Date.now() })
  const openEdit = () =>
    sel && setForm({ open: true, editing: true, initial: toItemUpsert(sel), key: Date.now() })
  const openDuplicate = () =>
    sel &&
    setForm({
      open: true,
      editing: false,
      initial: duplicateItem(toItemUpsert(sel), reg.items),
      key: Date.now(),
    })
  // 체크가 있으면 체크한 것 전부, 없으면 고른 행 하나.
  const openDelete = () => {
    const codes = picked.length > 0 ? picked : sel ? [sel.code] : []
    if (codes.length > 0) setDel(codes)
  }

  async function save(v: ItemUpsert) {
    if (!form.editing && reg.items.some((i) => i.code === v.code))
      throw new Error(`코드 ${v.code}는 이미 있습니다 — 편집으로 고치거나 다른 코드를 쓰세요`)
    if (form.editing) await api.itemUpdate(v.code, v)
    else await api.itemCreate(v)
    setForm((s) => ({ ...s, open: false }))
    setSelected(v.code)
    await reg.reload()
    toast.ok(`품목 ${v.code} 저장됨`)
  }

  async function remove(codes: number[]) {
    let ok = 0
    const failed: string[] = []
    for (const code of codes) {
      try {
        await api.itemDelete(code)
        ok++
      } catch (e) {
        failed.push(`${code}: ${errMsg(e)}`)
      }
    }
    setChecked(new Set())
    if (selected !== null && codes.includes(selected)) setSelected(null)
    await reg.reload()
    if (failed.length === 0) toast.ok(`품목 ${ok}건 삭제됨`)
    else toast.error(`품목 ${ok}건 삭제 · 실패 ${failed.length}건 — ${failed[0]}`)
  }

  const bulk = { stack_max: bulkValue(bulkStack), pallet_max: bulkValue(bulkPallet) }
  const bulkErrs = [
    ...(bulk.stack_max !== undefined ? bulkStackMaxErrors(pickedItems, bulk.stack_max) : []),
    ...(bulk.pallet_max !== undefined &&
    !(Number.isInteger(bulk.pallet_max) && bulk.pallet_max >= 0 && bulk.pallet_max <= 255)
      ? ['PalletMax 는 0..255 정수']
      : []),
  ]
  const canBulk =
    picked.length > 0 &&
    (bulk.stack_max !== undefined || bulk.pallet_max !== undefined) &&
    bulkErrs.length === 0

  async function applyBulk() {
    setBulkBusy(true)
    try {
      const r = await api.itemsBulkSpec({ codes: picked, ...bulk })
      toast.ok(
        `품목 ${r.updated}건에 ${[
          bulk.stack_max !== undefined ? `StackMax ${bulk.stack_max}` : '',
          bulk.pallet_max !== undefined ? `PalletMax ${bulk.pallet_max}` : '',
        ]
          .filter(Boolean)
          .join(' · ')} 적용됨`,
      )
      setBulkStack('')
      setBulkPallet('')
      await reg.reload()
    } catch (e) {
      toast.error(`일괄 적용 실패 — ${errMsg(e)}`)
    } finally {
      setBulkBusy(false)
    }
  }

  const delUsed = del ? usedCodes(del, usage) : []
  const canDelete = picked.length > 0 || !!sel

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="items-page">
      <ScreenHeader
        title="화물 규격"
        icon={<Ruler size={14} />}
        items={[
          { label: '규격', value: `${reg.items.length}건` },
          { label: '재고에 쓰는 코드', value: `${inUse}건` },
          { label: 'StackMax 초과 칸', value: `${overTotal}칸` },
          { label: '체크', value: `${picked.length}건` },
        ]}
      />
      <RegistryToolbar<Item>
        title="화물 규격"
        icon={<Ruler size={14} />}
        what="품목"
        rows={reg.items.length}
        dirty={0}
        selected={canDelete}
        disabledReason="행을 누르거나 체크하세요"
        io={IO}
        actions={toolbar}
        reload={reg.reload}
        onAdd={openAdd}
        onEdit={openEdit}
        onDelete={openDelete}
        extra={
          <>
            <Button
              size="sm"
              intent="ghost"
              icon={<Copy className="h-3.5 w-3.5" />}
              disabled={!sel}
              onClick={openDuplicate}
              title={sel ? `품목 ${sel.code}를 새 코드로 복제` : '복제 — 행을 하나 누르세요'}
              data-testid="items-duplicate"
            >
              복제
            </Button>
            <Button
              size="sm"
              intent="ghost"
              icon={<Gauge className="h-3.5 w-3.5" />}
              onClick={() => setDimsOpen(true)}
              title="측정 기록(MeasureItem)으로 InnerDiameter · UpperBeadHeight · Height 제안을 모아 검토 · 적용"
              data-testid="items-dims-review"
            >
              측정 반영
            </Button>
          </>
        }
      />
      <div className="flex flex-none flex-wrap items-center gap-3 border-b border-line-default px-2 py-1.5">
        <Input
          className="w-60"
          placeholder="Code · Name · Note 검색"
          value={q}
          onValueChange={setQ}
          aria-label="화물 규격 검색"
          data-testid="items-search"
        />
        <label className="flex items-center gap-1.5 text-xs text-content-tertiary">
          <input
            type="checkbox"
            checked={allShownPicked}
            disabled={rows.length === 0}
            onChange={toggleShown}
            data-testid="items-pick-all"
          />
          보이는 행 모두 체크
        </label>
        <span className="text-2xs text-content-muted tabular-nums">
          {q ? `${rows.length} / ${reg.items.length}건` : `${reg.items.length}건`}
        </span>
        <span className="flex-1" />
        <Switch
          inline
          label="측정 제안"
          checked={dimsOn}
          onCheckedChange={setDimsOn}
          data-testid="items-dims-on"
        />
      </div>
      {dimsOn && (dims.all.length > 0 || lastBatch.length > 0) ? (
        <div
          className="flex flex-none flex-wrap items-center gap-2 border-b border-line-default bg-surface-inset px-2 py-1"
          data-testid="items-dims-bar"
        >
          <span className="text-xs text-content-secondary tabular-nums">
            측정 제안 {dims.all.length}건
          </span>
          <span className="text-2xs text-content-muted tabular-nums">
            {`바로 적용 ${dimSafe.length} · 확인 필요 ${dims.all.length - dimSafe.length}`}
            {dimPick.size ? ` · 선택 ${dimSelected.length}` : ''}
          </span>
          <label className="flex items-center gap-1 text-2xs text-content-tertiary">
            <input
              type="checkbox"
              checked={dimOnly}
              onChange={(e) => setDimOnly(e.target.checked)}
              data-testid="items-dims-only"
            />
            제안 있는 행만
          </label>
          <select
            className="h-control-sm rounded-md border border-line-default bg-transparent px-1 text-2xs"
            value={String(dimWin)}
            onChange={(e) => setDimWin(Number(e.target.value))}
            aria-label="측정 창"
            title="제안 = 최근 N 개 측정의 중앙값"
          >
            {WINDOWS.map((w) => (
              <option key={w} value={String(w)}>
                최근 {w}개
              </option>
            ))}
          </select>
          <span className="flex-1" />
          {dimPick.size ? (
            <>
              <Button
                size="sm"
                intent="ghost"
                disabled={dimBusy}
                onClick={() => setDimPick(new Set())}
              >
                선택 해제
              </Button>
              <Button
                size="sm"
                intent={dimRisky(dimSelected).length ? 'outline' : 'primary'}
                icon={<Check className="h-3.5 w-3.5" />}
                disabled={dimBusy || dimSelected.length === 0}
                onClick={() => dimRequest('선택한 측정 제안 적용', dimSelected)}
                data-testid="items-dims-apply-picked"
              >
                선택 {dimSelected.length}건 적용
              </Button>
            </>
          ) : null}
          <Button
            size="sm"
            intent="primary"
            icon={<CheckCheck className="h-3.5 w-3.5" />}
            disabled={dimBusy || dimSafe.length === 0}
            title={`${TONE_LABEL.ok} · ${TONE_LABEL.info} 제안 전부(확인 필요 ${dims.all.length - dimSafe.length}건은 빼고)`}
            onClick={() => dimRequest('바로 적용 전체', dimSafe)}
            data-testid="items-dims-apply-safe"
          >
            바로 적용 전체 ({dimSafe.length})
          </Button>
          {lastBatch.length ? (
            <Button
              size="sm"
              intent="outline"
              icon={<Undo2 className="h-3.5 w-3.5" />}
              disabled={dimBusy}
              title={`방금 적용한 ${lastBatch.length}건을 모두 원래 값으로`}
              onClick={() => void dimRevert(lastBatch)}
              data-testid="items-dims-undo"
            >
              방금 적용 되돌리기 ({lastBatch.length})
            </Button>
          ) : null}
        </div>
      ) : null}
      {picked.length > 0 ? (
        <div
          className="flex flex-none flex-wrap items-end gap-2 border-b border-line-default bg-surface-inset px-2 py-1.5"
          data-testid="items-bulk"
        >
          <span className="pb-1.5 text-xs text-content-secondary">
            체크 {picked.length}건 일괄 편집
          </span>
          <Input
            className="w-28"
            label="StackMax"
            type="number"
            mono
            min={0}
            step="1"
            placeholder="그대로"
            value={bulkStack}
            onValueChange={setBulkStack}
            data-testid="items-bulk-stack"
          />
          <Input
            className="w-28"
            label="PalletMax"
            type="number"
            mono
            min={0}
            step="1"
            placeholder="그대로"
            value={bulkPallet}
            onValueChange={setBulkPallet}
            data-testid="items-bulk-pallet"
          />
          <Button
            size="sm"
            intent="primary"
            disabled={!canBulk}
            loading={bulkBusy}
            onClick={() => void applyBulk()}
            title={bulkErrs[0] ?? '빈칸은 그대로 둡니다 · 0 = 제한 없음'}
            data-testid="items-bulk-apply"
          >
            적용
          </Button>
          <span className="pb-2">
            <HelpTip title="일괄 편집" sections={BULK_HELP} />
          </span>
          {bulkErrs.length ? (
            <span className="pb-1.5 text-xs text-fault-fg">
              {bulkErrs[0]}
              {bulkErrs.length > 1 ? ` 외 ${bulkErrs.length - 1}건` : ''}
            </span>
          ) : null}
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1">
        <div className="min-h-0 min-w-0 flex-1 overflow-auto">
          {reg.error ? <p className="p-2 text-xs text-fault-fg">{reg.error}</p> : null}
          <ErrorBoundary label="화물 규격 표" resetKey={reg.items}>
            <DataTable
              rows={rows}
              columns={columns}
              rowKey={(i) => String(i.code)}
              selected={selected === null ? null : String(selected)}
              onPick={(i) => pick(i.code === selected ? null : i.code)}
              loading={reg.loading}
              fit
              empty={q ? '검색 결과 없음' : '등록된 화물 규격 없음'}
              emptyHint={
                q
                  ? '검색어를 지우면 전체 규격이 보입니다.'
                  : '한 건씩 입력하거나 양식을 채워 한 번에 가져옵니다.'
              }
              emptyAction={
                q ? undefined : (
                  <>
                    <Button
                      size="sm"
                      intent="primary"
                      icon={<Plus className="h-3.5 w-3.5" />}
                      onClick={openAdd}
                      data-testid="items-empty-add"
                    >
                      직접 입력
                    </Button>
                    <Button
                      size="sm"
                      intent="outline"
                      icon={<FileSpreadsheet className="h-3.5 w-3.5" />}
                      onClick={() => toolbar.current?.importExcel()}
                      data-testid="items-empty-excel"
                    >
                      Excel 가져오기
                    </Button>
                  </>
                )
              }
              testid="items-table"
            />
          </ErrorBoundary>
        </div>
        {sel ? (
          <>
            <Splitter
              axis="col"
              invert
              size={detailW}
              min={DETAIL_MIN}
              max={DETAIL_MAX}
              label="표와 상세 경계"
              onResize={setDetailW}
              onCommit={(w) => {
                try {
                  localStorage.setItem(DETAIL_KEY, String(Math.round(w)))
                } catch {
                  /* 저장 못 해도 동작 */
                }
              }}
            />
            <aside
              className="flex min-h-0 min-w-0 flex-none flex-col bg-surface-panel"
              style={{ width: detailW, maxWidth: '70%' }}
              aria-label={`품목 ${sel.code} 상세`}
            >
              <ErrorBoundary label="품목 상세" resetKey={sel.code}>
                <ItemDetail
                  key={`${sel.code}:${sel.updated_at}`}
                  item={sel}
                  stock={stockRows}
                  onSaved={async () => {
                    await reg.reload()
                  }}
                  onClose={() => pick(null)}
                  onDirtyChange={onDirtyChange}
                  focus={
                    dimFocus && dimFocus.code === sel.code
                      ? { field: dimFocus.field, n: dimFocus.n }
                      : null
                  }
                />
              </ErrorBoundary>
            </aside>
          </>
        ) : null}
      </div>

      {form.open ? (
        <ItemForm
          key={form.key}
          open={form.open}
          onOpenChange={(o) => setForm((s) => ({ ...s, open: o }))}
          initial={form.initial}
          editing={form.editing}
          onSave={save}
        />
      ) : null}
      <DimsReviewDialog open={dimsOpen} onOpenChange={setDimsOpen} onApplied={reg.reload} />
      <ConfirmDialog
        open={dimConfirm !== null}
        onOpenChange={(o) => {
          if (!o) setDimConfirm(null)
        }}
        scope={dimConfirm && dimConfirm.rows.length > 1 ? 'selection' : 'single'}
        title={dimConfirm?.title ?? ''}
        confirmLabel={`${dimConfirm?.rows.length ?? 0}건 적용`}
        danger={!!dimConfirm && dimRisky(dimConfirm.rows).length > 0}
        onConfirm={() => dimConfirm && void dimApply(dimConfirm.rows)}
      >
        <div className="flex max-h-72 flex-col gap-0.5 overflow-y-auto text-xs tabular-nums">
          {(dimConfirm?.rows ?? []).map((a) => (
            <div
              key={dimKey(a.code, a.s.field)}
              className={safeTone(dimTone(a.s)) ? undefined : 'text-warn-fg'}
            >
              {confirmLine(a.code, a.s)}
            </div>
          ))}
          <div className="mt-1 text-content-muted">
            PLC 로 보내는 품목 값이라 다음 명령부터 G · Z 가 바뀝니다. 띠의 "방금 적용 되돌리기" 로
            한 번에 되돌릴 수 있습니다.
          </div>
        </div>
      </ConfirmDialog>
      <ConfirmDialog
        open={pendingPick !== undefined}
        onOpenChange={(o) => {
          if (!o) setPendingPick(undefined)
        }}
        scope="single"
        title="저장하지 않은 변경"
        confirmLabel="버리고 이동"
        onConfirm={() => {
          setDetailDirty(false)
          setSelected(pendingPick ?? null)
          setPendingPick(undefined)
        }}
      >
        <p className="text-sm">
          품목 {selected} 상세에 저장하지 않은 변경이 있습니다. 버리고{' '}
          {pendingPick === null ? '닫을까요' : `품목 ${pendingPick}(으)로 옮길까요`}?
        </p>
      </ConfirmDialog>
      <ConfirmDialog
        open={del !== null}
        onOpenChange={(o) => {
          if (!o) setDel(null)
        }}
        scope={del && del.length > 1 ? 'selection' : 'single'}
        title="화물 규격 삭제"
        danger
        confirmLabel={del && del.length > 1 ? `${del.length}건 삭제` : '삭제'}
        onConfirm={() => del && void remove(del)}
      >
        <div className="flex flex-col gap-2">
          <p>
            {del && del.length === 1
              ? `품목 ${del[0]} ${reg.items.find((i) => i.code === del[0])?.name ?? ''}을(를) 지울까요?`
              : `품목 ${del?.length ?? 0}건을 지울까요?`}
          </p>
          {del && del.length > 1 ? (
            <p className="font-mono text-xs text-content-muted">
              {del.slice(0, 12).join(', ')}
              {del.length > 12 ? ` 외 ${del.length - 12}건` : ''}
            </p>
          ) : null}
          {delUsed.length > 0 ? (
            <p className="rounded-md border border-warn bg-warn-soft px-2 py-1 text-xs text-warn-fg">
              재고가 쓰는 코드 — {delUsed.map((u) => `${u.code}(${u.usage.cells}칸)`).join(', ')}.
              지우면 그 칸의 품목 치수를 찾지 못합니다.
            </p>
          ) : null}
          <p className="text-xs">
            이 코드를 쓰는 시나리오 스텝은 제출 때 실패합니다. 되돌릴 수 없습니다.
          </p>
        </div>
      </ConfirmDialog>
    </div>
  )
}
