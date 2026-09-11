// 측정 로그 항목 → 표 행 평탄화(GrWeb `flatten/filt/summary/codesOf` 이식). 순수 함수 — 테스트 대상.
import { KIND, STATUS } from '../gr/const'
import type { ByCode, MeasLogEntry, PlcTask } from '../types'
import { dtl, f1, flagStr } from './format'

export interface MeasRow {
  seq: number
  time: string
  kind: number
  kindName: string
  status: number
  statusName: string
  workId: number
  taskId: number
  taskType: number
  cellId: number
  code: number
  cmdCount: number
  cmdId: number
  cmdOd: number
  cmdHeight: number
  cmdX: number
  cmdY: number
  cmdZ: number
  cmdG: number
  cellZ: number
  /** 명령 Z − 셀 바닥 Z. */
  cmdZRel: number
  flags: string
  cmd: PlcTask
  data: number[]
  dInnerDia: number
  dHeight: number
  dZ: number
  dOffset: number
  dCount: number
}

/** 백엔드가 준 순서(Seq 내림차순)를 유지한다. */
export function flatten(entries: readonly MeasLogEntry[]): MeasRow[] {
  const out: MeasRow[] = []
  for (const e of entries) {
    if (!e) continue
    const cmd = e.Cmd ?? ({} as PlcTask)
    const item = cmd.Item ?? ({} as PlcTask['Item'])
    const pos = cmd.Position ?? []
    const cell = cmd.Cell ?? ({} as PlcTask['Cell'])
    const cp = cell.Position ?? []
    const d = e.Delta ?? { InnerDia: 0, Height: 0, Z: 0, Offset: 0, Count: 0 }
    const z = pos[2] ?? 0
    const cellZ = cp[2] ?? 0
    out.push({
      seq: e.Seq,
      time: dtl(e.TimeStamp),
      kind: e.Kind,
      kindName: KIND[e.Kind] ?? String(e.Kind),
      status: e.Status,
      statusName: STATUS[e.Status] ?? String(e.Status),
      workId: cmd.WorkId ?? 0,
      taskId: cmd.TaskId ?? 0,
      taskType: cmd.TaskType ?? 0,
      cellId: cell.Id ?? 0,
      code: item.Code ?? 0,
      cmdCount: item.Count ?? 0,
      cmdId: item.InnerDiameter ?? 0,
      cmdOd: item.OuterDiameter ?? 0,
      cmdHeight: item.Height ?? 0,
      cmdX: pos[0] ?? 0,
      cmdY: pos[1] ?? 0,
      cmdZ: z,
      cmdG: pos[3] ?? 0,
      cellZ,
      cmdZRel: z - cellZ,
      flags: flagStr(cmd),
      cmd,
      data: e.Data ?? [],
      dInnerDia: d.InnerDia ?? 0,
      dHeight: d.Height ?? 0,
      dZ: d.Z ?? 0,
      dOffset: d.Offset ?? 0,
      dCount: d.Count ?? 0,
    })
  }
  return out
}

/** 종류·코드 필터('' = 전체). */
export function filt(rows: readonly MeasRow[], kind: string | number, code: string | number): MeasRow[] {
  const k = Number(kind || 0)
  const c = Number(code || 0)
  return rows.filter((r) => (!k || r.kind === k) && (!c || r.code === c))
}

/** 종류별 한 줄 요약. */
export function summary(r: MeasRow): string {
  const d = r.data
  switch (r.kind) {
    case 1:
      return `내경 ${f1(d[1])} (토크 ${f1(d[7])} / In ${f1(d[10])} / Out ${f1(d[15])})  비드 ${f1(d[2])}  높이 ${f1(d[3])}`
    case 2:
      return `${d[17] ?? 0}단  단당 ${f1(d[18])}  스택 ${f1(d[19])}`
    case 3:
      return `바닥 ${f1(d[1])} (Z ${f1(d[2])}, FLD ${f1(d[3])})`
    case 4:
      return `비드-Target ${f1(d[1])}  ZTarget ${d[2] ? f1(d[2]) : '-'}  내경 ${d[3] ? f1(d[5]) : '-'}`
    case 5:
      return `레이저 ${f1(d[1])}  토크 ${f1(d[2])}  높이 ${f1(d[3])}`
    default:
      return ''
  }
}

/** 필터 목록용 코드 집합(규격별 슬롯 + 이력에 보이는 코드). 오름차순. */
export function codesOf(byCode: readonly ByCode[] | undefined, rows: readonly MeasRow[]): number[] {
  const set = new Set<number>()
  for (const b of byCode ?? []) if (b.Code) set.add(b.Code)
  for (const r of rows) if (r.code) set.add(r.code)
  return [...set].sort((a, b) => a - b)
}
