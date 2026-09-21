// 시나리오 편집기 — **띠 하나 + 그리드**. 스텝 편집은 대량 작업이라 그리드에 그대로 두고,
// 한 스텝의 파라미터 덮어쓰기만 대화상자로 뺀다(칸 하나에 열 몇 개를 심을 수는 없다).
//
// 예전에는 띠가 셋이었다: 조작 띠(가져오기·JSON·CSV·검증·저장) · 이름/설명/반복 폼 · 스텝 띠(버튼 여섯).
// 이름·설명·반복은 한 번 정하고 안 건드리는 값이라 `설정…` 대화상자로 내렸고, 파일 조작과 검증은
// `⋯`로 모았다. 늘 보이는 것은 **지금 하는 일**뿐이다: 행을 더하고/지우고/옮기고, 저장한다.
import { useRef, useState } from 'react'
import { ArrowDown, ArrowUp, Copy, Plus, Save, Trash2, Upload } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
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
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
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
  /** 이름·설명·반복 대화상자를 연다(값의 주인은 페이지다). */
  onOpenSettings: () => void
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
  onOpenSettings,
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

  const picker = (
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
  )

  if (!draft) {
    return (
      <div className="flex h-full min-h-0 flex-col" data-testid="scenario-editor">
        {picker}
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
      {picker}
      <Toolbar
        title={draft.name || '(이름 없음)'}
        meta={
          <>
            <span className="tabular-nums">
              스텝 {steps.length}개{selIdx >= 0 ? ` · ${selIdx + 1}번 선택` : ''}
            </span>
            <span className="tabular-nums">
              {draft.repeat === 0 ? '∞ 반복' : `${draft.repeat}회`}
            </span>
            {dirty ? (
              <span className="rounded bg-warn-soft px-1.5 py-0.5 text-warn-fg">변경됨</span>
            ) : null}
            {running ? (
              <span className="rounded bg-ok-soft px-1.5 py-0.5 text-ok-fg">
                실행 중 — 변경은 다음 실행부터
              </span>
            ) : null}
          </>
        }
      >
        {/* 스텝 조작은 붙여 세운다 — 한 덩어리로 읽히고 손이 자리를 기억한다. */}
        <Button size="sm" icon={<Plus size={13} />} onClick={addStep} data-testid="step-add">
          행
        </Button>
        <Button
          size="icon-sm"
          aria-label="선택한 행 복제"
          title="선택한 행 복제"
          disabled={selIdx < 0}
          onClick={dupStep}
          data-testid="step-dup"
        >
          <Copy size={13} />
        </Button>
        <Button
          size="icon-sm"
          aria-label="선택한 행 삭제"
          title="선택한 행 삭제"
          disabled={selIdx < 0}
          onClick={delStep}
          data-testid="step-delete"
        >
          <Trash2 size={13} />
        </Button>
        <Button
          size="icon-sm"
          aria-label="위로"
          title="위로"
          disabled={selIdx <= 0}
          onClick={() => move(-1)}
        >
          <ArrowUp size={13} />
        </Button>
        <Button
          size="icon-sm"
          aria-label="아래로"
          title="아래로"
          disabled={selIdx < 0 || selIdx >= steps.length - 1}
          onClick={() => move(1)}
        >
          <ArrowDown size={13} />
        </Button>
        {/* 저장은 **바뀌었을 때만** 강조 — 초록이 늘 켜져 있으면 "지금 눌러야 하는 것"이 안 보인다. */}
        <Button
          size="sm"
          intent={dirty ? 'primary' : 'neutral'}
          icon={<Save size={13} />}
          loading={saving}
          disabled={!dirty && !!draft.id}
          onClick={onSave}
          data-testid="scenario-save"
        >
          저장
        </Button>
        <OverflowMenu
          testid="scenario-more"
          title="설정 · 검증 · 파일"
          items={[
            { label: '시나리오 설정…', hint: '이름 · 반복', run: onOpenSettings },
            {
              label: '스텝 파라미터…',
              disabled: selIdx < 0 ? '먼저 행을 고르세요' : undefined,
              run: () => selIdx >= 0 && setParamsStep(steps[selIdx]),
            },
            { label: '검증', run: onValidate },
            { label: '파일' },
            { label: 'JSON/CSV 가져오기…', run: () => fileRef.current?.click() },
            {
              label: 'JSON 내보내기',
              disabled: draft.id ? undefined : '먼저 저장하세요',
              run: () => onExport('json'),
            },
            {
              label: 'CSV 내보내기',
              disabled: draft.id ? undefined : '먼저 저장하세요',
              run: () => onExport('csv'),
            },
          ]}
        />
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
