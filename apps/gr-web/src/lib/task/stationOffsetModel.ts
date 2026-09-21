// 스테이션 보정 표시 — 백엔드 감사 기록(`StationOffsetAudit`)을 FieldList 항목과 한 줄 요약으로.
//
// 계산은 전부 백엔드(`issue/station_offset.rs`)가 한다. 여기는 숫자를 읽기 좋게 옮기기만 한다 —
// 같은 기록이 작성 미리보기와 Task 상세(원장에 저장된 제출 시점 값) 두 곳에 같은 모양으로 뜬다.
import type { FieldItem } from '../ui/fieldListModel'
import type { StationOffsetAudit, StationOffsetRow } from './types'

export const OFFSET_MODE_LABEL: Record<StationOffsetAudit['mode'], string> = {
  auto: '자동 — 트래킹 반영',
  override: '생략 — 위치 직접 지정',
  off: '꺼짐 — 요청',
}

/** 소수 1자리. 음수 0 은 0 으로. */
export function mm(v: number | null | undefined): string {
  if (typeof v !== 'number' || !Number.isFinite(v)) return ''
  const r = Math.round(v * 10) / 10
  return (r === 0 ? 0 : r).toFixed(1)
}

/** 부호를 붙인 소수 1자리(더한 값). */
export function signedMm(v: number): string {
  const s = mm(v)
  return s === '' || s.startsWith('-') || s === '0.0' ? s : `+${s}`
}

export function xy(p: [number, number] | null | undefined): string {
  return p ? `${mm(p[0])}, ${mm(p[1])}` : ''
}

/** `StationCenterAdjust` 의 CASE 를 한 줄로 — 어느 축에 OD/2 가 붙는가. */
export function rotateLabel(rot: number): string {
  switch (rot) {
    case 1:
    case 5:
      return `${rot} · Y −OD/2, X 측정`
    case 2:
    case 6:
      return `${rot} · Y +OD/2, X 측정`
    case 3:
    case 7:
      return `${rot} · X +OD/2, Y 측정`
    case 4:
    case 8:
      return `${rot} · X −OD/2, Y 측정`
    default:
      return `${rot} · 측정 오프셋 그대로`
  }
}

export type OdSource = NonNullable<StationOffsetAudit['od_source']>

/** 예전 원장 기록에는 `od_source` 가 없다 — 측정 OD 가 있으면 Tracking, 아니면 None 으로 읽는다. */
export function odSource(a: StationOffsetAudit): OdSource {
  return a.od_source ?? (a.od ? 'tracking' : 'none')
}

/** 필드 값 — 영문 이름 그대로 + 쓴 외경. */
export function odSourceText(a: StationOffsetAudit): string {
  const s = odSource(a)
  if (s === 'none') return ''
  const used = a.od_used ?? a.od
  return `${s === 'tracking' ? 'Tracking' : 'ItemSpec'} · ${mm(used)}`
}

/** 폴백일 때만 붙는 한 줄 설명(툴팁·안내문). */
export function odSourceHint(a: StationOffsetAudit): string | undefined {
  if (odSource(a) !== 'item_spec') return undefined
  return `GRM 측정값이 없어 등록 품목 OuterDiameter ${mm(a.od_used ?? 0)} 로 보정했습니다 — GR2 isValidTaskArea 와 같은 식(방향 축에만 ±OD/2, 가로축 0)`
}

/** 스냅샷 나이 — 1초 미만 ms, 1분 미만 초(1자리), 그 이상 분. */
export function fmtAge(ms: number | null | undefined): string {
  if (typeof ms !== 'number' || !Number.isFinite(ms)) return ''
  const v = Math.max(0, ms)
  if (v < 1000) return `${Math.round(v)} ms`
  if (v < 60_000) return `${(v / 1000).toFixed(1)} s`
  return `${Math.round(v / 60_000)} 분`
}

export type OffsetTone = 'fault' | 'warn' | 'ok' | 'info'

/** 막힘 > 경고 > 생략/꺼짐 > 정상. */
export function offsetTone(a: StationOffsetAudit): OffsetTone {
  if (a.blocked) return 'fault'
  if (a.warnings.length && a.mode === 'auto') return 'warn'
  if (a.mode !== 'auto') return 'info'
  return 'ok'
}

