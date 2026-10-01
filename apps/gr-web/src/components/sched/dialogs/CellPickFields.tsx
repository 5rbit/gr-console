// 셀 자동 선택(CellPick) 입력 — 규칙의 From/To 자동과 정책의 InboundDest/OutboundSource 가 같이 쓴다.
import { useState } from 'react'
import type { CellPick } from '../../../lib/taskgen'
import { samePick } from '../../../lib/sched/policyModel'
import { FormDialog } from '../../../lib/ui/Dialog'
import { Input } from '../../../lib/ui/Input'
import { Select } from '../../../lib/ui/Select'
import { Switch } from '../../../lib/ui/Switch'

const num = (s: string) => (s.trim() === '' || !Number.isFinite(Number(s)) ? null : Number(s))

/** 구역 · 행 · 열 필터와 순서. `source` = 출발(순서 oldest/nearest), 아니면 도착(Near · SameItemFirst). */
export function CellPickFields({
  value,
  source = false,
  onChange,
}: {
  value: CellPick
  source?: boolean
  onChange: (p: CellPick) => void
}) {
  const p = value
  return (
    <div className="grid grid-cols-3 gap-2">
      <Input
        label="Section"
        value={p.section ?? ''}
        onValueChange={(x) => onChange({ ...p, section: num(x) })}
      />
      <Input
        label="RowMin"
        value={p.row_min ?? ''}
        onValueChange={(x) => onChange({ ...p, row_min: num(x) })}
      />
      <Input
        label="RowMax"
        value={p.row_max ?? ''}
        onValueChange={(x) => onChange({ ...p, row_max: num(x) })}
      />
      <Input
        label="ColMin"
        value={p.col_min ?? ''}
        onValueChange={(x) => onChange({ ...p, col_min: num(x) })}
      />
      <Input
        label="ColMax"
        value={p.col_max ?? ''}
        onValueChange={(x) => onChange({ ...p, col_max: num(x) })}
      />
      {source ? (
        <Select
          label="Order"
          value={p.order ?? 'oldest'}
          onValueChange={(o) => onChange({ ...p, order: o })}
        >
          <option value="oldest">oldest</option>
          <option value="nearest">nearest</option>
        </Select>
      ) : (
        <>
          <Input
            label="Near"
            placeholder="Station Id"
            title="이 스테이션에 가까운 셀 먼저 — 비우면 로봇 기준"
            value={p.near ?? ''}
            onValueChange={(x) => onChange({ ...p, near: num(x) })}
          />
          <Switch
            inline
            label="SameItemFirst"
            title="같은 품목이 쌓인 셀 먼저(아니면 빈 셀 먼저)"
            checked={!!p.same_item_first}
            onCheckedChange={(v) => onChange({ ...p, same_item_first: v })}
          />
        </>
      )}
    </div>
  )
}

/** CellPick 하나를 팝업으로 고친다 — 적용하면 `onApply`(저장은 호출자). */
export function CellPickDialog({
  title,
  value,
  source = false,
  onClose,
  onApply,
}: {
  title: string
  value: CellPick
  source?: boolean
  onClose: () => void
  onApply: (p: CellPick) => void
}) {
  const [p, setP] = useState<CellPick>(value)
  const bad =
    p.row_min != null && p.row_max != null && p.row_min > p.row_max
      ? 'RowMin > RowMax'
      : p.col_min != null && p.col_max != null && p.col_min > p.col_max
        ? 'ColMin > ColMax'
        : undefined
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={title}
      size="md"
      dirty={!samePick(p, value)}
      disabledReason={bad}
      onSubmit={() => onApply(p)}
      testid="sched-cellpick-dialog"
    >
      <CellPickFields value={p} source={source} onChange={setP} />
    </FormDialog>
  )
}
