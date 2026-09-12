// 시나리오 편집기 — 이름/설명/반복 폼 + 스텝 조작 띠 + `StepGrid` + 검증 목록 + 파라미터 다이얼로그.
// draft의 진실원은 페이지(`ScenarioPage`)이고 여기서는 바뀐 문서를 올린다.
import { useRef, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  Copy,
  Download,
  Plus,
  Save,
  SlidersHorizontal,
  Trash2,
  Upload,
} from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { Switch } from '../../lib/ui/Switch'
import { Toolbar } from '../../lib/ui/Toolbar'
import { EmptyState } from '../../lib/ui/EmptyState'
import type { Cell, Item, ScenarioStep, ScenarioUpsert, Station, TaskParams } from '../../lib/types'
import {
  cloneStep,
  issueMap,
  moveStep,
  newStep,
  type ValidationIssue,
} from '../../lib/scenario/model'
import { StepGrid } from './StepGrid'
import { StepParamsDialog } from './StepParamsDialog'
import { ValidationList } from './ValidationList'

export interface ScenarioEditorProps {
  draft: ScenarioUpsert | null
  dirty: boolean
  saving: boolean
  issues: ValidationIssue[]
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  /** 이 시나리오가 지금 실행 중 — 편집은 되지만 안내를 띄운다. */
  running: boolean
  onChange: (next: ScenarioUpsert) => void
  onSave: () => void
  onValidate: () => void
  onImportFile: (file: File) => void
  onExport: (format: 'json' | 'csv') => void
}

