// 시나리오 화면 — **목록 레일 + 편집기**. 실행 판넬은 편집기 위의 한 줄(`ScenarioRunner`)로 접혔다.
//
// draft(편집 중 문서)의 진실원은 이 페이지다. 다른 시나리오로 옮기거나 새로 만들 때 저장하지 않은 변경이
// 있으면 확인을 받는다. `nav.goScenario(id)`로 들어오면 그 시나리오를 연다.
//
// 새로 만들기·복제는 **설정 대화상자를 바로 연다** — 새 문서에서 제일 먼저 정하는 것이 이름이고,
// 이름 없는 문서는 저장이 막힌다. 열자마자 이름 칸에 포커스가 가고 Enter로 닫힌다.
import { useCallback, useEffect, useRef, useState } from 'react'
import { ListOrdered } from 'lucide-react'
import { api } from '../../lib/api'
import { runsFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { useRegistry } from '../../lib/registry'
import { useSse } from '../../lib/sse'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { useUnsavedGuard } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { toast } from '../../lib/ui/toast'
import type { Cell, Item, Scenario, ScenarioUpsert, Station } from '../../lib/types'
import { scenarioApi } from '../../lib/scenario/api'
import {
  RUN_ACTIVE,
  duplicateScenario,
  newScenario,
  toUpsert,
  validateScenario,
  type ValidationIssue,
} from '../../lib/scenario/model'
import { ScenarioEditor } from './ScenarioEditor'
import { ScenarioList } from './ScenarioList'
import { ScenarioRunner } from './ScenarioRunner'
import { ScenarioSettingsDialog } from './ScenarioSettingsDialog'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

export default function ScenarioPage() {
  const list = useRegistry<Scenario>(api.scenarios)
  const cells = useRegistry<Cell>(api.cells)
  const stations = useRegistry<Station>(api.stations)
  const items = useRegistry<Item>(api.items)
  const feed = useSse(runsFeed)
  const run = feed.data
  const runningId = run && RUN_ACTIVE.has(run.state) ? run.scenario_id : null

  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draft, setDraft] = useState<ScenarioUpsert | null>(null)
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const [issues, setIssues] = useState<ValidationIssue[]>([])
  const [settingsOpen, setSettingsOpen] = useState(false)
  // 저장하지 않은 변경을 버리는 이동 — 확인 뒤에 실행할 동작을 들고 있는다.
  // 되묻는 규칙 자체는 킷의 `useUnsavedGuard` 하나다(대화상자와 같은 규칙을 손으로 한 벌 더 쓰면
  // 한쪽만 고쳐진다). 여기서는 "닫기" 자리에 **들고 있던 이동**을 끼운다.
  const pending = useRef<(() => void) | null>(null)
  const runPending = useCallback(() => {
    const a = pending.current
    pending.current = null
    a?.()
  }, [])
  const unsaved = useUnsavedGuard(dirty, runPending)
  const request = unsaved.request
  const guard = useCallback(
    (action: () => void) => {
      pending.current = action
      request('cancel')
    },
    [request],
  )

  const open = useCallback((s: Scenario) => {
    setSelectedId(s.id)
    setDraft(toUpsert(s))
    setDirty(false)
    setIssues([])
  }, [])

  // `nav.goScenario(id)` 진입 — 목록이 오면 그 id를 연다.
  const wanted = useRef<string>('')
  useEffect(() => {
    wanted.current = nav.consumeScenarioId()
  }, [])
  useEffect(() => {
    if (!wanted.current || list.items.length === 0) return
    const s = list.items.find((x) => x.id === wanted.current)
    wanted.current = ''
    if (s) open(s)
  }, [list.items, open])

  // 다른 곳(가져오기·저장)에서 목록이 갈리면 선택은 유지하되, 사라진 시나리오면 비운다.
  useEffect(() => {
    if (!selectedId || list.loading) return
    if (!list.items.some((s) => s.id === selectedId)) {
      setSelectedId(null)
      setDraft(null)
      setDirty(false)
    }
  }, [list.items, list.loading, selectedId])

  const change = (next: ScenarioUpsert) => {
    setDraft(next)
    setDirty(true)
  }

  const reg = { cells: cells.items, stations: stations.items, items: items.items }

  const save = async () => {
    if (!draft) return
    const local = validateScenario(draft, reg)
    if (local.some((i) => i.step_index === null && i.field === 'name')) {
      setIssues(local)
      toast.warn('이름을 입력하세요')
      setSettingsOpen(true)
      return
    }
    setSaving(true)
    try {
      const saved = await api.scenarioSave(draft)
      open(saved)
      setIssues(local)
      await list.reload()
      toast.ok(`저장됨 — ${saved.name}`)
    } catch (e) {
      toast.error(`저장 실패 — ${errMsg(e)}`)
    } finally {
      setSaving(false)
    }
  }

  const validate = async () => {
    if (!draft) return
    const local = validateScenario(draft, reg)
    let merged = local
    try {
      const remote = await scenarioApi.validate(draft)
      const seen = new Set(local.map((i) => `${i.step_index}:${i.field}:${i.message}`))
      merged = [
        ...local,
        ...remote.filter((i) => !seen.has(`${i.step_index}:${i.field}:${i.message}`)),
      ]
    } catch (e) {
      toast.warn(`서버 검증 실패(로컬 결과만 표시) — ${errMsg(e)}`)
    }
    merged.sort((a, b) => (a.step_index ?? -1) - (b.step_index ?? -1))
    setIssues(merged)
    if (merged.length === 0) toast.ok('검증 통과 — 문제 없음')
    else toast.warn(`검증 ${merged.length}건`)
  }

  const importFile = (file: File) =>
    guard(() => {
      void (async () => {
        const tid = toast.pending(`가져오는 중 — ${file.name}`)
        try {
          const s = await scenarioApi.importFile(file)
          await list.reload()
          open(s)
          toast.resolve(tid, 'ok', `가져옴 — ${s.name} (스텝 ${s.steps.length}개)`)
        } catch (e) {
          toast.resolve(tid, 'error', `가져오기 실패 — ${errMsg(e)}`)
        }
      })()
    })

  const exportAs = (format: 'json' | 'csv') => {
    if (!draft?.id) return
    if (dirty) toast.warn('저장되지 않은 변경은 내보내기에 포함되지 않습니다')
    window.location.assign(scenarioApi.exportUrl(draft.id, format))
  }

  const remove = async (s: Scenario) => {
    try {
      await api.scenarioDelete(s.id)
      if (selectedId === s.id) {
        setSelectedId(null)
        setDraft(null)
        setDirty(false)
      }
      await list.reload()
      toast.ok(`삭제됨 — ${s.name}`)
    } catch (e) {
      toast.error(`삭제 실패 — ${errMsg(e)}`)
    }
  }

  /** 새 문서(새로 만들기·복제) — 이름부터 묻는다. */
  const startDraft = (next: ScenarioUpsert) => {
    setSelectedId(null)
    setDraft(next)
    setDirty(true)
    setIssues([])
    setSettingsOpen(true)
  }

  const selectedSaved = selectedId ? (list.items.find((s) => s.id === selectedId) ?? null) : null

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-scenario">
      {/* 실행 상태는 아래 실행 띠가 점·게이지로 말한다 — 머리띠에서 같은 말을 두 번 하지 않는다. */}
      <ScreenHeader
        title="시나리오"
        icon={<ListOrdered className="h-4 w-4" />}
        items={[{ label: '시나리오', value: String(list.items.length) }]}
      />
      <div className="flex min-h-0 flex-1">
        <div className="w-72 flex-none border-r border-line-default">
          <ScenarioList
            items={list.items}
            loading={list.loading}
            selectedId={selectedId}
            runningId={runningId}
            onSelect={(id) => {
              const s = list.items.find((x) => x.id === id)
              if (s && s.id !== selectedId) guard(() => open(s))
            }}
            onNew={() => guard(() => startDraft(newScenario()))}
            onDuplicate={(s) => guard(() => startDraft(duplicateScenario(s)))}
            onDelete={(s) => void remove(s)}
          />
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          {/* 실행은 이 화면의 주 조작이라 편집기보다 위에 선다 — 한 줄이고, 돌지 않을 때는 버튼 하나다. */}
          <ScenarioRunner
            scenario={selectedSaved}
            dirty={dirty}
            onOpenScenario={(id) => {
              const s = list.items.find((x) => x.id === id)
              if (s) guard(() => open(s))
              else toast.warn('그 시나리오는 삭제되었습니다')
            }}
          />
          <div className="min-h-0 flex-1">
            <ScenarioEditor
              draft={draft}
              dirty={dirty}
              saving={saving}
              issues={issues}
              cells={cells.items}
              stations={stations.items}
              items={items.items}
              running={!!draft?.id && draft.id === runningId}
              onChange={change}
              onSave={() => void save()}
              onValidate={() => void validate()}
              onImportFile={importFile}
              onExport={exportAs}
              onOpenSettings={() => setSettingsOpen(true)}
            />
          </div>
        </div>
      </div>

      <ScenarioSettingsDialog
        open={settingsOpen}
        draft={draft}
        onOpenChange={setSettingsOpen}
        onApply={change}
      />

      <ConfirmDialog
        open={unsaved.asking}
        onOpenChange={(o) => {
          if (o) return
          pending.current = null
          unsaved.keepEditing()
        }}
        scope="single"
        title="변경 버리기"
        danger
        confirmLabel="버리고 이동"
        onConfirm={unsaved.discard}
      >
        <div className="flex flex-col gap-2 text-xs">
          <p className="m-0">저장하지 않은 변경을 버릴까요?</p>
          <FieldList
            columns={2}
            dense
            labelWidth={56}
            items={[
              { label: '문서', value: draft?.name ?? '', missing: '이름 없음' },
              { label: '스텝', value: `${draft?.steps.length ?? 0}개` },
            ]}
          />
        </div>
      </ConfirmDialog>
    </div>
  )
}
