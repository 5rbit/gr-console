// 작업 대기열 스토어 — `/api/jobs` 를 1초마다 읽는다(화면이 하나라도 열려 있으면). 조작은 서버로 보내고 바로 다시 읽는다.
// 보내기 · 멈춤 · 실패 알림은 앱이 이 스토어를 늘 켜 두므로 어느 화면에서나 토스트로 뜬다.
import { getJson, patchJson, postJson, putJson } from '../api'
import { visibleInterval } from '../poll'
import { Store } from '../store'
import { toast } from '../ui/toast'
import {
  STAGE_LABEL,
  route,
  type DispatchRow,
  type Job,
  type JobsSnapshot,
  type NewJob,
} from './model'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

class JobsStore extends Store {
  snap: JobsSnapshot | null = null
  error: string | null = null
  #refs = 0
  #timer: ReturnType<typeof setInterval> | null = null

  start(): () => void {
    if (this.#refs++ === 0) {
      void this.refresh()
      this.#timer = visibleInterval(() => void this.refresh(), 1000)
    }
    return () => {
      if (--this.#refs === 0 && this.#timer) {
        clearInterval(this.#timer)
        this.#timer = null
      }
    }
  }

  async refresh(): Promise<void> {
    try {
      this.#apply(await getJson<JobsSnapshot>('/api/jobs?ended=60'))
      this.error = null
    } catch (e) {
      this.error = errText(e)
      this.notify()
    }
  }

  /** 새로 생긴 일의 토스트 — 실패 · 보내기 멈춤(첫 읽기는 조용히). */
  #apply(next: JobsSnapshot): void {
    const prev = this.snap
    if (prev) {
      const was = new Map([...prev.active, ...prev.ended].map((j) => [j.id, j.stage]))
      for (const j of next.ended) {
        const before = was.get(j.id)
        if (before && before !== j.stage && j.stage === 'failed') {
          const r = route(j)
          toast.error(
            `WorkId ${j.work_id ?? '-'} ${STAGE_LABEL[j.stage]} — ${r.from}${r.to ? ` → ${r.to}` : ''}`,
          )
        }
      }
      for (const d of next.dispatch) {
        const p = prev.dispatch.find((x) => x.robot === d.robot)
        if (d.paused && p?.paused !== d.paused)
          toast.warn(`${d.name} 작업 보내기 멈춤 — ${d.paused}`)
      }
    }
    this.snap = next
    this.notify()
  }

  get all(): Job[] {
    return this.snap ? [...this.snap.active, ...(this.snap.planned ?? []), ...this.snap.ended] : []
  }

  dispatch(robot: number | null): DispatchRow | null {
    return this.snap?.dispatch.find((d) => d.robot === robot) ?? null
  }

  /** 대기 순번(1부터, 없으면 null). */
  rank(job: Job): number | null {
    const d = this.dispatch(job.robot)
    const i = d ? d.order.indexOf(job.id) : -1
    return i < 0 ? null : i + 1
  }

  async add(n: NewJob): Promise<Job> {
    const j = await postJson<Job>('/api/jobs', n)
    void this.refresh()
    return j
  }

  async cancel(id: string): Promise<Job> {
    const j = await postJson<Job>(`/api/jobs/${encodeURIComponent(id)}/cancel`)
    void this.refresh()
    return j
  }

  /** 유실된 단계를 다시 보낸다(같은 WorkId · TaskId). */
  async resend(id: string): Promise<void> {
    await postJson<Job>(`/api/jobs/${encodeURIComponent(id)}/resend`)
    void this.refresh()
  }

  async front(id: string): Promise<void> {
    await patchJson<Job>(`/api/jobs/${encodeURIComponent(id)}`, { front: true })
    void this.refresh()
  }

  async setPriority(id: string, priority: number): Promise<void> {
    await patchJson<Job>(`/api/jobs/${encodeURIComponent(id)}`, { priority })
    void this.refresh()
  }

  async setEnabled(robot: number, enabled: boolean): Promise<void> {
    await putJson(`/api/jobs/dispatch/${robot}`, { enabled })
    void this.refresh()
  }

  async resume(robot: number): Promise<void> {
    await postJson(`/api/jobs/dispatch/${robot}/resume`)
    void this.refresh()
  }

  /** 지금 한 건 — 보냈으면 null, 아니면 기다리는 이유. */
  async next(robot: number): Promise<string | null> {
    const r = await postJson<{ sent?: unknown; wait?: string }>(`/api/jobs/dispatch/${robot}/next`)
    void this.refresh()
    return r.wait ?? null
  }
}

export const jobs = new JobsStore()