/** 제목 옆 한 줄 — 더한 값 또는 막힌/생략 이유의 짧은 형태. */
export function offsetHeadline(a: StationOffsetAudit): string {
  if (a.blocked) return '제출 거부'
  if (a.mode !== 'auto') return OFFSET_MODE_LABEL[a.mode]
  const src = odSource(a) === 'item_spec' ? ' · ItemSpec' : ''
  return `TX ${signedMm(a.tx_applied)} · TY ${signedMm(a.ty_applied)}${src}`
}

export function stationOffsetFields(a: StationOffsetAudit): FieldItem[] {
  const tone = offsetTone(a)
  return [
    {
      label: 'Mode',
      value: OFFSET_MODE_LABEL[a.mode],
      status: tone === 'ok' ? undefined : tone,
    },
    {
      label: 'Station (Id · Slot)',
      value: `${a.station_id} · ${a.slot || '범위 밖'}`,
      tooltip: 'PLC 는 STATION.Station[id MOD 100] 을 읽는다',
    },
    { label: 'RotateType', value: rotateLabel(a.rotate_type), tooltip: 'GRM Para.RotateType' },
    {
      label: 'OD',
      value: a.od ? mm(a.od) : null,
      missing: '트래킹 없음(OD 0)',
      tooltip: 'GRM Tracking.Now.OutterDiameter — 측정값',
    },
    {
      label: 'Now TX / TY',
      value: `${mm(a.now_tx)} / ${mm(a.now_ty)}`,
      tooltip: 'GRM Tracking.Now.TaskOffset — 흐름 방향 축은 ±OD/2 로 덮인다',
    },
    {
      label: 'Applied TX / TY',
      value: `${signedMm(a.tx_applied)} / ${signedMm(a.ty_applied)}`,
      status: odSource(a) === 'item_spec' ? 'warn' : undefined,
      tooltip: odSourceHint(a),
    },
    {
      label: 'OdSource',
      value: odSourceText(a) || null,
      missing: a.mode === 'auto' ? '쓸 외경 없음 — 보정 0' : '보정 안 함',
      status: odSource(a) === 'item_spec' ? 'warn' : undefined,
      tooltip: odSourceHint(a) ?? '보정에 쓴 외경 — Tracking = GRM 측정, ItemSpec = 등록 품목 스펙',
    },
    {
      label: 'Base → Final XY',
      value: `${xy(a.base_xy)} → ${xy(a.final_xy)}`,
      wide: true,
      tooltip: '기준 = 레지스트리 Info 위치(또는 직접 지정한 위치)',
    },
    {
      label: 'Snapshot',
      value: a.source ? `GRM ${a.source}${a.age_ms !== null ? ` · ${fmtAge(a.age_ms)} 전` : ''}` : null,
      missing: 'GRM 스냅샷 없음',
      tooltip: a.snapshot_at ?? undefined,
    },
    {
      label: 'GR2 Expected XY',
      value: a.gr2_expected_xy ? `${xy(a.gr2_expected_xy)} ± ${a.gr2_margin ?? ''}` : null,
      missing: '검사 안 함',
      tooltip: 'GR2 isValidTaskArea — Info ± 품목 외경/2, 마진 650(TaskType≠0) / 300',
    },
  ]
}

/** 스테이션 보정 표의 상태 칸 — 한 줄에 **가장 급한 것 하나**(나머지는 행 펼침의 경고 목록). */
export interface OffsetRowState {
  status: 'fault' | 'warn' | 'info' | 'neutral' | 'ok'
  text: string
}

export function offsetRowState(r: StationOffsetRow): OffsetRowState {
  if (r.pallet) return { status: 'info', text: '팔렛 — 보정 안 함' }
  if (r.blocked) return { status: 'fault', text: '트래킹 이상' }
  if (r.measuring_error) return { status: 'fault', text: 'MeasuringError' }
  if (r.data_mismatch) return { status: 'warn', text: 'DataMissMatch' }
  if (!r.source) return { status: 'warn', text: 'GRM 스냅샷 없음' }
  if (r.rotate_type !== r.registry_rotate_type) return { status: 'warn', text: 'RotateType 불일치' }
  // 측정 OD 가 없는 것은 컨베이어에 타이어가 없을 때의 **평소 상태**다 — 경고가 아니다. 작업을 보내면
  // 등록 품목 OuterDiameter 로 대신 보정한다(`odSourceHint`).
  if (odSource(r) !== 'tracking') return { status: 'neutral', text: '측정 없음' }
  return { status: 'ok', text: '측정' }
}
