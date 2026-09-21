// 스테이션 보정 블록 — GRM `StationCenterAdjust` 를 콘솔이 재현한 결과(RotateType · OD · 더한 TX/TY ·
// 기준 → 최종 XY · 스냅샷 나이 · 경고). 작성 미리보기에서는 끄기 스위치가 붙고(요청 `station_offset: 'off'`),
// Task 상세에서는 원장에 저장된 제출 시점 기록을 읽기 전용으로 보인다.
import {
  odSourceHint,
  offsetHeadline,
  stationOffsetFields,
} from '../../lib/task/stationOffsetModel'
import type { StationOffsetAudit } from '../../lib/task/types'
import { FieldList } from '../../lib/ui/FieldList'
import { Switch } from '../../lib/ui/Switch'

export interface StationOffsetBlockProps {
  audit: StationOffsetAudit | null | undefined
  /** 스위치 상태 — `onToggle` 이 있을 때만 그린다. */
  enabled?: boolean
  onToggle?: (on: boolean) => void
  /** 제목을 숨긴다(바깥 Section 이 제목을 가질 때). */
  bare?: boolean
}

export function StationOffsetBlock({
  audit,
  enabled = true,
  onToggle,
  bare = false,
}: StationOffsetBlockProps) {
  return (
    <div className="flex flex-col gap-1.5" data-testid="station-offset">
      {!bare || onToggle ? (
        <div className="flex items-center gap-2">
          {!bare ? (
            <span className="text-2xs font-semibold text-content-muted">StationOffset</span>
          ) : null}
          {audit ? (
            <span className="text-2xs text-content-faint tabular-nums">
              {offsetHeadline(audit)}
            </span>
          ) : null}
          <span className="flex-1" />
          {onToggle ? (
            <Switch
              inline
              checked={enabled}
              label="보정 적용"
              testid="station-offset-toggle"
              title="끄면 GRM 트래킹 없이 레지스트리 Info 위치로 보냅니다"
              onCheckedChange={onToggle}
            />
          ) : null}
        </div>
      ) : null}
      {!audit ? (
        <p className="m-0 text-2xs text-content-faint">PICK · DROP · MEASURE 에서 계산</p>
      ) : (
        <>
          {audit.blocked ? (
            <p
              className="m-0 rounded-md border border-fault bg-fault-soft px-2 py-1 text-xs text-fault-fg"
              data-testid="station-offset-blocked"
            >
              제출 거부 — {audit.blocked}
            </p>
          ) : null}
          {odSourceHint(audit) ? (
            <p
              className="m-0 rounded-md border border-warn bg-warn-soft px-2 py-1 text-xs text-warn-fg"
              data-testid="station-offset-itemspec"
            >
              {odSourceHint(audit)}
            </p>
          ) : null}
          <FieldList items={stationOffsetFields(audit)} columns={2} dense labelWidth={96} />
          {audit.warnings.length ? (
            <ul className="m-0 list-disc pl-4 text-xs text-warn-fg">
              {audit.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          ) : null}
        </>
      )}
    </div>
  )
}
