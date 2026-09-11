// 시맨틱 **상태 → 클래스** 단일 진실원. 컴포넌트마다 흩어져 있던 tone()/boolTone()/statusColor()
// (SchemaTable·EventTimeline·Fleet·ReportView)를 하나로 수렴한다 — green↔emerald 불일치를 봉합하고,
// StatusDot/StatusBadge가 이 표를 소비한다. 다크 대응 soft 배경 포함.
//
// 색값은 `app.css`의 @theme이 갖는다 — 여기 적힌 `emerald`·`amber`·`sky`·`slate`는 mx-console
// 팔레트로 재정의된 이름이라(브랜드 그린 · 골드 · 파랑 · 따뜻한 회색) 클래스 문자열은 그대로 두고
// 값만 갈린다. 상태 여섯의 dot·fg·soft·border는 디자인 시스템 `tokens/status.css`와 1:1이다.

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
    dot: 'bg-emerald-500',
    text: 'text-emerald-700 dark:text-emerald-400',
    soft: 'bg-emerald-50 dark:bg-emerald-500/15',
    border: 'border-emerald-500',
    ring: 'ring-emerald-500/30',
  },
  warn: {
    dot: 'bg-amber-500',
    text: 'text-amber-700 dark:text-amber-400',
    soft: 'bg-amber-50 dark:bg-amber-500/15',
    border: 'border-amber-500',
    ring: 'ring-amber-500/30',
  },
  degraded: {
    dot: 'bg-orange-500',
    text: 'text-orange-600 dark:text-orange-400',
    soft: 'bg-orange-50 dark:bg-orange-500/15',
    border: 'border-orange-500',
    ring: 'ring-orange-500/30',
  },
  fault: {
    dot: 'bg-red-500',
    text: 'text-red-600 dark:text-red-400',
    soft: 'bg-red-50 dark:bg-red-500/15',
    border: 'border-red-500',
    ring: 'ring-red-500/30',
  },
  info: {
    dot: 'bg-sky-500',
    text: 'text-sky-600 dark:text-sky-400',
    soft: 'bg-sky-50 dark:bg-sky-500/15',
    border: 'border-sky-500',
    ring: 'ring-sky-500/30',
  },
  neutral: {
    dot: 'bg-slate-400',
    text: 'text-slate-500 dark:text-slate-400',
    soft: 'bg-slate-100 dark:bg-slate-800',
    border: 'border-slate-400',
    ring: 'ring-slate-400/30',
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
