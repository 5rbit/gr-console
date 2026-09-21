// 시나리오 순수 모델 — 스텝 생성·이동·복제와 로컬 검증. DOM·fetch 없음(vitest 대상).
//
// 백엔드 `scenario/io.rs::validate_shape` 와 같은 규칙을 화면에서 먼저 돌린다(저장 전 붉은 칸).
// 레지스트리 조회(없는 셀·스테이션·품목)는 넘겨받은 목록으로 판정한다.
import type {
  Cell,
  Item,
  Scenario,
  ScenarioRun,
  ScenarioStep,
  ScenarioUpsert,
  Station,
  StepResult,
  TaskParams,
  TaskState,
  TaskType,
} from '../types'
import { PARAM_LABELS, TASK_TYPES } from '../gr/const'
import { pairIssuesOf } from '../task/plan'

export interface ValidationIssue {
  /** `null` = 시나리오 수준(이름·스텝 없음). */
  step_index: number | null
  field: string
  message: string
}

/** 백엔드가 더 주는 필드(`note`·스텝 `error`/`attempt`) — 공유 타입에는 없어 슬라이스 안에서만 넓힌다. */
export type StepResultView = StepResult & { error?: string | null; attempt?: number }
export type ScenarioRunView = Omit<ScenarioRun, 'results'> & {
  note?: string | null
  results: StepResultView[]
}

export const RUN_ACTIVE = new Set<ScenarioRun['state']>(['running', 'paused', 'stopping'])
export const RUN_STATE_LABEL: Record<ScenarioRun['state'], string> = {
  idle: '대기',
  running: '실행 중',
  paused: '일시정지',
  stopping: '정지 중',
  stopped: '정지됨',
  done: '완료',
  failed: '실패',
}

export const WAIT_FOR_LABEL: Record<ScenarioStep['wait_for'], string> = {
  accepted: '수락',
  completed: '완료',
}
export const ON_FAILURE_LABEL: Record<ScenarioStep['on_failure'], string> = {
  stop: '정지',
  skip: '건너뜀',
  retry: '재시도',
}
export const TARGET_KIND_LABEL = { cell: 'Cell', station: 'Station' } as const

/** 품목이 필수인 작업 종류. */
export const NEEDS_ITEM: ReadonlySet<TaskType> = new Set<TaskType>(['PICK', 'DROP', 'MEASURE'])
/** 대상이 필수인 작업 종류. */
export const NEEDS_TARGET: ReadonlySet<TaskType> = new Set<TaskType>(['PICK', 'DROP', 'MOVE'])

const PARAM_KEYS = new Set(Object.keys(PARAM_LABELS))
const BOOL_PARAM_KEYS = new Set<keyof TaskParams>([
  'lift_up_after_complete',
  'lift_up_partial',
  'measure_floor',
  'measure_item',
  'measure_sku',
  'adjust_center',
  'find_station_item',
  'avoid',
  'outbound',
  'use_drag_out',
  'use_drag_in',
])

let seq = 0
/** 스텝 id — `crypto.randomUUID`가 있으면 그것, 없으면(구형 환경) 시각+일련. */
export function stepId(): string {
  const c = globalThis.crypto as { randomUUID?: () => string } | undefined
  if (c?.randomUUID) return c.randomUUID()
  return `s-${Date.now().toString(36)}-${(++seq).toString(36)}`
}

export function newStep(partial: Partial<ScenarioStep> = {}): ScenarioStep {
  return {
    id: stepId(),
    label: '',
    type: 'PICK',
    target: null,
    item_code: null,
    count: 1,
    params: {},
    wait_for: 'completed',
    wait_after_ms: 0,
    on_failure: 'stop',
    note: '',
    ...partial,
  }
}

export function newScenario(): ScenarioUpsert {
  return { name: '새 시나리오', description: '', steps: [], repeat: 1 }
}

/** 스텝 복제 — 새 id, 나머지는 깊은 복사. */
export function cloneStep(s: ScenarioStep): ScenarioStep {
  return { ...s, id: stepId(), target: s.target ? { ...s.target } : null, params: { ...s.params } }
}

/** 시나리오 복제(저장 전 문서) — id 없음, 이름에 "(복사)", 스텝 id 재발급. */
export function duplicateScenario(s: Scenario | ScenarioUpsert): ScenarioUpsert {
  return {
    name: `${s.name} (복사)`,
    description: s.description,
    repeat: s.repeat,
    steps: s.steps.map(cloneStep),
  }
}

