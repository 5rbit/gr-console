// 이력 CSV — GrWeb `csv()` 와 같은 열 순서(BOM 포함, Excel 호환).
import type { MeasRow } from './rows'

export const CSV_COLUMNS = [
  'seq', 'time', 'kind', 'kindName', 'status', 'statusName', 'workId', 'taskId', 'taskType', 'cellId', 'code',
  'cmdCount', 'cmdId', 'cmdOd', 'cmdHeight', 'cmdX', 'cmdY', 'cmdZ', 'cmdG', 'cellZ', 'cmdZRel', 'flags',
  'dInnerDia', 'dHeight', 'dZ', 'dOffset', 'dCount',
] as const

function cell(v: unknown): string {
  const s = v === null || v === undefined ? '' : String(v)
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s
}

export function toCsv(rows: readonly MeasRow[]): string {
  let s = CSV_COLUMNS.join(',') + Array.from({ length: 20 }, (_, i) => `,data${i}`).join('') + '\n'
  for (const r of rows) {
    s += CSV_COLUMNS.map((c) => cell(r[c])).join(',')
    for (let i = 0; i < 20; i++) s += ',' + cell(r.data[i] ?? '')
    s += '\n'
  }
  return '﻿' + s
}

/** 브라우저 다운로드(파일 이름에 시각). */
export function downloadCsv(name: string, text: string): void {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }))
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}

export function csvFileName(): string {
  return `measlog_${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '')}.csv`
}
