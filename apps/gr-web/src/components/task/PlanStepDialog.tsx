// 계획 스텝 편집 팝업 — 표의 ✎ 가 연다. 초안은 팝업 안에만 있고 "적용" 이 그 스텝을 제자리에서 바꾼다.
//
// 검증은 작성 카드와 같은 규칙(`stepErrors` → `validateDraft`) + MOVE 여유 · 팔렛 슬롯. 제출은 여기서
// 하지 않는다 — 고친 스텝도 표의 "다음 1건 제출" 이 `toRequest(step, robots.selected)` 로 보낸다.
import { useState } from 'react'
import { targetKindsFor, itemRequired } from '../../lib/task/compose'
import { MOVE_CLEARANCE_DEFAULT, MOVE_MODES, moveOf, moveUsesItem } from '../../lib/task/moveMode'
import { retype, stepErrors, type PlanStep } from '../../lib/task/plan'
import { FormDialog } from '../../lib/ui/Dialog'
import { Field } from '../../lib/ui/Field'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { robots } from '../../lib/robots'
import type { RobotChipModel } from '../../lib/robotContext'
import type { Cell, Item, MeasureMode, MoveMode, Station, TaskType } from '../../lib/types'
import { ItemPicker } from '../shared/ItemPicker'
import { RobotChip } from '../shared/RobotChip'
import { TargetPicker, stockItemFor } from '../shared/TargetPicker'
import { FormErrors } from './forms'

const TYPES: TaskType[] = ['PICK', 'DROP', 'MEASURE', 'MOVE']

export interface PlanStepDialogProps {
  /** 고칠 스텝(없으면 닫힘). 여는 쪽이 `key={step.id}` 로 마운트해 초안을 새로 잡는다. */
  step: PlanStep
  no: number
  onClose: () => void
  onApply: (next: PlanStep) => void
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  /** 스텝에 로봇이 없을 때 갈 곳(카드의 선택 로봇). */
  robot: RobotChipModel
}

