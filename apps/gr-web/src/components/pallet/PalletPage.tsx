// 팔렛 패턴 화면 — 사양서 R4 팔렛타이징 패턴(Flow × Pattern)을 외경·Gap·Rotation·Mirror 로 펼쳐 보고,
// **품목의** 팔렛 패턴을 저장하고, 작업 명령 계획·시나리오로 보낸다.
//
// 귀속(결정 2026-09-21): 패턴 = 품목(좌표는 GR1·GR2 공통) · 드래그 방향 보정 = 로봇(헤드 방향) ·
// 팔렛 여부 = 스테이션.
//
// 왼쪽 입력 · 가운데 1600×1600 팔렛 그림(보기 방향 = 사양서 슬라이드 또는 기계 축) · 오른쪽 슬롯 표와 내보내기.
// 계산은 백엔드 `/api/pallet/plan` 한 곳 — 작업 명령 compose 와 같은 생성기라 미리보기와 제출이 갈리지 않는다.
// 현장 팔렛 스테이션은 아직 정해지지 않았다: 스테이션 없이 수동 중심으로도 미리보고, 팔렛 스테이션은 운전자가 켠다.
// 필드 이름은 데이터 이름 그대로(영문), 버튼·안내만 한국어.
//
// 왼쪽 띠에는 **매번 만지는 입력만** 선다(Station · Item · OuterDiameter · FlowIn/Out · Gap · Levels).
// 한 번 정해 두고 거의 안 바꾸는 것(Pattern · PalletSize · Rotation · Mirror · Note)은 `배치 옵션`
// 대화상자로 내려보내고, 띠에는 그 결과를 라벨+값 한 줄로 남긴다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Compass, Grid3x3, ListPlus, Pencil, Save, SlidersHorizontal } from 'lucide-react'
import { api } from '../../lib/api'
import { copyText } from '../../lib/clipboard'
import { nav } from '../../lib/nav'
import { palletApi } from '../../lib/pallet/api'
import {
  DEFAULT_FLOW,
  DEFAULT_GAP,
  DEFAULT_PALLET_SIZE,
  ROTATIONS,
  buildSteps,
  clampLevels,
  dirGrid,
  dragTypeByte,
  flowOf,
  hexByte,
  planCsv,
  viewAxes,
  type ItemPallet,
  type PalletPlan,
  type PalletSpec,
  type PalletStation,
  type RobotDirView,
  type PlanParams,
  type PlanSlot,
  type ViewMode,
} from '../../lib/pallet/model'
import { useRegistry } from '../../lib/registry'
import { robots } from '../../lib/robots'
import { robotLabel, withRobotChip } from '../../lib/robotContext'
import { RobotChip } from '../shared/RobotChip'
import { scenarioApi } from '../../lib/scenario/api'
import { toJson } from '../../lib/scenario/io'
import { useStore } from '../../lib/store'
import { planInbox } from '../../lib/task/planInbox'
import { toScenario } from '../../lib/task/plan'
import type { Defaults, Item, Station } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog, useUnsavedGuard } from '../../lib/ui/Dialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Field } from '../../lib/ui/Field'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import type { MenuItem } from '../../lib/ui/menu'
import { Pairs } from '../../lib/ui/Pair'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DRAG_DIR_HELP, ITEM_PALLET_HELP, PALLET_STATION_HELP, ROBOT_DIR_HELP } from './help'
import { PalletView } from './PalletView'
import { PatternEditor } from './PatternEditor'

const VIEW_KEY = 'gr-pallet-view'
/** 기본값에 드래그 거리가 없을 때 화살표만 이 길이로 그린다(작업에는 쓰지 않는다). */
const ARROW_FALLBACK = 150

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))
const num = (s: string): number | undefined => {
  if (s.trim() === '') return undefined
  const n = Number(s)
  return Number.isFinite(n) ? n : undefined
}
/** f32 왕복이 남기는 꼬리(457.20001…)를 자른다 — 요약과 표가 같은 자릿수로 선다. */
const n7 = (v: number): number => (Number.isFinite(v) ? Number(v.toPrecision(7)) : v)

/** 작업 종류에 맞는 품목 흐름 — 백엔드 `ItemPallet::flow_for` 와 같다. */
function flowFor(p: Pick<ItemPallet, 'flow_in' | 'flow_out'>, type: string): string | null {
  return type === 'DROP' ? (p.flow_out ?? p.flow_in) : (p.flow_in ?? p.flow_out)
}

const transformText = (t: { rotation: number; mirror_x: boolean; mirror_y: boolean }) =>
  `${t.rotation}°${t.mirror_x ? ' MX' : ''}${t.mirror_y ? ' MY' : ''}`

function readView(): ViewMode {
  try {
    return localStorage.getItem(VIEW_KEY) === 'machine' ? 'machine' : 'spec'
  } catch {
    return 'spec'
  }
}

const h3 = 'text-2xs font-semibold text-content-muted'

