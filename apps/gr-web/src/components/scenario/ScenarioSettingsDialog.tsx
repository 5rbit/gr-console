// 시나리오 설정 — 이름 · 설명 · 반복. 예전에는 편집기 위에 입력칸 넷이 늘 펼쳐져 있었는데,
// 한 시나리오에서 한 번 정하고 다시 건드리지 않는 값들이라 그 줄은 대부분 자리만 먹었다.
//
// 대화상자는 **열리면서 이름에 포커스**가 가고, Enter로 닫고(적용), Escape로 취소한다. 값은 즉시
// draft에 반영되지 않는다 — 취소가 취소로 남아야 "잘못 눌렀다"가 복구된다(저장은 바깥 `저장`이다).
import { useEffect, useState } from 'react'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Switch } from '../../lib/ui/Switch'
import type { ScenarioUpsert } from '../../lib/types'

export interface ScenarioSettingsDialogProps {
  open: boolean
  draft: ScenarioUpsert | null
  onOpenChange: (open: boolean) => void
  onApply: (next: ScenarioUpsert) => void
}

export function ScenarioSettingsDialog({
  open,
  draft,
  onOpenChange,
  onApply,
}: ScenarioSettingsDialogProps) {
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [repeat, setRepeat] = useState(1)
  const [infinite, setInfinite] = useState(false)

  useEffect(() => {
    if (!open || !draft) return
    setName(draft.name)
    setDescription(draft.description)
    setInfinite(draft.repeat === 0)
    setRepeat(draft.repeat === 0 ? 1 : draft.repeat)
  }, [open, draft])

  const apply = () => {
    if (!draft || name.trim() === '') return
    onApply({ ...draft, name: name.trim(), description, repeat: infinite ? 0 : repeat })
    onOpenChange(false)
  }

  const dirty =
    !!draft &&
    (name !== draft.name ||
      description !== draft.description ||
      (infinite ? 0 : repeat) !== draft.repeat)

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title="시나리오 설정"
      size="sm"
      dirty={dirty}
      submitLabel="적용"
      disabledReason={name.trim() === '' ? '이름을 입력하세요' : undefined}
      onSubmit={apply}
      testid="scenario-settings"
    >
      {draft ? (
        <>
          <Input label="Name" value={name} onValueChange={setName} data-testid="scenario-name" />
          <Input label="Description" value={description} onValueChange={setDescription} />
          <div className="flex items-end gap-3">
            <Input
              label="Repeat"
              className="w-28"
              type="number"
              mono
              min={1}
              value={infinite ? '' : String(repeat)}
              placeholder={infinite ? '∞' : ''}
              disabled={infinite}
              onValueChange={(v) => {
                const n = Math.trunc(Number(v))
                if (v.trim() !== '' && Number.isFinite(n) && n >= 1) setRepeat(n)
              }}
              data-testid="scenario-repeat"
            />
            <Switch
              inline
              label="무한 반복"
              checked={infinite}
              testid="scenario-infinite"
              onCheckedChange={setInfinite}
            />
          </div>
        </>
      ) : null}
    </FormDialog>
  )
}
