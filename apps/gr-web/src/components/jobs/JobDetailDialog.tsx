// 작업 상세 — 표의 어느 묶음에서 눌러도 같은 창. 단계 이력 · 지금 이유 · Task · 화물 · 조작(맨 앞으로 · 우선 · 취소).
// 창이 열려 있는 동안 그 행은 표에서 제자리에 남는다(닫으면 제 묶음으로).
import { useState } from 'react'
import { jobs as jobStore } from '../../lib/jobs/store'
import {
  STAGE_LABEL,
  cargoOf,
  groupOf,
  isPlanned,
  route,
  taskLabel,
  whoOf,
  type Job,
} from '../../lib/jobs/model'
import { robots } from '../../lib/robots'
import type { Item } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Dialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { InfoRows } from '../../lib/ui/Pair'
import { Section } from '../../lib/ui/Section'
import { toast } from '../../lib/ui/toast'
import { JobStageBadge } from './JobStageBadge'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))
const hms = (s: string | null | undefined) => (s ? s.replace('T', ' ').slice(5, 19) : '')

export function JobDetailDialog({
  job,
  items,
  onClose,
  heldElsewhere,
}: {
  job: Job | null
  items: readonly Item[]
  onClose: () => void
  /** 단계는 바뀌었는데 표에서 아직 제자리에 둔 행인가. */
  heldElsewhere: boolean
}) {
  const [ask, setAsk] = useState(false)
  const [prio, setPrio] = useState<string | null>(null)
  if (!job) return null
  const r = route(job)
  const c = cargoOf(job)
  const it = c.code ? items.find((i) => i.code === c.code) : undefined
  const planned = isPlanned(job)
  const editable = job.stage === 'wait'
  const name = robots.list.find((x) => x.id === job.robot)?.name ?? '미정'
  const title = `${r.from}${r.to ? ` → ${r.to}` : ''}`

  async function run(f: () => Promise<unknown>, ok: string) {
    try {
      await f()
      toast.ok(ok)
    } catch (e) {
      toast.error(errText(e))
    }
  }

  return (
    <>
      <Dialog
        open
        onOpenChange={(o) => {
          if (!o) onClose()
        }}
        title={title}
        meta={<JobStageBadge stage={job.stage} />}
        size="md"
        testid="job-detail"
        footer={
          <>
            <Button
              size="sm"
              intent="danger"
              onClick={() => setAsk(true)}
              disabled={
                planned ||
                job.stage === 'done' ||
                job.stage === 'failed' ||
                job.stage === 'canceled'
              }
              title={
                planned
                  ? '규칙이 만들 예정인 작업 — 끄려면 규칙 탭에서 그 규칙을 끄세요'
                  : groupOf(job.stage) === 'ended'
                    ? '이미 끝난 작업입니다'
                    : undefined
              }
              data-testid="job-cancel"
            >
              취소…
            </Button>
            <span className="flex-1" />
            {job.steps.some((s) => s.state === 'lost') ? (
              <Button
                size="sm"
                onClick={() =>
                  void run(
                    () => jobStore.resend(job.id),
                    `WorkId ${job.work_id ?? '-'} 유실 단계 다시 보냄`,
                  )
                }
                title="PLC 에 그 Task 가 정말 없는지 확인한 뒤 누르세요 — 같은 WorkId · TaskId 로 다시 나갑니다"
                data-testid="job-resend"
              >
                다시 보내기
              </Button>
            ) : null}
            {editable ? (
              <>
                <Button
                  size="sm"
                  onClick={() => setPrio(String(job.priority))}
                  data-testid="job-prio"
                >
                  우선 바꾸기…
                </Button>
                <Button
                  size="sm"
                  onClick={() =>
                    void run(() => jobStore.front(job.id), `WorkId ${job.work_id ?? '-'} 맨 앞으로`)
                  }
                  data-testid="job-front"
                >
                  맨 앞으로
                </Button>
              </>
            ) : null}
            <Button size="sm" intent="primary" onClick={onClose}>
              닫기
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3 text-xs">
          <InfoRows
            rows={[
              {
                label: 'WorkId',
                value: job.work_id ? String(job.work_id) : '대기열에 들어갈 때 붙음',
              },
              { label: 'Task', value: taskLabel(job) || '-' },
              { label: '이송 지시', value: job.transfer_order_id ?? '-' },
              { label: '로봇 · 우선', value: `${name} · ${job.priority}` },
              { label: '주체', value: `${whoOf(job)} · ${hms(job.created_at)}` },
              { label: '화물', value: c.code ? `${c.code} ${it?.name ?? ''} × ${c.count}` : '-' },
              { label: '바코드', value: it?.spec?.barcode || '-' },
            ]}
          />
          {job.wait_reason || job.error || heldElsewhere ? (
            <Section title="지금">
              <div className="flex flex-col gap-1">
                {job.error ? <span className="text-fault-fg">{job.error}</span> : null}
                {job.wait_reason && !job.error ? <span>{job.wait_reason}</span> : null}
                {heldElsewhere ? (
                  <span className="text-content-muted">
                    표에서 이 행은 보는 동안 제자리에 있습니다 — 닫으면 {STAGE_LABEL[job.stage]}{' '}
                    묶음으로 갑니다
                  </span>
                ) : null}
              </div>
            </Section>
          ) : null}
          <Section title="Task">
            <InfoRows
              rows={job.steps.map((s, i) => ({
                label: `-${i + 1} ${s.request.type}`,
                value: s.task
                  ? `${s.state ?? '-'} · ${job.work_id ?? ''}-${i + 1}`
                  : '아직 안 나감',
              }))}
            />
          </Section>
          <Section title="단계 이력">
            <InfoRows
              alignRight={false}
              rows={job.history.map((h, i) => ({
                label: hms(h.at) || String(i),
                value: `${STAGE_LABEL[h.stage]}${h.note && h.note !== STAGE_LABEL[h.stage] ? ` · ${h.note}` : ''}`,
              }))}
            />
          </Section>
        </div>
      </Dialog>
      {prio !== null ? (
        <Dialog
          open
          onOpenChange={(o) => {
            if (!o) setPrio(null)
          }}
          title={`우선 — WorkId ${job.work_id ?? '-'}`}
          size="sm"
          footer={
            <>
              <span className="flex-1" />
              <Button size="sm" onClick={() => setPrio(null)}>
                취소
              </Button>
              <Button
                size="sm"
                intent="primary"
                onClick={() => {
                  const n = Number(prio)
                  setPrio(null)
                  void run(() => jobStore.setPriority(job.id, n), `우선 ${n}`)
                }}
              >
                저장
              </Button>
            </>
          }
        >
          <Input
            type="number"
            mono
            value={prio}
            onValueChange={setPrio}
            min={0}
            max={999}
            data-testid="job-prio-input"
          />
        </Dialog>
      ) : null}
      <ConfirmDialog
        open={ask}
        onOpenChange={setAsk}
        scope="single-robot"
        danger
        title={`작업 취소 — WorkId ${job.work_id ?? '-'}`}
        confirmLabel="작업 취소"
        onConfirm={() =>
          void run(() => jobStore.cancel(job.id), `WorkId ${job.work_id ?? '-'} 취소`)
        }
      >
        <InfoRows
          rows={[
            { label: '경로', value: title },
            { label: '단계', value: STAGE_LABEL[job.stage] },
            { label: '로봇', value: name },
          ]}
        />
      </ConfirmDialog>
    </>
  )
}