/** 저장된 시나리오 → 편집 문서. */
export function toUpsert(s: Scenario): ScenarioUpsert {
  return {
    id: s.id,
    name: s.name,
    description: s.description,
    repeat: s.repeat,
    steps: s.steps.map((st) => ({
      ...st,
      target: st.target ? { ...st.target } : null,
      params: { ...st.params },
    })),
  }
}

/** 스텝을 위(-1)/아래(+1)로 옮긴 새 배열. 끝이면 같은 배열을 돌려준다(변화 없음). */
export function moveStep(steps: ScenarioStep[], index: number, dir: -1 | 1): ScenarioStep[] {
  const j = index + dir
  if (index < 0 || index >= steps.length || j < 0 || j >= steps.length) return steps
  const next = steps.slice()
  const [it] = next.splice(index, 1)
  next.splice(j, 0, it)
  return next
}

/** 덮어쓰는 파라미터 수. */
export function paramCount(step: ScenarioStep): number {
  return Object.keys(step.params).length
}

/** 한 줄 요약 — "PICK Cell#101 ×2 · 1001". */
export function stepSummary(step: ScenarioStep): string {
  const parts: string[] = [step.type]
  if (step.target)
    parts.push(`${TARGET_KIND_LABEL[step.target.kind] ?? step.target.kind}#${step.target.id}`)
  if (step.count > 1) parts.push(`×${step.count}`)
  if (step.item_code !== null) parts.push(`· ${step.item_code}`)
  return parts.join(' ')
}

export interface RegistryLists {
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
}

/** 로컬 검증 — 백엔드 `validate`와 같은 규칙. 레지스트리 목록이 비어 있으면(아직 안 받음) 참조 검사는 건너뛴다. */
export function validateScenario(
  s: ScenarioUpsert,
  reg?: Partial<RegistryLists>,
): ValidationIssue[] {
  const out: ValidationIssue[] = []
  if (!s.name.trim()) out.push({ step_index: null, field: 'name', message: '이름이 비어 있음' })
  if (s.steps.length === 0) out.push({ step_index: null, field: 'steps', message: '스텝이 없음' })
  const cells = reg?.cells?.length ? new Set(reg.cells.map((c) => c.id)) : null
  const stations = reg?.stations?.length ? new Set(reg.stations.map((c) => c.id)) : null
  const items = reg?.items?.length ? new Set(reg.items.map((i) => i.code)) : null
  s.steps.forEach((st, i) => {
    const push = (field: string, message: string) => out.push({ step_index: i, field, message })
    if (!TASK_TYPES.includes(st.type)) push('type', `알 수 없는 작업 종류 '${st.type}'`)
    if (!Number.isInteger(st.count) || st.count < 1) push('count', '수량은 1 이상')
    else if (st.count > 255) push('count', '수량은 255 이하')
    if (NEEDS_ITEM.has(st.type) && st.item_code === null)
      push('item_code', `${st.type}에는 품목이 필요`)
    if (st.item_code !== null && items && !items.has(st.item_code))
      push('item_code', `품목 ${st.item_code} 이(가) 레지스트리에 없음`)
    if (!st.target) {
      if (NEEDS_TARGET.has(st.type)) push('target', `${st.type}에는 대상이 필요`)
    } else {
      if (st.target.kind !== 'cell' && st.target.kind !== 'station')
        push('target', `알 수 없는 대상 종류 '${st.target.kind}'`)
      else if (!Number.isInteger(st.target.id) || st.target.id <= 0) push('target', '대상 id가 0')
      else if (st.target.kind === 'cell' && cells && !cells.has(st.target.id))
        push('target', `셀 ${st.target.id} 이(가) 레지스트리에 없음`)
      else if (st.target.kind === 'station' && stations && !stations.has(st.target.id))
        push('target', `스테이션 ${st.target.id} 이(가) 레지스트리에 없음`)
    }
    if (!Number.isInteger(st.wait_after_ms) || st.wait_after_ms < 0)
      push('wait_after_ms', '대기 시간은 0 이상')
    for (const [k, v] of Object.entries(st.params)) {
      if (!PARAM_KEYS.has(k)) push('params', `알 수 없는 파라미터 '${k}'`)
      else if (
        BOOL_PARAM_KEYS.has(k as keyof TaskParams) ? typeof v !== 'boolean' : typeof v !== 'number'
      )
        push('params', `'${k}' 값 형식 오류`)
    }
  })
  // PICK/DROP 짝(백엔드 `validate_pairs`) — 같은 로봇의 다음 스텝이 짝 DROP. 다른 로봇 스텝은 사이에 와도 된다.
  for (const p of pairIssuesOf(s.steps))
    out.push({ step_index: p.index, field: 'type', message: p.message })
  return out
}

