// [+ 수동작업] 팝업 — 단일 생성 탭을 대신한다. 종류에 따라 칸이 바뀐다:
//   이송(출발 · 도착 · 화물 · 개수) · 측정(셀 · 종류) · 이동(대상) · 내려놓기(손 → 도착, 그리퍼에 든 것).
// 내려놓기의 [화물 지정] 은 콘솔 Hand 를 고르는 화물로 고치고 이송 지시를 열어 DROP 한다(콘솔 Hand 가 비었거나 다를 때 기본으로 켜짐).
// [대기열에 추가] 는 나갈 순서 끝에, [바로 맨 앞에] 는 우선을 올려 다음으로. 요청은 계획과 같은 `toRequest` 로 만든다.
import { useState } from 'react'
import { jobs as jobStore } from '../../lib/jobs/store'
import { targetCode } from '../../lib/jobs/model'
import { stepId, toRequest, type PlanStep } from '../../lib/task/plan'
import { robots } from '../../lib/robots'
import { stock as stockStore } from '../../lib/stock'
import type { Cell, Item, MeasureMode, Station, Target } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Dialog } from '../../lib/ui/Dialog'
import { Field } from '../../lib/ui/Field'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'
import { ItemPicker } from '../shared/ItemPicker'

export type ManualKind = 'transfer' | 'measure' | 'move' | 'drop'

export interface ManualPrefill {
  kind: ManualKind
  from?: Target | null
  to?: Target | null
  item?: number | null
  count?: number
}

const KINDS: { id: ManualKind; label: string }[] = [
  { id: 'transfer', label: '이송' },
  { id: 'measure', label: '측정' },
  { id: 'move', label: '이동' },
  { id: 'drop', label: '내려놓기' },
]

const key = (t: Target | null | undefined) => (t ? `${t.kind}:${t.id}` : '')
const parse = (v: string): Target | null => {
  const [kind, id] = v.split(':')
  return kind && id ? { kind: kind as Target['kind'], id: Number(id) } : null
}

