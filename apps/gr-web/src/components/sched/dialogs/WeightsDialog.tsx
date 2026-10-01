// 상황별 가중(대상 · 품목 · 로봇) 편집 팝업 — `id:값` 한 줄씩. 적용하면 `onSave(config)`(저장은 호출자).
import { useState } from 'react'
import { EMPTY_WEIGHTS, formatMap, parseMap, type GenConfig } from '../../../lib/taskgen'
import { FormDialog } from '../../../lib/ui/Dialog'
import { Input } from '../../../lib/ui/Input'

export function WeightsDialog<C extends GenConfig>({
  cfg,
  onClose,
  onSave,
}: {
  cfg: C
  onClose: () => void
  onSave: (c: C) => void
}) {
  const w = { ...EMPTY_WEIGHTS, ...cfg.weights }
  const init = [formatMap(w.target), formatMap(w.item), formatMap(w.robot)]
  const [target, setTarget] = useState(init[0])
  const [item, setItem] = useState(init[1])
  const [robot, setRobot] = useState(init[2])
  const maps = [parseMap(target), parseMap(item), parseMap(robot)]
  const bad = maps.find((m) => 'error' in m) as { error: string } | undefined
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="생성 우선순위 가중치"
      size="md"
      submitLabel="적용"
      dirty={target !== init[0] || item !== init[1] || robot !== init[2]}
      disabledReason={bad?.error}
      onSubmit={() => {
        const [t, i, r] = maps as { ok: Record<string, number> }[]
        onSave({ ...cfg, weights: { target: t.ok, item: i.ok, robot: r.ok } })
      }}
      testid="sched-weights-dialog"
    >
      <div className="grid grid-cols-1 gap-2">
        <Input
          label="Weights.Target"
          placeholder="2101:10, 401:2"
          mono
          value={target}
          onValueChange={setTarget}
        />
        <Input
          label="Weights.Item"
          placeholder="2011:5"
          mono
          value={item}
          onValueChange={setItem}
        />
        <Input
          label="Weights.Robot"
          placeholder="2:1"
          mono
          value={robot}
          onValueChange={setRobot}
        />
      </div>
    </FormDialog>
  )
}