export default function PalletPage() {
  const stations = useRegistry<Station>(api.stations)
  const items = useRegistry<Item>(api.items)
  const itemPallets = useRegistry<ItemPallet>(palletApi.items)
  const palletStations = useRegistry<PalletStation>(palletApi.stations)
  const robotDirs = useRegistry<RobotDirView>(palletApi.robots)
  useStore(robots)
  useEffect(() => robots.start(), [])

  const [spec, setSpec] = useState<PalletSpec | null>(null)
  const [specError, setSpecError] = useState<string | null>(null)
  const [defaults, setDefaults] = useState<Defaults | null>(null)
  /** 패턴 저장소가 바뀔 때마다 올려 사양·계획을 다시 읽는다. */
  const [storeRev, setStoreRev] = useState(0)
  const [editing, setEditing] = useState(false)
  const [editDirty, setEditDirty] = useState(false)
  useEffect(() => {
    palletApi.spec().then(
      (v) => {
        setSpec(v)
        setSpecError(null)
      },
      (e) => setSpecError(errMsg(e)),
    )
  }, [storeRev])
  useEffect(() => {
    api.defaults().then(setDefaults, () => {
      /* 화살표만 기본 길이로 */
    })
  }, [])
  const storeChanged = useCallback(() => setStoreRev((n) => n + 1), [])
  // 편집을 끄는 길은 하나다 — 킷의 지킴이가 "버릴지" 판정하고, 화면은 그 물음만 그린다.
  const leaveEdit = useCallback(() => {
    setEditDirty(false)
    setEditing(false)
  }, [])
  const editGuard = useUnsavedGuard(editDirty, leaveEdit)
  const toggleEdit = () => {
    if (editing) editGuard.request('cancel')
    else setEditing(true)
  }

  // ── 입력 ──
  const [stationId, setStationId] = useState<number | null>(null)
  const [centerX, setCenterX] = useState('0')
  const [centerY, setCenterY] = useState('0')
  const [floorZ, setFloorZ] = useState('')
  const [itemCode, setItemCode] = useState<number | null>(null)
  const [odText, setOdText] = useState('')
  const [flowIn, setFlowIn] = useState<string>(DEFAULT_FLOW)
  const [flowOut, setFlowOut] = useState<string>('')
  /** 미리보기에 펼칠 쪽 — 입고(FlowIn, PICK) / 출하(FlowOut, DROP). */
  const [side, setSide] = useState<'in' | 'out'>('in')
  const [pattern, setPattern] = useState<'auto' | number>('auto')
  const [gapText, setGapText] = useState(String(DEFAULT_GAP))
  const [rotation, setRotation] = useState(0)
  const [mirrorX, setMirrorX] = useState(false)
  const [mirrorY, setMirrorY] = useState(false)
  const [sizeText, setSizeText] = useState(String(DEFAULT_PALLET_SIZE))
  const [note, setNote] = useState('')
  const [levelsText, setLevelsText] = useState('1')
  const [view, setView] = useState<ViewMode>(readView)
  const [selSeq, setSelSeq] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [optOpen, setOptOpen] = useState(false)
  const [askDelete, setAskDelete] = useState(false)
  const [robotOpen, setRobotOpen] = useState(false)

  const itemPallet = useMemo(
    () => (itemCode === null ? null : (itemPallets.items.find((p) => p.code === itemCode) ?? null)),
    [itemPallets.items, itemCode],
  )
  const stationPallet = stationId === null ? null : (palletStations.items.find((p) => p.station_id === stationId) ?? null)
  const stationOn = !!stationPallet?.enabled
  const item = itemCode === null ? null : (items.items.find((i) => i.code === itemCode) ?? null)
  const robotDir = robotDirs.items.find((r) => r.robot === robots.selected) ?? null

  // 품목을 고르면 입력을 그 품목의 팔렛 패턴(없으면 기본값)으로 채운다 — 저장은 버튼으로만.
  useEffect(() => {
    setFlowIn(itemPallet ? (itemPallet.flow_in ?? '') : DEFAULT_FLOW)
    setFlowOut(itemPallet?.flow_out ?? '')
    setPattern(itemPallet?.pattern ?? 'auto')
    setGapText(String(itemPallet?.gap ?? DEFAULT_GAP))
    setRotation(itemPallet?.rotation ?? 0)
    setMirrorX(itemPallet?.mirror_x ?? false)
    setMirrorY(itemPallet?.mirror_y ?? false)
    setSizeText(String(itemPallet?.pallet_size ?? DEFAULT_PALLET_SIZE))
    setNote(itemPallet?.note ?? '')
    setSide(itemPallet && !itemPallet.flow_in && itemPallet.flow_out ? 'out' : 'in')
  }, [itemPallet])

  /** 미리보기에 펼치는 흐름 — 고른 쪽이 비었으면 다른 쪽(작업 compose 와 같은 규칙). */
  const flow = (side === 'in' ? flowIn || flowOut : flowOut || flowIn) || DEFAULT_FLOW

  const changeView = useCallback((v: ViewMode) => {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }, [])

  const levels = clampLevels(num(levelsText))
  const params: PlanParams = {
    station: stationId ?? undefined,
    center_x: stationId === null ? num(centerX) : undefined,
    center_y: stationId === null ? num(centerY) : undefined,
    floor_z: stationId === null ? num(floorZ) : undefined,
    item_code: itemCode ?? undefined,
    od: num(odText),
    gap: num(gapText),
    flow,
    pattern,
    rotation,
    mirror_x: mirrorX,
    mirror_y: mirrorY,
    pallet_size: num(sizeText),
    levels,
    robot: robots.selected ?? undefined,
  }
  const paramsKey = JSON.stringify(params)
  const hasOd = (num(odText) ?? 0) > 0 || (item?.outer_diameter ?? 0) > 0

  // ── 계획(백엔드) ──
  const [plan, setPlan] = useState<PalletPlan | null>(null)
  const [planError, setPlanError] = useState<string | null>(null)
  useEffect(() => {
    if (!hasOd) {
      setPlan(null)
      setPlanError(null)
      return
    }
    let alive = true
    const p = JSON.parse(paramsKey) as PlanParams
    const timer = setTimeout(() => {
      palletApi.plan(p).then(
        (v) => {
          if (!alive) return
          setPlan(v)
          setPlanError(null)
        },
        (e) => {
          if (!alive) return
          setPlan(null)
          setPlanError(errMsg(e))
        },
      )
    }, 200)
    return () => {
      alive = false
      clearTimeout(timer)
    }
  }, [paramsKey, hasOd, itemPallets.items, palletStations.items, robotDirs.items, storeRev])

  // 편집으로 흐름이 지워지거나 이름이 바뀌면 생성 입력을 남은 흐름으로 옮긴다.
  useEffect(() => {
    if (!spec || spec.flows.length === 0) return
    if (flowIn && !flowOf(spec, flowIn)) setFlowIn(spec.flows[0].id)
    if (flowOut && !flowOf(spec, flowOut)) setFlowOut('')
  }, [spec, flowIn, flowOut])

  const flowDef = flowOf(spec, plan?.flow ?? flow)
  const axes = viewAxes(view, flowDef)
  const base = defaults?.base
  const dist = plan?.drag_kind === 'out' ? base?.drag_out_dist : base?.drag_in_dist
  const arrowDist = dist && dist > 0 ? dist : ARROW_FALLBACK

  // ── 품목 팔렛 패턴 ──
  const draftItem = {
    code: itemCode ?? 0,
    flow_in: flowIn || null,
    flow_out: flowOut || null,
    pattern: pattern === 'auto' ? null : pattern,
    gap: num(gapText) ?? DEFAULT_GAP,
    rotation,
    mirror_x: mirrorX,
    mirror_y: mirrorY,
    pallet_size: num(sizeText) ?? DEFAULT_PALLET_SIZE,
    note,
  }
  const itemDirty =
    !itemPallet ||
    itemPallet.flow_in !== draftItem.flow_in ||
    itemPallet.flow_out !== draftItem.flow_out ||
    itemPallet.pattern !== draftItem.pattern ||
    itemPallet.gap !== draftItem.gap ||
    itemPallet.rotation !== draftItem.rotation ||
    itemPallet.mirror_x !== draftItem.mirror_x ||
    itemPallet.mirror_y !== draftItem.mirror_y ||
    itemPallet.pallet_size !== draftItem.pallet_size ||
    itemPallet.note !== draftItem.note

  async function saveItemPallet() {
    if (itemCode === null) return
    setBusy(true)
    try {
      const saved = await palletApi.saveItem(draftItem)
      await itemPallets.reload()
      toast.ok(`품목 ${saved.code} 팔렛 패턴 저장 — FlowIn ${saved.flow_in ?? '—'} · FlowOut ${saved.flow_out ?? '—'}`)
    } catch (e) {
      toast.error(`품목 팔렛 패턴 저장 실패 — ${errMsg(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function deleteItemPallet() {
    if (itemCode === null || !itemPallet) return
    setBusy(true)
    try {
      await palletApi.deleteItem(itemCode)
      await itemPallets.reload()
      toast.ok(`품목 ${itemCode} 팔렛 패턴을 지웠습니다`)
    } catch (e) {
      toast.error(`품목 팔렛 패턴 삭제 실패 — ${errMsg(e)}`)
    } finally {
      setBusy(false)
    }
  }

  // ── 팔렛 스테이션 — 스위치 하나라 누르면 바로 저장한다 ──
  async function setStationOn(on: boolean) {
    if (stationId === null) return
    setBusy(true)
    try {
      await palletApi.saveStation({ station_id: stationId, enabled: on, note: stationPallet?.note ?? '' })
      await palletStations.reload()
      toast.ok(
        on
          ? `스테이션 ${stationId} 팔렛 스테이션 켬 — 작업 명령이 품목 패턴의 슬롯을 씁니다`
          : `스테이션 ${stationId} 팔렛 스테이션 끔 — 작업 명령은 기존 스테이션 동작`,
      )
    } catch (e) {
      toast.error(`팔렛 스테이션 저장 실패 — ${errMsg(e)}`)
    } finally {
      setBusy(false)
    }
  }

  // ── 로봇 드래그 방향 ──
  async function saveRobotDir(r: RobotDirView, patch: Partial<RobotDirView>) {
    const next = { ...r, ...patch }
    try {
      await palletApi.saveRobot({ robot: next.robot, rotation: next.rotation, mirror_x: next.mirror_x, mirror_y: next.mirror_y, note: next.note })
      await robotDirs.reload()
    } catch (e) {
      toast.error(`${r.name} 드래그 방향 저장 실패 — ${errMsg(e)}`)
    }
  }

  // ── 내보내기 ──
  // 작업 명령 compose 는 저장된 품목 팔렛 패턴 + 품목 외경 + 로봇 방향으로 슬롯을 다시 고른다 — 화면 값이 그와 다르면 막는다.
  const itemOd = item?.outer_diameter ?? 0
  const manualOd = num(odText)
  const sendReason =
    stationId === null
      ? '스테이션을 고르세요 (수동 중심은 미리보기만)'
      : itemCode === null
        ? '품목을 고르세요 (패턴은 품목에 속합니다)'
        : !stationOn
          ? '이 스테이션을 팔렛 스테이션으로 켜야 작업 명령이 슬롯 위치를 씁니다'
          : !itemPallet
            ? '이 품목의 팔렛 패턴을 먼저 저장하세요'
            : itemDirty
              ? '품목 팔렛 패턴 입력이 저장값과 다릅니다 — 먼저 저장하세요'
              : manualOd !== undefined && manualOd !== itemOd
                ? 'OuterDiameter 수동 값이 품목 값과 다릅니다 — 비우세요'
                : !plan
                  ? '계획이 없습니다'
                  : flowFor(itemPallet, plan.type) !== plan.flow
                    ? `${plan.type} 작업은 ${flowFor(itemPallet, plan.type)} 흐름을 씁니다 — 미리보기 쪽(입고/출하)을 바꾸세요`
                    : plan.errors.length > 0
                      ? '배치 오류(겹침)가 있습니다'
                      : null

  function steps() {
    if (!plan || stationId === null) return []
    return buildSteps(plan, { station: stationId, item_code: itemCode, levels, robot: robots.selected })
  }

  function addToPlan() {
    const s = steps()
    if (!plan || s.length === 0) return
    planInbox.push(s)
    // 스텝마다 로봇이 실린다(`robots.selected`) — 어느 호기의 계획인지 토스트가 말한다.
    toast.ok(withRobotChip(robots.chip, `${s.length}개 스텝(${plan.type} · ${levels}단)을 작업 명령 계획에 보냈습니다`))
    nav.go('task')
  }

  async function exportScenario() {
    const s = steps()
    if (!plan || s.length === 0) return
    setBusy(true)
    try {
      const doc = toScenario(
        s,
        `Pallet ${plan.flow} P${plan.pattern} Item ${itemCode} Station ${stationId}`,
        `OuterDiameter ${plan.od} · Gap ${plan.gap} · Rotation ${plan.transform.rotation} · Levels ${levels}`,
      )
      const file = new File([toJson(doc)], 'pallet-scenario.json', { type: 'application/json' })
      const created = await scenarioApi.importFile(file, 'json')
      toast.ok(withRobotChip(robots.chip, `시나리오 "${created.name}" (${s.length}스텝)을 만들었습니다`))
      nav.goScenario(created.id)
    } catch (e) {
      toast.error(`시나리오 내보내기 실패 — ${errMsg(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function copyCsv() {
    if (!plan) return
    if (await copyText(planCsv(plan))) toast.ok(`슬롯 ${plan.slots.length}행을 CSV 로 복사했습니다`)
    else toast.error('클립보드에 복사하지 못했습니다')
  }

  // ── 표 ──
  const columns: Column<PlanSlot>[] = [
    { key: 'seq', label: 'Seq', get: (s) => s.seq, numeric: true, priority: 1 },
    { key: 'slot', label: 'Slot', get: (s) => s.slot, priority: 1 },
    { key: 'offset_x', label: 'OffsetX (mm)', get: (s) => s.offset_x, cell: (s) => n7(s.offset_x), numeric: true, priority: 2 },
    { key: 'offset_y', label: 'OffsetY (mm)', get: (s) => s.offset_y, cell: (s) => n7(s.offset_y), numeric: true, priority: 2 },
    { key: 'x', label: 'X (mm)', get: (s) => s.x, cell: (s) => n7(s.x), numeric: true, priority: 1 },
    { key: 'y', label: 'Y (mm)', get: (s) => s.y, cell: (s) => n7(s.y), numeric: true, priority: 1 },
    {
      key: 'drag_dir',
      label: 'DragDir',
      get: (s) => s.drag_dir,
      cell: (s) => {
        const layout = s.layout_drag_dir ?? s.drag_dir
        const notes = [
          ...(layout !== s.spec_drag_dir ? [`spec ${s.spec_drag_dir}`] : []),
          ...(layout !== s.drag_dir ? [`item ${layout}`] : []),
        ]
        return notes.length === 0 ? s.drag_dir : `${s.drag_dir} (${notes.join(' · ')})`
      },
      numeric: true,
      priority: 1,
    },
    { key: 'drag_type', label: 'DragType', get: (s) => s.drag_type, cell: (s) => hexByte(s.drag_type), numeric: true, priority: 2 },
    ...(plan?.z_levels ?? []).map(
      (z): Column<PlanSlot> => ({
        key: `z${z.level}`,
        label: `Z L${z.level} (mm)`,
        get: () => z.z,
        cell: () => (z.z === null ? '—' : n7(z.z)),
        numeric: true,
        priority: z.level <= 2 ? 2 : 3,
      }),
    ),
  ]

  const grid = dirGrid(axes)
  const selectedSlot = plan?.slots.find((s) => s.seq === selSeq) ?? null
  const hasZ = !!plan && plan.z_levels.some((z) => z.z !== null)

  /** 띠에 남기는 배치 한 줄 — 대화상자 안의 값들을 라벨+값으로 되비춘다. */
  const layout = `${pattern === 'auto' ? 'auto' : `P${pattern}`} · ${sizeText} · ${rotation}°${mirrorX ? ' MX' : ''}${mirrorY ? ' MY' : ''}`

  const summary: StatItem[] = plan
    ? [
        { label: 'Flow', value: `${plan.flow}${plan.reference ? ' (참고)' : ''}` },
        { label: 'Pattern', value: `${plan.pattern}${plan.pattern_auto ? ' auto' : ''}`, hint: `${plan.od_range[0]}~${plan.od_range[1]}` },
        { label: 'Type', value: `${plan.type} · Drag-${plan.drag_kind}` },
        { label: 'OuterDiameter (mm)', value: n7(plan.od) },
        { label: 'Gap (mm)', value: n7(plan.gap) },
        { label: 'Pitch (mm)', value: n7(plan.pitch) },
        { label: 'MinDistance (mm)', value: n7(plan.min_distance) },
        { label: 'Center (mm)', value: `${n7(plan.center[0])}, ${n7(plan.center[1])}`, hint: plan.center_source },
        { label: 'Rotation', value: transformText(plan.transform) },
        { label: 'Robot DragDir', value: transformText(plan.robot_dir), hint: robotDir?.name },
        { label: 'DragDist (mm)', value: dist && dist > 0 ? n7(dist) : '—', hint: dist && dist > 0 ? undefined : `그림 ${ARROW_FALLBACK}` },
        { label: 'Drawn D / Gap (mm)', value: `${n7(plan.drawn.d_mm)} / ${n7(plan.drawn.gap_mm)}` },
        { label: 'GR2 margin (mm)', value: plan.area ? n7(plan.area.margin) : '—', hint: plan.area ? `TaskType ${plan.area.task_type}` : undefined },
      ]
    : []

  const itemMenu = (): MenuItem[] => [
    {
      label: 'Note · 배치 옵션…',
      disabled: itemCode === null ? '품목을 고르세요' : undefined,
      run: () => setOptOpen(true),
    },
    {
      label: '품목 팔렛 패턴 삭제',
      danger: true,
      disabled: !itemPallet ? '저장된 팔렛 패턴이 없습니다' : busy ? '처리 중입니다' : undefined,
      run: () => setAskDelete(true),
    },
  ]

  const exportMenu = (): MenuItem[] => [
    {
      label: `시나리오로 내보내기 — ${robots.chip.name}`,
      disabled: sendReason ?? (busy ? '처리 중입니다' : undefined),
      run: () => void exportScenario(),
    },
    {
      label: 'CSV 복사',
      hint: `${plan?.slots.length ?? 0}행`,
      disabled: plan ? undefined : '계획이 없습니다',
      run: () => void copyCsv(),
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="pallet-page">
      <ScreenHeader
        title="팔렛 패턴"
        icon={<Grid3x3 size={14} />}
        trailing={
          <div className="flex items-center gap-2">
            <Segmented
              value={view}
              onChange={changeView}
              ariaLabel="보기 방향"
              compact
              options={[
                { id: 'spec', label: '사양서 방향', title: '사양서 슬라이드와 같은 화면 방향' },
                { id: 'machine', label: '기계 축', title: 'X+ 오른쪽 · Y+ 위' },
              ]}
            />
            <Button
              size="sm"
              intent={editing ? 'primary' : 'outline'}
              icon={<Pencil size={14} />}
              aria-pressed={editing}
              title={editing ? '생성 보기로 돌아갑니다' : '흐름·패턴·슬롯·DragDir 을 고칩니다 (사양서 R4 는 기준본으로 남습니다)'}
              onClick={toggleEdit}
              data-testid="pallet-edit-toggle"
            >
              {editing ? '패턴 편집 끝' : '패턴 편집'}
            </Button>
          </div>
        }
      />
      <ConfirmDialog
        open={editGuard.asking}
        onOpenChange={(o) => !o && editGuard.keepEditing()}
        scope="single"
        title="저장하지 않은 변경"
        danger
        confirmLabel="버리고 나가기"
        onConfirm={editGuard.discard}
      >
        편집 중인 패턴의 변경을 버릴까요?
      </ConfirmDialog>
      {editing && (
        <ErrorBoundary label="패턴 편집">
          <PatternEditor
            view={view}
            gap={num(gapText) ?? DEFAULT_GAP}
            palletSize={num(sizeText) ?? DEFAULT_PALLET_SIZE}
            dragDist={arrowDist}
            initialFlow={flow}
            onChanged={storeChanged}
            onDirtyChange={setEditDirty}
          />
        </ErrorBoundary>
      )}
      <div className="flex min-h-0 flex-1 flex-wrap content-start gap-4 overflow-auto p-3" hidden={editing}>
        {/* 입력 */}
        <section className="flex w-72 shrink-0 flex-col gap-3" aria-label="입력">
          {specError && <p className="text-2xs text-danger-text">사양을 읽지 못했습니다 — {specError}</p>}
          <div className="flex flex-col gap-2">
            <h3 className={h3}>중심</h3>
            <Select
              label="Station"
              value={stationId ?? ''}
              onValueChange={(v) => setStationId(v === '' ? null : Number(v))}
              data-testid="pallet-station"
            >
              <option value="">수동 중심 (스테이션 없음)</option>
              {stations.items.map((s) => {
                const on = palletStations.items.some((x) => x.station_id === s.id && x.enabled)
                return (
                  <option key={s.id} value={s.id}>
                    {`${s.id} · TaskType ${s.task_type} · (${s.info.position[0]}, ${s.info.position[1]})${on ? ' · pallet' : ''}`}
                  </option>
                )
              })}
            </Select>
            {stationId !== null && (
              <div className="flex items-center gap-1">
                <Switch
                  inline
                  label="팔렛 스테이션"
                  checked={stationOn}
                  disabled={busy}
                  onCheckedChange={(v) => void setStationOn(v)}
                  data-testid="pallet-station-on"
                />
                <HelpTip title="팔렛 스테이션" text={PALLET_STATION_HELP} />
              </div>
            )}
            {stationId === null && (
              <div className="grid grid-cols-3 gap-2">
                <Input label="Center X" value={centerX} onValueChange={setCenterX} mono />
                <Input label="Center Y" value={centerY} onValueChange={setCenterY} mono />
                <Input label="Floor Z" value={floorZ} onValueChange={setFloorZ} mono placeholder="—" />
              </div>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <h3 className={h3}>타이어</h3>
            <Select
              label="Item"
              value={itemCode ?? ''}
              onValueChange={(v) => setItemCode(v === '' ? null : Number(v))}
              data-testid="pallet-item"
            >
              <option value="">품목 없음 (OuterDiameter 직접)</option>
              {items.items.map((i) => (
                <option key={i.code} value={i.code}>
                  {`${i.code} ${i.name} · OuterDiameter ${i.outer_diameter}${itemPallets.items.some((p) => p.code === i.code) ? ' · pallet' : ''}`}
                </option>
              ))}
            </Select>
            <Input
              label="OuterDiameter"
              value={odText}
              onValueChange={setOdText}
              mono
              placeholder={item ? String(item.outer_diameter) : 'mm'}
              hint={item ? '비우면 품목 값' : 'mm'}
              data-testid="pallet-od"
            />
          </div>

          <div className="flex flex-col gap-2 border-t border-line-default pt-3">
            <div className="flex items-center gap-1">
              <h3 className={h3}>품목 팔렛 패턴</h3>
              <HelpTip title="품목 팔렛 패턴" text={ITEM_PALLET_HELP} />
              <span className="flex-1" />
              <OverflowMenu items={itemMenu()} testid="pallet-item-menu" />
            </div>
            <div className="grid grid-cols-2 gap-2">
              <Select label="FlowIn" value={flowIn} onValueChange={setFlowIn} data-testid="pallet-flow-in">
                <option value="">—</option>
                {(spec?.flows ?? []).map((f) => (
                  <option key={f.id} value={f.id} title={`${f.name}${f.reference ? ' (참고)' : ''}`}>
                    {f.id}
                  </option>
                ))}
              </Select>
              <Select label="FlowOut" value={flowOut} onValueChange={setFlowOut} data-testid="pallet-flow-out">
                <option value="">—</option>
                {(spec?.flows ?? []).map((f) => (
                  <option key={f.id} value={f.id} title={`${f.name}${f.reference ? ' (참고)' : ''}`}>
                    {f.id}
                  </option>
                ))}
              </Select>
            </div>
            <Field label="미리보기">
              <Segmented
                value={side}
                onChange={(v) => setSide(v)}
                ariaLabel="미리보기 흐름"
                compact
                options={[
                  { id: 'in', label: 'In · PICK', title: flowIn || flowOut || DEFAULT_FLOW },
                  { id: 'out', label: 'Out · DROP', title: flowOut || flowIn || DEFAULT_FLOW },
                ]}
              />
            </Field>
            <div className="grid grid-cols-2 gap-2">
              <Input label="Gap" value={gapText} onValueChange={setGapText} mono hint="mm" data-testid="pallet-gap" />
              <Input label="Levels" value={levelsText} onValueChange={setLevelsText} mono hint="단" />
            </div>
            <div className="flex items-end gap-2">
              {/* 대화상자로 내려보낸 값을 띠에 라벨+값으로 되비춘다 — 숨겼다고 모르게 두지 않는다. */}
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="text-2xs font-medium text-content-muted">배치</span>
                <span className="truncate font-mono text-2xs tabular-nums text-content-secondary" data-testid="pallet-layout">
                  {layout}
                </span>
              </div>
              <Button
                size="sm"
                intent="ghost"
                icon={<SlidersHorizontal size={14} />}
                title="Pattern · PalletSize · Rotation · Mirror · Note"
                onClick={() => setOptOpen(true)}
                data-testid="pallet-options-open"
              >
                옵션
              </Button>
            </div>
            {itemCode !== null && (
              <>
                <p className="text-3xs text-content-faint">
                  {itemPallet
                    ? `저장됨 ${itemPallet.updated_at.slice(0, 19).replace('T', ' ')}${itemDirty ? ' · 입력이 바뀜' : ''}`
                    : '이 품목에 팔렛 패턴 없음'}
                </p>
                <Button
                  intent="primary"
                  size="sm"
                  icon={<Save size={14} />}
                  disabled={busy || !itemDirty || (!flowIn && !flowOut)}
                  title={!flowIn && !flowOut ? 'FlowIn 또는 FlowOut 을 고르세요' : itemDirty ? `품목 ${itemCode} 팔렛 패턴 저장` : '저장값과 같습니다'}
                  onClick={() => void saveItemPallet()}
                  data-testid="pallet-item-save"
                  className="self-start"
                >
                  품목 패턴 저장
                </Button>
              </>
            )}
          </div>

          <div className="flex items-end gap-2 border-t border-line-default pt-3">
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="flex items-center gap-1">
                <span className="text-2xs font-medium text-content-muted">로봇 DragDir</span>
                <HelpTip title="로봇 DragDir" text={ROBOT_DIR_HELP} />
              </span>
              <span className="truncate font-mono text-2xs tabular-nums text-content-secondary" data-testid="pallet-robot-dir">
                {robotDir ? `${robotDir.name} · ${transformText(robotDir)}` : '—'}
              </span>
            </div>
            <Button
              size="sm"
              intent="ghost"
              icon={<Compass size={14} />}
              title="로봇별 드래그 방향 보정"
              onClick={() => setRobotOpen(true)}
              data-testid="pallet-robot-dir-open"
            >
              설정
            </Button>
          </div>
        </section>

        {/* 그림 */}
        <section className="flex min-w-0 flex-1 basis-80 flex-col gap-2" aria-label="팔렛 그림">
          <ErrorBoundary label="팔렛 그림">
            {plan ? (
              <PalletView plan={plan} axes={axes} dragDist={arrowDist} selected={selSeq} onSelect={setSelSeq} />
            ) : (
              <EmptyState
                title={planError ? '계획을 만들지 못했습니다' : 'OuterDiameter 를 넣거나 품목을 고르세요'}
                hint={planError ?? '외경으로 Pattern 이 정해지고, Gap·Rotation·Mirror 로 슬롯 위치가 펼쳐집니다.'}
                icon={<Grid3x3 size={20} />}
              />
            )}
          </ErrorBoundary>
          {plan && (
            <>
              <StatRow items={summary} testid="pallet-summary" className="rounded-md border border-line-default" bordered={false} />
              {plan.errors.length > 0 && (
                <ul className="flex flex-col gap-0.5 text-2xs text-danger-text" data-testid="pallet-errors">
                  {plan.errors.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              )}
              {plan.warnings.length > 0 && (
                <ul className="flex flex-col gap-0.5 text-2xs text-warn-fg" data-testid="pallet-warnings">
                  {plan.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              )}
              {(plan.notes.length > 0 || (flowDef?.notes.length ?? 0) > 0) && (
                <details className="text-2xs text-content-muted">
                  <summary className="cursor-pointer">사양서 메모</summary>
                  <ul className="mt-1 flex flex-col gap-0.5">
                    {[...(flowDef?.notes ?? []), ...plan.notes].map((n) => (
                      <li key={n}>{n}</li>
                    ))}
                  </ul>
                </details>
              )}
            </>
          )}
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-1">
              <span className={h3}>DragDir</span>
              <HelpTip title="DragDir" text={DRAG_DIR_HELP} />
            </div>
            {/* 손으로 짠 격자를 남긴다 — 행·열이 데이터가 아니라 **방향**이라(위/아래·좌/우) 표가 아니다. */}
            <table className="border-collapse text-2xs" aria-label="DragDir 코드 (이 보기 방향)">
              <tbody>
                {grid.map((row, i) => (
                  <tr key={i}>
                    {row.map((d, j) => (
                      // design-lint-allow: no-control-height — 컨트롤이 아니라 DragDir 코드 격자의 칸이다(밀도로 줄면 여덟 칸이 한 덩어리로 붙는다)
                      <td key={j} className="h-10 w-14 border border-line-default text-center font-mono tabular-nums">
                        {d === null ? '' : (
                          <span className="flex flex-col leading-tight">
                            <span className="text-content-primary">{d}</span>
                            <span className="text-3xs text-content-faint">{hexByte(dragTypeByte(d))}</span>
                          </span>
                        )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        {/* 슬롯 표 */}
        <section className="flex min-w-0 flex-1 basis-80 flex-col gap-2" aria-label="슬롯">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              intent="primary"
              size="sm"
              icon={<ListPlus size={14} />}
              disabled={busy || sendReason !== null}
              title={sendReason ?? `${levels}단 × ${plan?.slots.length ?? 0}슬롯을 ${robotLabel(robots.chip)} 작업 명령 계획 끝에 추가`}
              onClick={addToPlan}
              data-testid="pallet-add-plan"
            >
              계획에 추가
            </Button>
            {/* 계획·시나리오로 보내는 스텝에 실리는 로봇 — 보내기 전에 눈에 띄어야 한다. */}
            <RobotChip chip={robots.chip} prefix="대상" testid="pallet-robot" />
            <span className="flex-1" />
            {hasZ && plan && (
              <HelpTip
                title="Z"
                align="right"
                text={`스테이션 Floor ${plan.floor_z} 위 단별 스택 규칙(${plan.type} · grip ${plan.grip_ref}). 품목 단별 표를 켜 두었으면 그 값입니다.`}
              />
            )}
            <OverflowMenu items={exportMenu()} testid="pallet-export-menu" />
          </div>
          <ErrorBoundary label="슬롯 표">
            <DataTable
              rows={plan ? [...plan.slots].sort((a, b) => a.seq - b.seq) : []}
              columns={columns}
              rowKey={(s) => s.slot}
              onPick={(s) => setSelSeq(selSeq === s.seq ? null : s.seq)}
              selected={selectedSlot?.slot ?? null}
              fit
              stickyHeader
              empty="슬롯 없음"
              emptyHint="OuterDiameter 를 넣거나 품목을 고르세요"
              testid="pallet-table"
            />
          </ErrorBoundary>
        </section>
      </div>

      <Dialog
        open={optOpen}
        onOpenChange={setOptOpen}
        title="배치 옵션"
        meta={<span className="font-mono tabular-nums">{layout}</span>}
        size="sm"
        testid="pallet-options"
        // 값이 켜는 즉시 반영되는 팝업이라 "적용/취소" 가 거짓말이다 — 킷이 닫기 하나를 그린다.
        closeLabel="닫기"
      >
        <div className="flex flex-col gap-3">
          <Select
            label="Pattern"
            value={pattern}
            onValueChange={(v) => setPattern(v === 'auto' ? 'auto' : Number(v))}
            data-testid="pallet-pattern"
          >
            <option value="auto">auto (OuterDiameter)</option>
            {(flowOf(spec, flow)?.patterns ?? []).map((p) => (
              <option key={p.pattern} value={p.pattern}>
                {`${p.pattern} (${p.od_min}~${p.od_max})`}
              </option>
            ))}
          </Select>
          <Input label="PalletSize" value={sizeText} onValueChange={setSizeText} mono hint="mm" />
          <Field label="Rotation">
            <Segmented
              value={String(rotation)}
              onChange={(v) => setRotation(Number(v))}
              ariaLabel="Rotation"
              compact
              options={ROTATIONS.map((r) => ({ id: String(r), label: `${r}°` }))}
            />
          </Field>
          <div className="flex gap-4">
            <Switch inline label="MirrorX" checked={mirrorX} onCheckedChange={setMirrorX} />
            <Switch inline label="MirrorY" checked={mirrorY} onCheckedChange={setMirrorY} />
          </div>
          {itemCode !== null && (
            <Input label="Note" value={note} onValueChange={setNote} hint="품목 팔렛 패턴에 함께 저장됩니다" />
          )}
        </div>
      </Dialog>

      <Dialog
        open={robotOpen}
        onOpenChange={setRobotOpen}
        title="로봇 DragDir"
        size="sm"
        testid="pallet-robot-dir"
        // 바꾸는 즉시 저장된다 — 닫기 하나.
        closeLabel="닫기"
      >
        <div className="flex flex-col gap-4">
          {robotDirs.items.length === 0 && <span className="text-2xs text-content-muted">설정된 로봇이 없습니다</span>}
          {robotDirs.items.map((r) => (
            <div key={r.robot} className="flex flex-col gap-2">
              <span className="text-2xs font-semibold text-content-secondary">{`${r.name} (${r.plc})`}</span>
              <Field label="Rotation">
                <Segmented
                  value={String(r.rotation)}
                  onChange={(v) => void saveRobotDir(r, { rotation: Number(v) })}
                  ariaLabel={`${r.name} Rotation`}
                  compact
                  options={ROTATIONS.map((x) => ({ id: String(x), label: `${x}°` }))}
                />
              </Field>
              <div className="flex gap-4">
                <Switch inline label="MirrorX" checked={r.mirror_x} onCheckedChange={(v) => void saveRobotDir(r, { mirror_x: v })} />
                <Switch inline label="MirrorY" checked={r.mirror_y} onCheckedChange={(v) => void saveRobotDir(r, { mirror_y: v })} />
              </div>
            </div>
          ))}
        </div>
      </Dialog>

      <ConfirmDialog
        open={askDelete}
        onOpenChange={setAskDelete}
        scope="single"
        title="품목 팔렛 패턴 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => void deleteItemPallet()}
      >
        <p className="mb-2">저장된 품목 팔렛 패턴을 지울까요?</p>
        <Pairs
          layout="stacked"
          items={[
            { label: 'Item', value: itemCode },
            { label: 'FlowIn', value: itemPallet?.flow_in ?? '—' },
            { label: 'FlowOut', value: itemPallet?.flow_out ?? '—' },
          ]}
        />
      </ConfirmDialog>
    </div>
  )
}
