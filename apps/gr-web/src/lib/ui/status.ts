// 시맨틱 **상태 → 클래스** 단일 진실원. 컴포넌트마다 흩어져 있던 tone()/boolTone()/statusColor()
// (SchemaTable·EventTimeline·Fleet·ReportView)를 하나로 수렴한다 — green↔emerald 불일치를 봉합하고,
// StatusDot/StatusBadge가 이 표를 소비한다.
//
// **클래스는 시맨틱 토큰으로 부른다**(`bg-ok` · `text-ok-fg` · `bg-ok-soft`). 예전에는 원시 스케일과
// `dark:` 쌍을 함께 적었다(`bg-emerald-50 dark:bg-emerald-500/15`) — 그러면 ① 다크 배경을 투명도로
// 만들게 되고(면이 한 층 더 생긴다) ② 두 테마의 값이 이 파일에 박혀서 팔레트를 갈 때 여기도 같이
// 고쳐야 한다. 토큰으로 부르면 `tokens.css`의 라이트/다크 블록이 알아서 따라오고, 이 표는 **뜻만**
// 든다. 상태 여섯의 dot·fg·soft는 그 토큰과 1:1이다.
//
// 기본은 화이트톤이고 다크는 두 번째 층이다(`tokens.css`의 `[data-theme='dark']`).

export type Status = 'ok' | 'warn' | 'degraded' | 'fault' | 'info' | 'neutral'

export interface StatusClasses {
  /** 점/막대 채움색. */
  dot: string
  /** 텍스트 강조색(라이트/다크). */
  text: string
  /** 은은한 배경(뱃지·카드 강조). */
  soft: string
  /** 좌측 액센트 보더. */
  border: string
  /** 포커스/강조 ring. */
  ring: string
}

const MAP: Record<Status, StatusClasses> = {
  ok: {
    dot: 'bg-ok',
    text: 'text-ok-fg',
    soft: 'bg-ok-soft',
    border: 'border-ok',
    ring: 'ring-ok/30',
  },
  warn: {
    dot: 'bg-warn',
    text: 'text-warn-fg',
    soft: 'bg-warn-soft',
    border: 'border-warn',
    ring: 'ring-warn/30',
  },
  degraded: {
    dot: 'bg-degraded',
    text: 'text-degraded-fg',
    soft: 'bg-degraded-soft',
    border: 'border-degraded',
    ring: 'ring-degraded/30',
  },
  fault: {
    dot: 'bg-fault',
    text: 'text-fault-fg',
    soft: 'bg-fault-soft',
    border: 'border-fault',
    ring: 'ring-fault/30',
  },
  info: {
    dot: 'bg-info',
    text: 'text-info-fg',
    soft: 'bg-info-soft',
    border: 'border-info',
    ring: 'ring-info/30',
  },
  neutral: {
    dot: 'bg-neutral-dot',
    text: 'text-neutral-fg',
    soft: 'bg-neutral-soft',
    border: 'border-neutral-dot',
    ring: 'ring-neutral-dot/30',
  },
}

/** 상태 → 클래스 묶음(미지 상태는 neutral). */
export function statusTone(s: Status | string | undefined | null): StatusClasses {
  return MAP[(s as Status) ?? 'neutral'] ?? MAP.neutral
}

/** 헬스 레벨(mxcore HealthLevel 직렬화 "ok"|"degraded"|"fault") → Status. */
export function healthStatus(level: string | undefined | null): Status {
  if (level === 'fault') return 'fault'
  if (level === 'degraded') return 'degraded'
  if (level === 'ok') return 'ok'
  return 'neutral'
}

/** **제네릭** enum/text 값 → Status(모듈 무관 시맨틱 휴리스틱 — SchemaTable이 enum 셀에 씀).
 * fault/error/fail/alarm/estop/offline → fault, warn/degraded/stale/overflow → warn,
 * ok/enabled/online/ready/running/pass → ok, 그 외 neutral. */
export function valueStatus(v: string | undefined | null): Status {
  const s = String(v ?? '').toLowerCase()
  if (/fault|error|fail|alarm|estop|critical|offline|down|reaction/.test(s)) return 'fault'
  if (/warn|degrad|stale|paused|overflow|pending|quick_stop/.test(s)) return 'warn'
  if (/\bok\b|enabled|online|healthy|ready|running|active|pass|nominal|attained/.test(s))
    return 'ok'
  return 'neutral'
}

/** CiA402 드라이브 상태(snake_case) → Status(진단 색 매핑). */
export function driveStatus(driveState: string | undefined | null): Status {
  switch (driveState) {
    case 'operation_enabled':
      return 'ok'
    case 'fault':
    case 'fault_reaction_active':
      return 'fault'
    case 'quick_stop_active':
      return 'warn'
    case 'unknown':
      return 'neutral'
    default:
      return 'info' // 핸드셰이크 중간 상태(ready/switched_on 등).
  }
}

// ── 편집 셀의 라이프Cycle 상태 ─────────────────────────────────────────────
//
// 표에서 값을 고치면 "고쳤다 → 보냈다 → 앉았다/안 앉았다"가 서로 다른 사건이고, 색으로 구분하지
// 않으면 거부된 값이 화면에서 사라진다.

/** 셀 상태 — 우선순위 판정은 `cellStateFor`(lib/cellEdit.ts)가 소유한다. */
export type CellState =
  /** 적용했는데 값이 안 앉음(거부·클램프) — 드래프트를 유지한 채 빨갛게 남긴다. */
  | 'mismatch'
  /** 고쳤지만 아직 적용 안 함. */
  | 'draft'
  /** 적용·영속 확인됨(잠시 초록으로 알리고 사라진다). */
  | 'applied'
  /** 저장은 됐으나 재시작해야 반영(컨피그 tier boot·safety). */
  | 'pending-restart'
  /** 평상 상태. */
  | 'clean'

/** 셀 상태 → 배경·텍스트 클래스. 배경은 은은하게, 글자는 굵게(눈에 띄되 값을 가리지 않게). */
export function cellTone(state: CellState): { bg: string; text: string } {
  const of = (s: Status) => ({ bg: MAP[s].soft, text: `${MAP[s].text} font-semibold` })
  switch (state) {
    case 'mismatch':
      return of('fault')
    case 'draft':
      return of('warn')
    case 'applied':
      return of('ok')
    case 'pending-restart':
      // 상태 여섯을 빌리지 않는 유일한 자리 — 저하(주황)가 아니라 **보라**다.
      // 재시작해야 반영되는 값은 나쁜 상태가 아니라 '아직 살아 있지 않은' 시간축 표식이라,
      // 판정색으로 칠하면 고장·저하와 한 덩어리로 읽힌다.
      return {
        bg: 'bg-violet-50 dark:bg-violet-500/15',
        text: 'text-violet-700 dark:text-violet-300 font-semibold',
      }
    default:
      return { bg: '', text: '' }
  }
}
