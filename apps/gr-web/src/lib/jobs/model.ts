// 작업 대기열 — 화면 규칙(순수, 테스트 대상). 서버 정본은 `/api/jobs`(백엔드 `jobs/`).
//
// - 표기: 셀 `C101`, 스테이션 `S2102`(구역 이름과 헷갈리지 않게 구역은 "구역 2").
// - 세 묶음(시간순): 대기 · 예정 → 할당 · 실행 → 완료 · 실패 · 취소. 묶음 안 순서는 **고정**(WorkId 순, 번호 전은 생긴 순)
//   — 우선이 바뀌어도 행은 그대로이고 `순번` 만 바뀐다(설계 7판 ②).
// - 단계 색: 예정 neutral · 대기 warn · 할당 pending · 실행 info · 완료 ok · 실패 fault · 취소 neutral.
import type { PlanStep } from '../task/plan'
import type { Target, TaskRequest, TaskState } from '../types'

export type JobStage = 'pre' | 'wait' | 'assigned' | 'running' | 'done' | 'failed' | 'canceled'

export interface JobStep {
  request: TaskRequest
  task: string | null
  state: TaskState | null
}

export interface Job {
  id: string
  seq: number
  robot: number | null
  stage: JobStage
  origin: 'manual' | 'auto' | 'external'
  via: string | null
  priority: number
  work_id: number | null
  transfer_order_id: string | null
  steps: JobStep[]
  wait_reason: string | null
  error: string | null
  note: string
  created_at: string
  updated_at: string
  ended_at: string | null
  history: { at: string; stage: JobStage; note: string }[]
}

export interface DispatchRow {
  robot: number
  name: string
  enabled: boolean
  paused: string | null
  /** 대기 순서(작업 id) — 화면의 순번. */
  order: string[]
}

export interface JobsSnapshot {
  active: Job[]
  ended: Job[]
  dispatch: DispatchRow[]
  /** 예정 — 규칙이 만들 후보(아직 작업 아님, WorkId 없음, id = `pre:<규칙>:<로봇>`). */
  planned?: Job[]
}

/** 예정 행(아직 대기열에 없는 규칙 후보)인가 — 조작(맨 앞 · 우선 · 취소)이 없다. */
export function isPlanned(j: Pick<Job, 'id'>): boolean {
  return j.id.startsWith('pre:')
}

export interface NewJob {
  robot: number | null
  steps: TaskRequest[]
  priority?: number
  note?: string
}

export const STAGE_LABEL: Record<JobStage, string> = {
  pre: '예정',
  wait: '대기',
  assigned: '할당',
  running: '실행',
  done: '완료',
  failed: '실패',
  canceled: '취소',
}

/** 상태 토큰(StatusBadge) — 할당은 예정색(pending)이라 따로 둔다. */
export const STAGE_TONE: Record<
  JobStage,
  'neutral' | 'warn' | 'pending' | 'info' | 'ok' | 'fault'
> = {
  pre: 'neutral',
  wait: 'warn',
  assigned: 'pending',
  running: 'info',
  done: 'ok',
  failed: 'fault',
  canceled: 'neutral',
}

export type JobGroup = 'queue' | 'live' | 'ended'

export const GROUPS: readonly { id: JobGroup; label: string; stages: readonly JobStage[] }[] = [
  { id: 'queue', label: '대기 · 예정', stages: ['wait', 'pre'] },
  { id: 'live', label: '할당 · 실행', stages: ['assigned', 'running'] },
  { id: 'ended', label: '완료 · 실패 · 취소', stages: ['done', 'failed', 'canceled'] },
]

export function groupOf(stage: JobStage): JobGroup {
  return GROUPS.find((g) => g.stages.includes(stage))?.id ?? 'ended'
}

/** `C101` · `S2102` — 화면 어디서나 이 모양. */
export function targetCode(t: Target | null | undefined): string {
  if (!t) return '—'
  return `${t.kind === 'station' ? 'S' : 'C'}${t.id}`
}

/** 출발 → 도착(짝) 또는 대상 하나. */
export function route(job: Pick<Job, 'steps'>): { from: string; to: string } {
  const [a, b] = job.steps
  if (b) return { from: targetCode(a?.request.target), to: targetCode(b.request.target) }
  const t = a?.request.type
  return t === 'DROP'
    ? { from: '손', to: targetCode(a?.request.target) }
    : { from: targetCode(a?.request.target), to: '' }
}

/** 작업 종류 한 단어. */
export function kindOf(job: Pick<Job, 'steps'>): string {
  if (job.steps.length === 2) return '이송'
  const t = job.steps[0]?.request.type
  if (t === 'MEASURE') return '측정'
  if (t === 'MOVE') return '이동'
  if (t === 'DROP') return '내려놓기'
  return t ?? '-'
}