export function ScenarioEditor({
  draft,
  dirty,
  saving,
  issues,
  cells,
  stations,
  items,
  running,
  onChange,
  onSave,
  onValidate,
  onImportFile,
  onExport,
}: ScenarioEditorProps) {
  const [selectedStepId, setSelectedStepId] = useState<string | null>(null)
  const [paramsStep, setParamsStep] = useState<ScenarioStep | null>(null)
  const fileRef = useRef<HTMLInputElement | null>(null)

  const steps = draft?.steps ?? []
  const selIdx = steps.findIndex((s) => s.id === selectedStepId)
  const issueByCell = issueMap(issues)

  const setSteps = (next: ScenarioStep[]) => draft && onChange({ ...draft, steps: next })
  const addStep = () => {
    if (!draft) return
    const base = selIdx >= 0 ? steps[selIdx] : null
    const st = newStep(
      base
        ? {
            type: base.type,
            target: base.target ? { ...base.target } : null,
            item_code: base.item_code,
            wait_for: base.wait_for,
            on_failure: base.on_failure,
          }
        : {},
    )
    const next = steps.slice()
    next.splice(selIdx >= 0 ? selIdx + 1 : steps.length, 0, st)
    setSteps(next)
    setSelectedStepId(st.id)
  }
  const dupStep = () => {
    if (selIdx < 0) return
    const c = cloneStep(steps[selIdx])
    const next = steps.slice()
    next.splice(selIdx + 1, 0, c)
    setSteps(next)
    setSelectedStepId(c.id)
  }
  const delStep = () => {
    if (selIdx < 0) return
    const next = steps.filter((_, i) => i !== selIdx)
    setSteps(next)
    setSelectedStepId(next[Math.min(selIdx, next.length - 1)]?.id ?? null)
  }
  const move = (dir: -1 | 1) => {
    if (selIdx < 0) return
    const next = moveStep(steps, selIdx, dir)
    if (next !== steps) setSteps(next)
  }
  // 다이얼로그가 보는 스텝은 draft의 최신 것 — 편집 중 목록이 바뀌어도 어긋나지 않게 id로 다시 찾는다.
  const liveParamsStep = paramsStep ? (steps.find((s) => s.id === paramsStep.id) ?? null) : null
  const setParams = (params: Partial<TaskParams>) => {
    if (!liveParamsStep) return
    setSteps(steps.map((s) => (s.id === liveParamsStep.id ? { ...s, params } : s)))
  }

  if (!draft) {
    return (
      <div className="flex h-full min-h-0 flex-col" data-testid="scenario-editor">
        <input
          ref={fileRef}
          type="file"
          accept=".json,.csv,application/json,text/csv"
          className="hidden"
          onChange={(e) => {
            const f = e.currentTarget.files?.[0]
            e.currentTarget.value = ''
            if (f) onImportFile(f)
          }}
        />
        <EmptyState
          title="시나리오를 선택하세요"
          hint="왼쪽 목록에서 고르거나 새로 만듭니다. JSON/CSV 파일을 가져올 수도 있습니다."
          action={
            <Button size="sm" icon={<Upload size={13} />} onClick={() => fileRef.current?.click()}>
              JSON/CSV 가져오기
            </Button>
          }
        />
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="scenario-editor">
      <input
        ref={fileRef}
        type="file"
        accept=".json,.csv,application/json,text/csv"
        className="hidden"
        data-testid="scenario-import-file"
        onChange={(e) => {
          const f = e.currentTarget.files?.[0]
          e.currentTarget.value = ''
          if (f) onImportFile(f)
        }}
      />
      <Toolbar
        title="편집"
        meta={
          <>
            {dirty ? (
              <span className="rounded bg-warn-soft px-1.5 py-0.5 text-warn-fg">변경됨</span>
            ) : (
              <span>저장됨</span>
            )}
            {running ? (
              <span className="rounded bg-ok-soft px-1.5 py-0.5 text-ok-fg">
                실행 중 — 변경은 다음 실행부터
              </span>
            ) : null}
          </>
        }
      >
        <Button
          size="sm"
          icon={<Upload size={13} />}
          title="JSON/CSV 파일에서 새 시나리오로 가져오기"
          onClick={() => fileRef.current?.click()}
        >
          가져오기
        </Button>
        <Button
          size="sm"
          icon={<Download size={13} />}
          disabled={!draft.id}
          title={draft.id ? 'JSON으로 내보내기' : '먼저 저장하세요'}
          onClick={() => onExport('json')}
        >
          JSON
        </Button>
        <Button
          size="sm"
          icon={<Download size={13} />}
          disabled={!draft.id}
          title={draft.id ? 'CSV로 내보내기' : '먼저 저장하세요'}
          onClick={() => onExport('csv')}
        >
          CSV
        </Button>
        <Button
          size="sm"
          icon={<CheckCircle2 size={13} />}
          onClick={onValidate}
          data-testid="scenario-validate"
        >
          검증
        </Button>
        <Button
          size="sm"
          intent="primary"
          icon={<Save size={13} />}
          loading={saving}
          disabled={!dirty && !!draft.id}
          onClick={onSave}
          data-testid="scenario-save"
        >
          저장
        </Button>
      </Toolbar>

      <div className="flex flex-wrap items-end gap-3 border-b border-line-subtle px-3 py-2">
        <Input
          label="이름"
          className="w-56"
          value={draft.name}
          onValueChange={(v) => onChange({ ...draft, name: v })}
          data-testid="scenario-name"
        />
        <Input
          label="설명"
          className="min-w-64 flex-1"
          value={draft.description}
          onValueChange={(v) => onChange({ ...draft, description: v })}
        />
        <Input
          label="반복"
          className="w-24"
          type="number"
          mono
          min={1}
          value={draft.repeat === 0 ? '' : String(draft.repeat)}
          disabled={draft.repeat === 0}
          placeholder={draft.repeat === 0 ? '∞' : ''}
          onValueChange={(v) => {
            const n = Math.trunc(Number(v))
            if (v.trim() !== '' && Number.isFinite(n) && n >= 1) onChange({ ...draft, repeat: n })
          }}
          data-testid="scenario-repeat"
        />
        <label className="flex h-8 items-center gap-2 text-xs">
          <Switch
            checked={draft.repeat === 0}
            label="무한 반복"
            testid="scenario-infinite"
            onCheckedChange={(on) => onChange({ ...draft, repeat: on ? 0 : 1 })}
          />
          무한
        </label>
      </div>

      <Toolbar
        title="스텝"
        meta={`${steps.length}개${selIdx >= 0 ? ` · ${selIdx + 1}번 선택` : ''}`}
        dense
      >
        <Button size="sm" icon={<Plus size={13} />} onClick={addStep} data-testid="step-add">
          행 추가
        </Button>
        <Button size="sm" icon={<Copy size={13} />} disabled={selIdx < 0} onClick={dupStep}>
          복제
        </Button>
        <Button
          size="sm"
          icon={<Trash2 size={13} />}
          disabled={selIdx < 0}
          onClick={delStep}
          data-testid="step-delete"
        >
          삭제
        </Button>
        <Button
          size="sm"
          icon={<ArrowUp size={13} />}
          disabled={selIdx <= 0}
          onClick={() => move(-1)}
          aria-label="위로"
        >
          위
        </Button>
        <Button
          size="sm"
          icon={<ArrowDown size={13} />}
          disabled={selIdx < 0 || selIdx >= steps.length - 1}
          onClick={() => move(1)}
          aria-label="아래로"
        >
          아래
        </Button>
        <Button
          size="sm"
          icon={<SlidersHorizontal size={13} />}
          disabled={selIdx < 0}
          onClick={() => selIdx >= 0 && setParamsStep(steps[selIdx])}
        >
          파라미터
        </Button>
      </Toolbar>

      <div className="min-h-0 flex-1 overflow-auto">
        <StepGrid
          steps={steps}
          cells={cells}
          stations={stations}
          items={items}
          issues={issueByCell}
          selectedId={selectedStepId}
          onSelect={setSelectedStepId}
          onChange={setSteps}
          onEditParams={(s) => {
            setSelectedStepId(s.id)
            setParamsStep(s)
          }}
        />
      </div>
      <ValidationList
        issues={issues}
        onPick={(i) => i !== null && steps[i] && setSelectedStepId(steps[i].id)}
      />

      <StepParamsDialog
        step={liveParamsStep}
        onOpenChange={(o) => !o && setParamsStep(null)}
        onChange={setParams}
      />
    </div>
  )
}
