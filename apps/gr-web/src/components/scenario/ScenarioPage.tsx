// 시나리오 화면 — 왼쪽 목록 레일 · 가운데 편집기 · 오른쪽 실행 판넬.
//
// draft(편집 중 문서)의 진실원은 이 페이지다. 다른 시나리오로 옮기거나 새로 만들 때 저장하지 않은 변경이
// 있으면 확인을 받는다. `nav.goScenario(id)`로 들어오면 그 시나리오를 연다.
import { useCallback, useEffect, useRef, useState } from 'react'
import { ListOrdered } from 'lucide-react'
import { api } from '../../lib/api'
import { runsFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { useRegistry } from '../../lib/registry'
import { useSse } from '../../lib/sse'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { toast } from '../../lib/ui/toast'
import type { Cell, Item, Scenario, ScenarioUpsert, Station } from '../../lib/types'
import { scenarioApi } from '../../lib/scenario/api'
import { RUN_ACTIVE, RUN_STATE_LABEL, duplicateScenario, newScenario, toUpsert, validateScenario, type ValidationIssue } from '../../lib/scenario/model'
import { ScenarioEditor } from './ScenarioEditor'
import { ScenarioList } from './ScenarioList'
import { ScenarioRunner } from './ScenarioRunner'

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
  // 저장하지 않은 변경을 버리는 이동 — 확인 뒤에 실행할 동작을 들고 있는다.
  const pending = useRef<(() => void) | null>(null)
  const [discardOpen, setDiscardOpen] = useState(false)

  const guard = useCallback(
    (action: () => void) => {
      if (!dirty) {
        action()
        return
      }
      pending.current = action
      setDiscardOpen(true)
    },
    [dirty],
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
      merged = [...local, ...remote.filter((i) => !seen.has(`${i.step_index}:${i.field}:${i.message}`))]
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

  const selectedSaved = selectedId ? (list.items.find((s) => s.id === selectedId) ?? null) : null

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-scenario">
      <ScreenHeader
        title="시나리오"
        icon={<ListOrdered className="h-4 w-4" />}
        items={[
          { label: '시나리오', value: String(list.items.length) },
          { label: '실행', value: run ? `${RUN_STATE_LABEL[run.state]}${runningId ? ` · ${run.scenario_name}` : ''}` : feed.connected ? '대기' : '연결 중' },
        ]}
      />
      <div className="flex min-h-0 flex-1">
        <div className="w-80 flex-none border-r border-line-default">
          <ScenarioList
            items={list.items}
            loading={list.loading}
            selectedId={selectedId}
            runningId={runningId}
            onSelect={(id) => {
              const s = list.items.find((x) => x.id === id)
              if (s && s.id !== selectedId) guard(() => open(s))
            }}
            onNew={() =>
              guard(() => {
                setSelectedId(null)
                setDraft(newScenario())
                setDirty(true)
                setIssues([])
              })
            }
            onDuplicate={(s) =>
              guard(() => {
                setSelectedId(null)
                setDraft(duplicateScenario(s))
                setDirty(true)
                setIssues([])
              })
            }
            onDelete={(s) => void remove(s)}
          />
        </div>
        <div className="min-w-0 flex-1">
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
          />
        </div>
        <ScenarioRunner
          scenario={selectedSaved}
          dirty={dirty}
          onOpenScenario={(id) => {
            const s = list.items.find((x) => x.id === id)
            if (s) guard(() => open(s))
            else toast.warn('그 시나리오는 삭제되었습니다')
          }}
        />
      </div>

      <ConfirmDialog
        open={discardOpen}
        onOpenChange={(o) => {
          setDiscardOpen(o)
          if (!o) pending.current = null
        }}
        scope="single"
        title="변경 버리기"
        danger
        confirmLabel="버리고 이동"
        onConfirm={() => {
          const a = pending.current
          pending.current = null
          a?.()
        }}
      >
        <p className="m-0">저장하지 않은 변경이 있습니다. 버리고 계속할까요?</p>
      </ConfirmDialog>
    </div>
  )
}
