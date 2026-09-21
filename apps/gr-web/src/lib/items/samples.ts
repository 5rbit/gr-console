// 화물 규격 › Beads 탭의 "Measured" 목록 — SKU 측정 표본 줄의 순수 모델.
//
// PLC 가 measureSKU 로 스택 하나를 통째로 재면 표본 하나가 하중 곡선의 점 n 개를 채운다. 여기서는 그 표본
// 목록을 화면이 그대로 그릴 수 있는 모양으로 바꾼다 — 서버가 준 `applied` / `reason` 을 손대지 않고,
// 몇 번째 하중을 채웠는지(`aboves`)와 사람이 읽을 상태 글만 더한다.
//
// 필드 이름은 PLC/데이터 이름 그대로(영문). 설명·상태 글만 한국어.
import type { BeadSample } from '../types'

/** 표본 줄의 상태 — 색·아이콘을 고르는 데 쓴다. */
export type SampleState = 'applied' | 'rejected' | 'pending'

export interface SampleRow {
  /** `${plc}#${seq}` — 목록 key. */
  id: string
  plc: string
  seq: number
  /** PLC 가 찍은 시각(초까지). */
  time: string
  /** 잰 스택의 단수. */
  totalCount: number
  eachHeight: number | null
  totalHeight: number | null
  /** MEASLOG Status(3 이상 = 오류). */
  status: number
  diagFlags: number
  /** 값이 있는 단 수. */
  filled: number
  /** 이 표본이 채운(채울) 하중 값들 — n 단 스택은 Above 0..n−1. */
  aboves: number[]
  state: SampleState
  /** 반영 내용 또는 거부 사유(서버 글 그대로). */
  reason: string
  /** 지금 규칙으로도 넣을 수 있는가 — Apply 버튼을 켜는 조건. */
  canApply: boolean
  /** 상태 한 줄. */
  label: string
}

export const round1 = (v: number): number => Math.round(v * 10) / 10

/** `2026-09-18 10:11:12.345` → `2026-09-18 10:11:12` (빈 값은 `—`). */
export function sampleTime(at: string): string {
  const t = (at ?? '').trim()
  if (t === '') return '—'
  return t.replace('T', ' ').slice(0, 19)
}

/** `0` 은 빈칸으로, 나머지는 소수 한 자리. */
export function mm(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v) || v === 0) return '—'
  return String(round1(v))
}

/** `n` 단 스택이 채우는 하중 값 — k 단 타이어 위에는 n − k 개가 있다. */
export function abovesOf(totalCount: number): number[] {
  if (!Number.isInteger(totalCount) || totalCount <= 0) return []
  return Array.from({ length: Math.min(totalCount, 20) }, (_, i) => i)
}

export function sampleState(s: BeadSample): SampleState {
  if (s.applied) return 'applied'
  return s.valid ? 'pending' : 'rejected'
}

const STATE_LABEL: Record<SampleState, string> = {
  applied: '반영됨',
  rejected: '거부',
  pending: '대기',
}

export function toRow(s: BeadSample): SampleRow {
  const state = sampleState(s)
  const aboves = abovesOf(s.total_count)
  return {
    id: `${s.plc}#${s.seq}`,
    plc: s.plc,
    seq: s.seq,
    time: sampleTime(s.at),
    totalCount: s.total_count,
    eachHeight: Number.isFinite(s.each_height) && s.each_height > 0 ? round1(s.each_height) : null,
    totalHeight: Number.isFinite(s.total_height) && s.total_height > 0 ? round1(s.total_height) : null,
    status: s.status,
    diagFlags: s.diag_flags,
    filled: s.filled,
    aboves,
    state,
    reason: s.reason ?? '',
    canApply: s.valid && aboves.length > 0,
    label: state === 'applied' ? STATE_LABEL.applied : `${STATE_LABEL[state]}${s.reason ? ` — ${s.reason}` : ''}`,
  }
}

/** 최신 순(서버 순서를 믿되, 같은 PLC 안에서 seq 로 한 번 더 정렬). */
export function sampleRows(samples: readonly BeadSample[] | null | undefined): SampleRow[] {
  return [...(samples ?? [])].sort((a, b) => b.seq - a.seq || a.plc.localeCompare(b.plc)).map(toRow)
}

/** 목록 머리의 개수 — **문장이 아니라 라벨+값 짝**으로 센다(화면이 칩으로 편다). */
export interface SamplesStats {
  total: number
  applied: number
  rejected: number
  pending: number
}

export function samplesStats(rows: readonly SampleRow[]): SamplesStats {
  const by = (s: SampleState) => rows.filter((r) => r.state === s).length
  return {
    total: rows.length,
    applied: by('applied'),
    rejected: by('rejected'),
    pending: by('pending'),
  }
}

/** 어느 하중이 어느 표본에서 왔는지 — 곡선 표의 SampleSeq 칸 풀이. */
export function sampleLabel(plc: string, seq: number): string {
  return seq === 0 ? '—' : `${plc || '?'}#${seq}`
}
