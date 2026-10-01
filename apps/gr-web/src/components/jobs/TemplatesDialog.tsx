// 수동작업 목록(옛 시퀀스 시나리오) — 지금 이 로봇의 수동작업을 이름 붙여 저장하고, 고른 목록을 대기열 끝에 불러온다.
// 옛 시나리오는 실행기가 은퇴해 여기서 목록으로 가져온다(짝 = PICK 뒤 같은 로봇의 DROP). 불러올 때 Z 는 그 시점 재고로.
import { useEffect, useState } from 'react'
import { Trash2 } from 'lucide-react'
import { getJson, postJson, del } from '../../lib/api'
import { route, type JobStep } from '../../lib/jobs/model'
import { jobs as jobStore } from '../../lib/jobs/store'
import { robots } from '../../lib/robots'
import type { TaskRequest } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Section } from '../../lib/ui/Section'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

interface JobTemplate {
  id: string
  name: string
  jobs: TaskRequest[][]
  note: string
  created_at: string
}

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))
const asSteps = (j: TaskRequest[]): JobStep[] =>
  j.map((request) => ({ request, task: null, state: null }))

export function TemplatesDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [list, setList] = useState<JobTemplate[]>([])
  const [scenarios, setScenarios] = useState<{ id: string; name: string }[]>([])
  const [name, setName] = useState('')
  const [pick, setPick] = useState('')
  const [busy, setBusy] = useState(false)
  const robot = robots.selected
  const rname = robots.current?.name ?? '로봇'

  async function reload() {
    try {
      setList(await getJson<JobTemplate[]>('/api/jobs/templates'))
      setScenarios(await getJson<{ id: string; name: string }[]>('/api/scenarios'))
    } catch (e) {
      toast.error(`목록을 읽지 못함 — ${errText(e)}`)
    }
  }
  useEffect(() => {
    if (open) void reload()
  }, [open])

  async function run(f: () => Promise<unknown>, ok: string) {
    setBusy(true)
    try {
      await f()
      toast.ok(ok)
      await reload()
      void jobStore.refresh()
    } catch (e) {
      toast.error(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const cols: Column<JobTemplate>[] = [
    { key: 'name', label: '이름', get: (t) => t.name, priority: 1 },
    { key: 'n', label: '작업', get: (t) => t.jobs.length, numeric: true, priority: 1 },
    {
      key: 'route',
      label: '처음',
      get: (t) => (t.jobs[0] ? `${route({ steps: asSteps(t.jobs[0]) }).from}` : ''),
      cell: (t) => {
        const r = t.jobs[0] ? route({ steps: asSteps(t.jobs[0]) }) : null
        return <span className="font-mono">{r ? `${r.from}${r.to ? ` → ${r.to}` : ''}` : '—'}</span>
      },
      priority: 2,
    },
    { key: 'note', label: '메모', get: (t) => t.note, priority: 3 },
  ]

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && onClose()}
      title="수동작업 목록"
      meta={<span className="text-xs text-content-muted">{rname}</span>}
      size="md"
      closeLabel="닫기"
      testid="templates"
    >
      <div className="flex flex-col gap-3 text-xs">
        <Section title="지금 수동작업을 목록으로 저장" first>
          <div className="flex items-end gap-2">
            <Input
              label="이름"
              value={name}
              onValueChange={setName}
              placeholder="예: 청라 측정 A"
              data-testid="templates-name"
            />
            <Button
              size="sm"
              intent="primary"
              disabled={!name.trim() || robot === null || busy}
              onClick={() =>
                void run(
                  () => postJson('/api/jobs/templates', { name: name.trim(), robot }),
                  `"${name.trim()}" 저장`,
                )
              }
              data-testid="templates-save"
            >
              저장
            </Button>
          </div>
        </Section>
        <Section title="저장한 목록">
          <DataTable
            rows={list}
            columns={cols}
            rowKey={(t) => t.id}
            density="compact"
            actions={(t) => (
              <span className="flex gap-1">
                <Button
                  size="sm"
                  disabled={robot === null || busy}
                  onClick={() =>
                    void run(async () => {
                      const r = await postJson<{
                        added: number[]
                        failed: { index: number; error: string }[]
                      }>(`/api/jobs/templates/${encodeURIComponent(t.id)}/load`, { robot })
                      if (r.failed.length)
                        toast.warn(
                          `${r.failed.length}건 못 넣음 — ${r.failed.map((f) => `#${f.index} ${f.error}`).join(' · ')}`,
                        )
                    }, `"${t.name}" ${t.jobs.length}건 → ${rname} 대기열`)
                  }
                  data-testid={`templates-load-${t.id}`}
                >
                  불러오기
                </Button>
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<Trash2 className="h-3.5 w-3.5" />}
                  title="지우기"
                  onClick={() =>
                    void run(
                      () => del(`/api/jobs/templates/${encodeURIComponent(t.id)}`),
                      `"${t.name}" 지움`,
                    )
                  }
                />
              </span>
            )}
            empty="저장한 목록 없음"
            emptyDense
            testid="templates-table"
          />
        </Section>
        <Section title="옛 시나리오 가져오기">
          <div className="flex items-end gap-2">
            <Select
              label="시나리오"
              value={pick}
              onValueChange={setPick}
              data-testid="templates-scenario"
            >
              <option value="">고르기</option>
              {scenarios.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </Select>
            <Button
              size="sm"
              disabled={!pick || busy}
              onClick={() =>
                void run(
                  () => postJson(`/api/jobs/templates/from-scenario/${encodeURIComponent(pick)}`),
                  '시나리오를 수동작업 목록으로 가져옴',
                )
              }
              data-testid="templates-import"
            >
              가져오기
            </Button>
          </div>
        </Section>
      </div>
    </Dialog>
  )
}
