// 시나리오 슬라이스 전용 API — 공유 `api.ts`에 없는 엔드포인트(실행 옵션·검증·이력·CSV 내보내기).
// 공유 fetch 래퍼 위에 얹는다(오류 봉투·share 무효화 규칙이 같다).
import { getJson, postForm, postJson } from '../api'
import type { Scenario, ScenarioRun, ScenarioUpsert } from '../types'
import type { ValidationIssue } from './model'

export interface RunOptions {
  /** 시나리오의 repeat를 덮어쓴다(0 = 무한). */
  repeat?: number
  /** 첫 회차를 시작할 스텝(0-based). */
  start_step?: number
}

export const scenarioApi = {
  run: (id: string, opts: RunOptions = {}) =>
    postJson<ScenarioRun>(`/api/scenarios/${id}/run`, opts),
  pause: () => postJson<ScenarioRun>('/api/scenarios/run/pause'),
  resume: () => postJson<ScenarioRun>('/api/scenarios/run/resume'),
  stop: () => postJson<ScenarioRun>('/api/scenarios/run/stop'),
  runNow: () => getJson<ScenarioRun>('/api/scenarios/run'),
  runs: (limit = 50) => getJson<ScenarioRun[]>(`/api/scenarios/runs?limit=${limit}`),
  /** 저장본(`draft` 없음) 또는 편집 중 문서(`draft`)를 검증한다. 미저장 문서는 id 자리에 `_`. */
  validate: (draft: ScenarioUpsert) =>
    postJson<ValidationIssue[]>(`/api/scenarios/${draft.id ?? '_'}/validate`, draft),
  importFile: (file: File, format?: 'json' | 'csv') => {
    const fd = new FormData()
    fd.append('file', file, file.name)
    return postForm<Scenario>(`/api/scenarios/import${format ? `?format=${format}` : ''}`, fd)
  },
  exportUrl: (id: string, format: 'json' | 'csv'): string =>
    `/api/scenarios/${id}/export?format=${format}`,
}