export function ManualJobDialog({
  prefill,
  onClose,
  cells,
  stations,
  items,
}: {
  prefill: ManualPrefill | null
  onClose: () => void
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
}) {
  const [kind, setKind] = useState<ManualKind>('transfer')
  const [from, setFrom] = useState<Target | null>(null)
  const [to, setTo] = useState<Target | null>(null)
  const [item, setItem] = useState<number | null>(null)
  const [count, setCount] = useState(1)
  const [measure, setMeasure] = useState<'auto' | MeasureMode>('auto')
  const [busy, setBusy] = useState(false)
  /** 화물 지정 — null 이면 콘솔 Hand 와 맞는지로 정한다. */
  const [force, setForce] = useState<boolean | null>(null)
  const [seed, setSeed] = useState<ManualPrefill | null>(null)
  if (prefill !== seed) {
    setSeed(prefill)
    if (prefill) {
      setKind(prefill.kind)
      setFrom(prefill.from ?? null)
      setTo(prefill.to ?? null)
      const hand = robots.current ? stockStore.hand(robots.current.plc) : null
      const st = prefill.from ? stockStore.get(prefill.from.id) : null
      setItem(
        prefill.item ?? (prefill.kind === 'drop' ? hand?.item_code || null : st?.item_code || null),
      )
      setCount(prefill.count ?? (prefill.kind === 'drop' ? hand?.count || 1 : 1))
      setMeasure('auto')
      setForce(null)
    }
  }
  if (!prefill) return null

  const robot = robots.selected
  const name = robots.current?.name ?? '로봇'
  const options = [
    ...cells.map((c) => ({
      v: key({ kind: 'cell', id: c.id }),
      t: { kind: 'cell' as const, id: c.id },
    })),
    ...stations.map((s) => ({
      v: key({ kind: 'station', id: s.id }),
      t: { kind: 'station' as const, id: s.id },
    })),
  ]
  const label = (t: Target) => {
    const st = stockStore.get(t.id)
    return `${targetCode(t)}${st && st.count > 0 ? ` · ${st.item_code} ×${st.count}` : ''}`
  }
  const hand = robots.current ? stockStore.hand(robots.current.plc) : null
  const handFits = !!hand && hand.count > 0 && hand.item_code === item && hand.count === count
  const forceOn = kind === 'drop' && (force ?? !handFits)
  const needFrom = kind !== 'drop'
  const needTo = kind === 'transfer' || kind === 'drop'
  const why =
    needFrom && !from
      ? '출발(대상)을 고르세요'
      : needTo && !to
        ? '도착을 고르세요'
        : (kind === 'transfer' || kind === 'drop') && !item
          ? '화물을 고르세요'
          : count < 1
            ? '개수는 1 이상'
            : null

  function steps(): PlanStep[] {
    const base = { item_code: item, count, note: '', robot }
    if (kind === 'transfer')
      return [
        { id: stepId(), type: 'PICK', target: from!, ...base },
        { id: stepId(), type: 'DROP', target: to!, ...base },
      ]
    if (kind === 'drop') return [{ id: stepId(), type: 'DROP', target: to!, ...base }]
    if (kind === 'measure')
      return [
        {
          id: stepId(),
          type: 'MEASURE',
          target: from!,
          ...base,
          item_code: null,
          measure: measure === 'auto' ? null : measure,
        },
      ]
    return [{ id: stepId(), type: 'MOVE', target: from!, ...base, item_code: null }]
  }

  async function save(front: boolean) {
    if (why) return
    setBusy(true)
    try {
      const j = await jobStore.add({
        robot,
        steps: steps().map((s) => toRequest(s, robot)),
        force_cargo: forceOn || undefined,
      })
      if (front) await jobStore.front(j.id)
      toast.ok(`${name} 대기열 ${front ? '맨 앞' : '끝'}에 추가 · WorkId ${j.work_id ?? '-'}`)
      onClose()
    } catch (e) {
      toast.error(`${name}: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const targetSelect = (
    lbl: string,
    v: Target | null,
    set: (t: Target | null) => void,
    onlyCells = false,
    testid = '',
  ) => (
    <Field label={lbl}>
      <Select
        value={key(v)}
        onValueChange={(x) => set(parse(x))}
        aria-label={lbl}
        data-testid={testid}
      >
        <option value="">고르기 — 맵 클릭도 됨</option>
        {options
          .filter((o) => !onlyCells || o.t.kind === 'cell')
          .map((o) => (
            <option key={o.v} value={o.v}>
              {label(o.t)}
            </option>
          ))}
      </Select>
    </Field>
  )

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="수동작업"
      meta={<span className="text-xs text-content-muted">{name}</span>}
      size="sm"
      testid="manual-job"
      footer={
        <>
          <span className="flex-1" />
          <Button
            size="sm"
            disabled={!!why || busy}
            title={why ?? undefined}
            onClick={() => void save(true)}
            data-testid="manual-job-front"
          >
            바로 맨 앞에
          </Button>
          <Button
            size="sm"
            intent="primary"
            disabled={!!why || busy}
            title={why ?? undefined}
            onClick={() => void save(false)}
            data-testid="manual-job-add"
          >
            대기열에 추가
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-xs">
        <Segmented<ManualKind>
          ariaLabel="종류"
          value={kind}
          onChange={setKind}
          options={KINDS.map((k) => ({ id: k.id, label: k.label, testid: `manual-kind-${k.id}` }))}
        />
        {needFrom
          ? targetSelect(
              kind === 'transfer' ? '출발' : '대상',
              from,
              (t) => {
                setFrom(t)
                const st = t ? stockStore.get(t.id) : null
                if (kind === 'transfer' && st?.item_code) setItem(st.item_code)
              },
              kind === 'measure',
              'manual-from',
            )
          : null}
        {needTo ? targetSelect('도착', to, setTo, false, 'manual-to') : null}
        {kind === 'measure' ? (
          <Field label="측정 종류">
            <Select
              value={measure}
              onValueChange={(v) => setMeasure(v as 'auto' | MeasureMode)}
              aria-label="측정 종류"
            >
              <option value="auto">재고 기준(1개 Item · 2개 이상 SKU)</option>
              <option value="item">Item</option>
              <option value="sku">SKU</option>
              <option value="floor">Floor (바닥)</option>
            </Select>
          </Field>
        ) : null}
        {kind === 'transfer' || kind === 'drop' ? (
          <>
            <ItemPicker value={item} onChange={setItem} items={[...items]} allowNone={false} />
            <Field label="개수">
              <Input
                type="number"
                mono
                value={String(count)}
                min={1}
                max={20}
                onValueChange={(v) => setCount(Math.max(0, Math.round(Number(v) || 0)))}
                data-testid="manual-count"
              />
            </Field>
          </>
        ) : null}
        {kind === 'drop' ? (
          <div className="flex flex-col gap-1">
            <Switch
              inline
              label="화물 지정 — 콘솔 Hand 를 이 화물로 고치고 이송 지시를 열어 DROP"
              checked={forceOn}
              onCheckedChange={setForce}
              testid="manual-force-cargo"
            />
            <span className="text-content-muted">
              {`콘솔 Hand: ${hand && hand.count > 0 ? `${hand.item_code} ×${hand.count}` : '비어 있음'}${handFits ? ' (맞음)' : ''} · PLC 그리퍼 화물 데이터가 비어 있으면 보내기가 기다립니다 — PLC HMI Item 화면에서 지정(그대로 보내면 알람 6017)`}
            </span>
          </div>
        ) : null}
      </div>
    </Dialog>
  )
}