/** 실행 화면의 스텝 상태 — 아직 안 보낸 스텝은 **예정**. */
export type StepPhase = '예정' | '제출됨' | '실행 중' | '완료' | '실패' | '삭제'

export function phaseOf(state: TaskState | null | undefined): StepPhase {
  switch (state) {
    case 'submitted':
    case 'accepted':
    case 'queued':
      return '제출됨'
    case 'running':
      return '실행 중'
    case 'completed':
      return '완료'
    case 'rejected':
    case 'failed':
    case 'canceled':
    case 'lost':
      return '실패'
    default:
      return '예정'
  }
}

/**
 * 이번 회차 스텝별 상태 — 각 스텝의 마지막 결과가 가리키는 Task 의 **지금** 상태(`live`, 없으면 결과 때 상태).
 * 결과가 없으면 예정, Task 없이 끝난 결과(작성·제출 실패)는 실패.
 */
export function stepPhases(
  stepCount: number,
  iteration: number,
  results: readonly StepResultView[],
  live: (taskId: string) => TaskState | null | undefined,
  skipped: readonly [number, number][] = [],
): StepPhase[] {
  const out: StepPhase[] = Array.from({ length: stepCount }, () => '예정')
  for (const r of results) {
    if (r.iteration !== iteration || r.step_index < 0 || r.step_index >= stepCount) continue
    out[r.step_index] = r.task_id ? phaseOf(live(r.task_id) ?? r.state) : '실패'
  }
  for (const [it, i] of skipped) if (it === iteration && i >= 0 && i < stepCount) out[i] = '삭제'
  return out
}

/**
 * 예정 스텝 지우기(백엔드 `runner::skip_set` 과 같은 규칙) — 짝까지 묶은 스텝 목록, 또는 지울 수 없는 사유.
 * 보낸 스텝(지난 스텝 · 지금 보낸 스텝 · 결과에 Task 가 있는 스텝)은 Task 취소로. PICK 을 지우면 같은 로봇의 짝 DROP 도,
 * 아직 안 나간 PICK 의 DROP 을 지우면 그 PICK 도.
 */
export function skipSet(
  steps: readonly { type: TaskType; robot?: number | null }[],
  runRobot: number | null,
  cur: { iteration: number; step_index: number; sent: boolean },
  results: readonly StepResultView[],
  idx: number,
): { steps: number[] } | { error: string } {
  const robotOf = (i: number) => steps[i].robot ?? runRobot
  const sent = (i: number) =>
    i < cur.step_index ||
    (i === cur.step_index && cur.sent) ||
    results.some((r) => r.iteration === cur.iteration && r.step_index === i && !!r.task_id)
  if (idx < 0 || idx >= steps.length) return { error: `스텝 ${idx + 1} 없음` }
  if (sent(idx)) return { error: `스텝 ${idx + 1} 은 이미 로봇에 보냄 — Task 취소로 지우세요` }
  const out = [idx]
  if (steps[idx].type === 'PICK') {
    const j = steps.findIndex((_, k) => k > idx && robotOf(k) === robotOf(idx))
    if (j >= 0 && steps[j].type === 'DROP') out.push(j)
  } else if (steps[idx].type === 'DROP') {
    let p = idx - 1
    while (p >= 0 && robotOf(p) !== robotOf(idx)) p--
    if (p >= 0 && steps[p].type === 'PICK') {
      if (sent(p))
        return {
          error: `짝 PICK(스텝 ${p + 1}) 이 이미 나감 — DROP 만 지우면 타이어가 그리퍼에 남습니다. Task 취소로 처리하세요`,
        }
      out.push(p)
    }
  }
  return { steps: out.sort((a, b) => a - b) }
}

/** "완료 3 · 실행 중 1 · 예정 4" — 0 인 것은 뺀다. */
export function phaseSummary(phases: readonly StepPhase[]): string {
  const order: StepPhase[] = ['완료', '실행 중', '제출됨', '예정', '실패']
  return order
    .map((p) => [p, phases.filter((x) => x === p).length] as const)
    .filter(([, n]) => n > 0)
    .map(([p, n]) => `${p} ${n}`)
    .join(' · ')
}

/** 검증 결과를 (스텝, 필드) → 메시지로 — 그리드가 칸을 붉게 칠할 때 쓴다. */
export function issueMap(issues: readonly ValidationIssue[]): Map<string, string> {
  const m = new Map<string, string>()
  for (const it of issues) {
    const key = `${it.step_index ?? '-'}:${it.field}`
    const prev = m.get(key)
    m.set(key, prev ? `${prev}; ${it.message}` : it.message)
  }
  return m
}
