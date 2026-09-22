// 레이아웃 맵의 스테이션 표현 규칙(DOM 없는 순수 함수) — GRM 실시간 상태(`/api/stations/live/stream`, 바뀔 때만).
//
// 채널마다 뜻 하나(2026-09-21 사용자 정리):
//   몸체 테두리 = **정합** — 화물 유무(ItemExist) · 트래킹 데이터(GRM OD) · 화물 코드(콘솔 재고)가 맞으면 초록,
//                 어긋나거나 GRM 오류면 빨강, 화물이 없고 남은 것도 없으면 기본 선.
//   안쪽 테두리 = **인터록** — 로봇 진입(CVNO) 주황 > 작업 완료(Comp) 보라 > 작업 요청(Req) 파랑 >
//                 측정 요청(MeasReq) 파랑 점선. CV 준비 안 됨(!CVOK)은 회색 점선(가장 앞).
//   화물       = 실제 크기·위치: 트래킹 OD·TaskOffset → 없으면 품목 외경(가로 0) → 없으면 도형 크기(점선).
//   바깥 윤곽(선택·작업·계획 …)은 셀과 같은 `mapStyleModel` 규칙.
//
// 비트 뜻은 GRM `Comm_CV` / `FB_Station`(백엔드 `issue/station_live.rs` 머리 주석).
import type { StationLive } from './types'

export type StationFill = 'disabled' | 'unknown' | 'normal'

export function stationFill(use: boolean, l: StationLive | undefined): StationFill {
  if (!use) return 'disabled'
  if (!l || !l.live || l.stale) return 'unknown'
  return 'normal'
}

/** 콘솔 재고(화물 코드) 한 칸 — 필요한 것만. */
export interface CodeStock {
  item_code: number
  count: number
}

export interface StationMatch {
  /** ok = 초록, mismatch = 빨강, null = 표시 없음(비어 있음 · 모름 · 이동 중 · 측정 중). */
  state: 'ok' | 'mismatch' | null
  /** 사람이 읽는 판정 이유(맞으면 맞는 이유, 어긋나면 어긋난 것들). */
  reasons: string[]
}

/** 트래킹 데이터를 기대하는 스테이션 — 측정 센서가 있거나 GRM 앞 연결로 넘겨받는다. */
export function expectsTracking(l: StationLive): boolean {
  return l.sensor || l.prev > 0
}

export function hasTracking(l: StationLive): boolean {
  return l.has_tracking || l.od > 0
}

export function stationMatch(
  l: StationLive | undefined,
  code: CodeStock | null | undefined,
): StationMatch {
  if (!l || !l.live || l.stale) return { state: null, reasons: [] }
  const bad: string[] = []
  if (l.error_code) bad.push(`GRM ErrorCode ${l.error_code}`)
  else if (l.state === 10) bad.push('GRM State 10 오류')
  if (l.measuring_error) bad.push('MeasuringError')
  if (l.po.meas_err) bad.push('측정 오류 신호(MeasErr)')
  if (l.data_mismatch) bad.push('DataMissMatch')
  const item = l.pi.item_exist
  const trk = hasTracking(l)
  const hasCode = (code?.count ?? 0) > 0
  const measuring = l.measuring || l.state === 2
  if (item) {
    if (expectsTracking(l) && !trk && !measuring) bad.push('화물 있음 · 트래킹 데이터 없음')
    if (!hasCode) bad.push('화물 있음 · 화물 코드 없음 — 더블 클릭으로 입력')
    if (bad.length) return { state: 'mismatch', reasons: bad }
    const ok = ['화물 있음', hasCode ? `코드 ${code?.item_code || '?'}` : '']
    if (trk) ok.push('트래킹 있음')
    return { state: 'ok', reasons: [ok.filter(Boolean).join(' · ')] }
  }
  if (trk) bad.push('화물 없음 · 트래킹 데이터 남음')
  // 다음 스테이션으로 가는 중이면(연결이 있으면) 코드가 잠시 남는 것이 정상 — 도착하면 옮겨진다.
  const transit = l.next_id !== null && l.next_id !== 0
  if (hasCode && !transit) bad.push('화물 없음 · 화물 코드 남음')
  if (bad.length) return { state: 'mismatch', reasons: bad }
  return { state: null, reasons: hasCode ? ['이동 중 — 다음 스테이션 도착 대기'] : ['비어 있음'] }
}

export type StationRing = 'cv_not_ready' | 'robot_in' | 'comp' | 'req' | 'meas_req'