/** 지금 단계의 Task 표시(`-1 PICK` · `-2 DROP` · 끝나면 `-1 · -2`). */
export function taskLabel(job: Job): string {
  if (job.work_id === null) return ''
  const sent = job.steps.map((s, i) => ({ i: i + 1, s }))
  const live = sent.find(
    (x) =>
      x.s.task && x.s.state && !['completed', 'canceled', 'rejected', 'failed'].includes(x.s.state),
  )
  if (live) return `-${live.i} ${live.s.request.type}`
  if (groupOf(job.stage) === 'ended') return sent.map((x) => `-${x.i}`).join(' · ')
  const next = sent.find((x) => !x.s.task)
  return next ? `-${next.i} ${next.s.request.type} 전` : ''
}

/** 화물(품목 코드 · 개수) — 첫 단계 요청. */
export function cargoOf(job: Pick<Job, 'steps'>): { code: number | null; count: number } {
  const r = job.steps[0]?.request
  return { code: r?.item_code ?? null, count: r?.count ?? 0 }
}

/** 할당 주체 한 줄 — `수동` · `자동 · 규칙 R2`. */
export function whoOf(job: Pick<Job, 'origin' | 'via'>): string {
  if (job.origin === 'manual') return '수동'
  if (job.origin === 'external') return '외부'
  const v = job.via ?? ''
  if (v.startsWith('rule:')) return `자동 · 규칙 ${v.slice(5)}`
  if (v.startsWith('gen:')) return `자동 · 규칙 ${v.slice(4)}`
  if (v.startsWith('req:')) return `자동 · 요청 ${v.slice(4).split('@')[0]}`
  return v ? `자동 · ${v}` : '자동'
}

/**
 * 묶음 안의 **고정** 순서: WorkId 오름차순(번호가 있는 것 먼저), 번호 전은 생긴 순(seq).
 * 대기열의 "나갈 순서"는 `order`(서버)가 순번으로 따로 말한다.
 */
export function stableSort(a: Job, b: Job): number {
  if ((a.work_id === null) !== (b.work_id === null)) return a.work_id === null ? 1 : -1
  if (a.work_id !== null && b.work_id !== null && a.work_id !== b.work_id)
    return a.work_id - b.work_id
  return a.seq - b.seq
}

/**
 * 표에 놓을 자리. `held` = 사람이 눌러서 보고 있는 행 — 그 행만 **지금 놓인 묶음**(`placed`)에 남는다
 * (단계는 바뀌어도). 나머지는 단계대로.
 */
export function placeJobs(
  jobs: readonly Job[],
  held: string | null,
  placed: ReadonlyMap<string, JobGroup>,
): Record<JobGroup, Job[]> {
  const out: Record<JobGroup, Job[]> = { queue: [], live: [], ended: [] }
  for (const j of jobs) {
    const g = j.id === held && placed.has(j.id) ? placed.get(j.id)! : groupOf(j.stage)
    out[g].push(j)
  }
  for (const g of Object.keys(out) as JobGroup[]) out[g].sort(stableSort)
  return out
}

/**
 * 계획 스텝(맵 클릭 · 측정 경로 · 팔렛) → 대기열에 넣을 작업. PICK 은 같은 로봇의 다음 DROP 과 짝으로,
 * 짝이 아직 없는 PICK 은 `rest`(작성 중)로 남긴다. 그 밖의 스텝은 한 건 작업.
 */
export function stepsToJobs(
  steps: readonly PlanStep[],
  runRobot: number | null,
  toReq: (s: PlanStep, multiPick: boolean) => TaskRequest,
): { jobs: { new: NewJob; ids: string[] }[]; rest: PlanStep[] } {
  const jobs: { new: NewJob; ids: string[] }[] = []
  const used = new Set<string>()
  const rest: PlanStep[] = []
  const robotOf = (s: PlanStep) => s.robot ?? runRobot
  for (let i = 0; i < steps.length; i++) {
    const s = steps[i]
    if (used.has(s.id)) continue
    if (s.type === 'PICK') {
      const j = steps.findIndex((d, k) => k > i && !used.has(d.id) && robotOf(d) === robotOf(s))
      const d = j >= 0 ? steps[j] : undefined
      if (!d || d.type !== 'DROP') {
        rest.push(s)
        continue
      }
      used.add(s.id)
      used.add(d.id)
      jobs.push({
        new: {
          robot: robotOf(s),
          steps: [toReq(s, false), toReq(d, false)],
          note: s.note || d.note,
        },
        ids: [s.id, d.id],
      })
      continue
    }
    used.add(s.id)
    jobs.push({ new: { robot: robotOf(s), steps: [toReq(s, false)], note: s.note }, ids: [s.id] })
  }
  return { jobs, rest }
}