export function PlanStepDialog({
  step,
  no,
  onClose,
  onApply,
  cells,
  stations,
  items,
  robot,
}: PlanStepDialogProps) {
  const [d, setD] = useState<PlanStep>(step)
  const set = (p: Partial<PlanStep>) => setD((cur) => ({ ...cur, ...p }))
  const errors = stepErrors(d)
  const dirty = JSON.stringify(d) !== JSON.stringify(step)
  const isMove = d.type === 'MOVE'
  const mv = moveOf(d)
  const showItem = !isMove || moveUsesItem(mv)
  const chip = d.robot === null || d.robot === undefined ? robot : robots.chipOf(d.robot)
  const kinds = targetKindsFor(d.type)

  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`스텝 ${no} 편집`}
      size="md"
      meta={<RobotChip chip={chip} prefix="대상" testid="plan-step-robot" />}
      dirty={dirty}
      submitLabel="적용"
      disabledReason={errors[0]}
      onSubmit={() => {
        onApply(d)
        onClose()
      }}
      testid="plan-step"
    >
      <div className="flex flex-wrap items-end gap-2">
        <Select
          label="Type"
          value={d.type}
          // 새 종류가 못 받는 대상(MEASURE → 스테이션)은 그대로 두고 오류로 알린다 — 몰래 바꾸지 않는다.
          onValueChange={(v) => set(retype(d, v as TaskType))}
          data-testid="plan-step-type"
        >
          {TYPES.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </Select>
        {robots.multi ? (
          <Select
            label="Robot"
            value={d.robot === null || d.robot === undefined ? '' : String(d.robot)}
            onValueChange={(v) => set({ robot: v === '' ? null : Number(v) })}
            data-testid="plan-step-robot-select"
          >
            <option value="">기본</option>
            {robots.list.map((rb) => (
              <option key={rb.id} value={String(rb.id)}>
                {rb.name}
              </option>
            ))}
          </Select>
        ) : null}
      </div>

      {isMove ? (
        <div className="flex flex-wrap items-end gap-2">
          <Field label="MoveMode">
            <Segmented<MoveMode>
              ariaLabel="MoveMode"
              value={mv.mode}
              onChange={(mode) => set({ move: { ...mv, mode } })}
              options={MOVE_MODES.map((m) => ({ ...m, testid: `plan-step-move-${m.id}` }))}
            />
          </Field>
          {mv.mode === 'stack' ? (
            <Input
              label="Clearance (mm)"
              type="number"
              mono
              min={0}
              step="1"
              className="w-28"
              value={
                mv.clearance === null || mv.clearance === undefined ? '' : String(mv.clearance)
              }
              placeholder={String(MOVE_CLEARANCE_DEFAULT)}
              onValueChange={(s) =>
                set({ move: { ...mv, clearance: s.trim() === '' ? null : Number(s) } })
              }
              data-testid="plan-step-clearance"
            />
          ) : null}
          <span className="pb-2">
            <HelpTip
              title="MoveMode"
              text="Top: Z 9999 — 상단에서 XY 이동만. Avoid: Z 9999 + Avoid — X 만 이동(Y 유지). Stack: 스택 윗면 + Clearance 까지 내려갔다 올라옴."
            />
          </span>
        </div>
      ) : null}

      {d.type === 'MEASURE' ? (
        <div className="flex flex-wrap items-end gap-2">
          <Field label="Measure">
            <Segmented<'' | MeasureMode>
              ariaLabel="Measure"
              value={d.measure ?? ''}
              onChange={(m) => set({ measure: m === '' ? null : m })}
              options={[
                {
                  id: '',
                  label: 'auto',
                  title: '셀 재고 1개 = Item, 2개 이상 = SKU',
                  testid: 'plan-step-measure-auto',
                },
                { id: 'item', label: 'Item', testid: 'plan-step-measure-item' },
                { id: 'sku', label: 'SKU', testid: 'plan-step-measure-sku' },
              ]}
            />
          </Field>
          <span className="pb-2">
            <HelpTip
              title="Measure"
              text="Item: 타이어 한 개 치수(MeasureItem). SKU: 스택 전체의 단별 비드(MeasureSku). auto 는 그 스텝 시점 셀 재고로 고릅니다 — 1개면 Item, 2개 이상이면 SKU."
            />
          </span>
        </div>
      ) : null}

      <TargetPicker
        value={d.target}
        onChange={(t) => {
          if (t) setD((cur) => ({ ...cur, target: t, item_code: cur.item_code ?? stockItemFor(t) }))
        }}
        cells={[...cells]}
        stations={[...stations]}
        items={items}
        kinds={kinds}
      />

      {showItem ? (
        <div className="flex flex-wrap items-end gap-2">
          <ItemPicker
            value={d.item_code}
            onChange={(item_code) => set({ item_code })}
            items={[...items]}
            allowNone={!itemRequired(d.type)}
          />
          {!isMove ? (
            <Input
              label="Count"
              type="number"
              mono
              min={1}
              max={255}
              step="1"
              className="w-20"
              value={String(d.count)}
              onValueChange={(s) => set({ count: Number(s) })}
              data-testid="plan-step-count"
            />
          ) : null}
        </div>
      ) : null}

      {d.pallet ? (
        <div className="flex flex-wrap items-end gap-2">
          {d.pallet.auto ? (
            <Field label="Pallet">
              <span className="text-xs">auto</span>
            </Field>
          ) : (
            <>
              <Input
                label="Pallet Seq"
                type="number"
                mono
                min={1}
                step="1"
                className="w-24"
                value={String(d.pallet.seq ?? '')}
                onValueChange={(s) => set({ pallet: { ...d.pallet, seq: Number(s) } })}
              />
              <Input
                label="Pallet Level"
                type="number"
                mono
                min={1}
                step="1"
                className="w-24"
                value={String(d.pallet.level ?? '')}
                onValueChange={(s) => set({ pallet: { ...d.pallet, level: Number(s) } })}
              />
            </>
          )}
        </div>
      ) : null}

      <Input
        label="Note"
        value={d.note}
        onValueChange={(note) => set({ note })}
        placeholder="원장에 남는 한 줄"
      />

      <FormErrors list={errors} />
    </FormDialog>
  )
}