/** 우선순위(앞이 높다). */
export const RING_ORDER: readonly StationRing[] = [
  'cv_not_ready',
  'robot_in',
  'comp',
  'req',
  'meas_req',
]

export function stationRing(l: StationLive | undefined): StationRing | null {
  if (!l || !l.live || l.stale) return null
  const on: Record<StationRing, boolean> = {
    cv_not_ready: !l.pi.cvok,
    robot_in: l.po.cvno,
    comp: l.po.comp,
    req: l.pi.req,
    meas_req: l.pi.meas_req,
  }
  return RING_ORDER.find((k) => on[k]) ?? null
}

export const MATCH_LABEL = { ok: '정합 — 화물·트래킹·코드 일치', mismatch: '불일치' } as const

export const RING_LABEL: Record<StationRing, string> = {
  cv_not_ready: 'CV 준비 안 됨 — CVOK 꺼짐',
  robot_in: '로봇 진입 — CVNO (CV 정지 요구)',
  comp: '로봇 작업 완료 — Comp',
  req: '작업 요청 — Req',
  meas_req: '측정 요청 — MeasReq',
}

/** `FB_Station` 상태 번호. */
export function stateLabel(n: number): string {
  switch (n) {
    case 0:
      return '0 대기'
    case 1:
      return '1 화물 수신 대기'
    case 2:
      return '2 측정 중'
    case 4:
      return '4 트래킹 보유'
    case 5:
      return '5 완료'
    case 10:
      return '10 오류'
    default:
      return String(n)
  }
}

export interface Lamp {
  key: string
  name: string
  desc: string
  on: boolean
}

/** 호버 인디케이터 — IN(PI, CV → GRM) / OUT(PO, GRM → CV). */
export function interlockLamps(l: StationLive): { pi: Lamp[]; po: Lamp[] } {
  return {
    pi: [
      { key: 'cvok', name: 'CVOK', desc: '컨베이어 정상', on: l.pi.cvok },
      { key: 'req', name: 'Req', desc: '로봇 작업 요청', on: l.pi.req },
      { key: 'meas_req', name: 'MeasReq', desc: '측정 요청', on: l.pi.meas_req },
      { key: 'item_exist', name: 'ItemExist', desc: '화물 감지', on: l.pi.item_exist },
    ],
    po: [
      { key: 'cvno', name: 'CVNO', desc: '로봇 진입 · CV 정지', on: l.po.cvno },
      { key: 'comp', name: 'Comp', desc: '로봇 작업 완료', on: l.po.comp },
      { key: 'meas_comp', name: 'MeasComp', desc: '측정 완료', on: l.po.meas_comp },
      { key: 'meas_err', name: 'MeasErr', desc: '측정 오류', on: l.po.meas_err },
    ],
  }
}

export interface Tire {
  /** Info.Position 기준 월드 오프셋(mm)과 지름(mm). */
  dx: number
  dy: number
  d: number
  /** tracking = GRM OD·TaskOffset, item = 품목 외경(가로 0), nominal = 모름(도형 크기, 점선). */
  source: 'tracking' | 'item' | 'nominal'
}

/**
 * 화물 원의 자리·크기 — GRM `StationCenterAdjust` 와 같은 식: 흐름 축에 ±OD/2(벽에 닿음), 가로축에 TaskOffset.
 * 1·5 (TX, −OD/2) · 2·6 (TX, +OD/2) · 3·7 (+OD/2, TY) · 4·8 (−OD/2, TY) · 그 밖 (TX, TY).
 */
export function tireOf(
  rotate: number,
  od: number,
  tx: number,
  ty: number,
  itemOd: number,
  nominal: number,
): Tire {
  const source: Tire['source'] = od > 0 ? 'tracking' : itemOd > 0 ? 'item' : 'nominal'
  const d = source === 'tracking' ? od : source === 'item' ? itemOd : nominal
  const [ox, oy] = source === 'tracking' ? [tx, ty] : [0, 0]
  switch (rotate) {
    case 1:
    case 5:
      return { dx: ox, dy: -d / 2, d, source }
    case 2:
    case 6:
      return { dx: ox, dy: d / 2, d, source }
    case 3:
    case 7:
      return { dx: d / 2, dy: oy, d, source }
    case 4:
    case 8:
      return { dx: -d / 2, dy: oy, d, source }
    default:
      return { dx: ox, dy: oy, d, source }
  }
}
