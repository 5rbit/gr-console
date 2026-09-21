// 인디케이터 어휘 — 사이드바(로봇·PLC·OPC UA)와 상태바가 **같은 말과 같은 색**을 쓰게 하는 순수 모듈.
//
// 왜 만들었나(2026-09-21): 로봇 행의 빨간 사각(게이트 닫힘·레이아웃)과 PLC 행의 초록 사각(연결)·빨간 사각
// (레이아웃)이 글자 없이 섰고, 로봇 색 점이 또 하나의 상태 점처럼 보였다. 빨강이 "운전자가 손쓸 수 없는
// 설정 불일치"와 "게이트 닫힘"에도 쓰여 알람과 구분되지 않았다. 이제 상태는 **글자 + 톤**으로 서고,
// 톤은 넷뿐이다:
//
//   ok        정상 — 쓸 수 있다                       (연결 · 준비 · 제출 가능 · AUTO)
//   degraded  돌지만 막혔다 — 사유를 읽고 조치한다     (제출 불가 · 레이아웃 불일치)
//   danger    통신이 끊겼다 — 값이 낡았다             (끊김)
//   idle      아직 모른다 / 판정 대상 아님            (연결 중 · 미검사 · MANUAL 등)
//
// 화면은 `label`(짧은 한국어)·`tone`·`tooltip`(사유 전부)만 쓴다. 색 클래스는 `toneStatus` → `lib/ui/status`.
import type { Status } from './ui/status'
import type { PlcStatus, Robot } from './types'

export type Tone = 'ok' | 'degraded' | 'danger' | 'idle'

export interface Indicator {
  label: string
  tone: Tone
  /** 툴팁 — 사유·세부. 빈 문자열이면 툴팁 없음. */
  tooltip: string
}

/** 톤 → 상태 토큰(색). degraded 는 주황이 아니라 **노랑(warn)** — 빨강과 확실히 갈라지게. */
export function toneStatus(t: Tone): Status {
  switch (t) {
    case 'ok':
      return 'ok'
    case 'degraded':
      return 'warn'
    case 'danger':
      return 'fault'
    default:
      return 'neutral'
  }
}

/** 범례 — `?` 말풍선이 그대로 그린다(어휘의 정본이 이 표 하나). */
export const TONE_LEGEND: readonly { tone: Tone; name: string; words: string; meaning: string }[] = [
  { tone: 'ok', name: '정상', words: '연결 · 준비 · 제출 가능 · AUTO', meaning: '쓸 수 있음' },
  {
    tone: 'degraded',
    name: '막힘',
    words: '제출 불가 · 레이아웃 불일치',
    meaning: '통신은 되지만 막힘 — 툴팁의 사유 확인',
  },
  { tone: 'danger', name: '끊김', words: '끊김', meaning: '통신 없음 — 값이 낡음' },
  { tone: 'idle', name: '대기', words: '연결 중 · 미검사 · MANUAL', meaning: '아직 모름 / 판정 대상 아님' },
]

const SEP = ' · '

// ── 연결(S7 · OPC UA · 스트림) ────────────────────────────────────────────────

/** PLC 한 줄의 연결 상태. S7 은 `연결 4ms`/`끊김`, OPC UA 는 `준비`/`끊김`, 한 번도 붙지 않았으면 `연결 중`. */
export function plcConnection(p: PlcStatus): Indicator {
  const opc = p.kind === 'opcua'
  const where = p.endpoint
  if (p.connected) {
    const rtt = p.rtt_ms !== null ? `${Math.round(p.rtt_ms)}ms` : ''
    return {
      label: opc ? '준비' : rtt ? `연결 ${rtt}` : '연결',
      tone: 'ok',
      tooltip: [opc ? 'OPC UA 명령 경로 준비' : 'S7 연결', rtt && `RTT ${rtt}`, where]
        .filter(Boolean)
        .join(SEP),
    }
  }
  if (p.last_error || p.last_ok_at) {
    return {
      label: '끊김',
      tone: 'danger',
      tooltip: [p.last_error ?? '연결 끊김', p.last_ok_at && `마지막 정상 ${p.last_ok_at}`, where]
        .filter(Boolean)
        .join(SEP),
    }
  }
  return { label: '연결 중', tone: 'idle', tooltip: `아직 응답 없음${SEP}${where}` }
}

/** 불일치 한 건 → 짧은 사유(`WEBMON 서명` · `LASERDIAG 5242≠5188` · `PARA 없음`). */
export function mismatchText(m: PlcStatus['layout']['mismatches'][number]): string {
  switch (m.kind) {
    case 'signature':
      return `${m.db} 서명`
    case 'size': {
      const a = /size (\d+) B/.exec(m.actual)?.[1]
      const e = /(\d+) B/.exec(m.expected)?.[1]
      return a && e ? `${m.db} ${a}≠${e}` : `${m.db} 크기`
    }
    case 'missing':
      return `${m.db} 없음`
    case 'semantic': {
      // `STAT.ComponentID = 4001, expected 99` → `OPCUA STAT.ComponentID 4001≠99`
      const s = /^(.*) = (.*), expected (.*)$/.exec(m.actual)
      return s ? `${m.db} ${s[1]} ${s[2]}≠${s[3]}` : `${m.db} ${m.actual}`
    }
    default:
      return `${m.db} ${m.actual}`
  }
}

/**
 * 레이아웃 검사. **정상은 숨긴다**(`hidden`) — 연결 표시가 이미 초록이고, 행마다 `레이아웃 OK`를 반복하면
 * 불일치가 묻힌다. 불일치일 때만 `레이아웃 불일치`(degraded)가 서고 사유는 툴팁에.
 * 미검사도 숨긴다(끊겨 있으면 검사할 수 없다 — 연결 표시가 이미 말한다). OPC UA 행은 검사 대상이 아니다.
 */
export function plcLayout(p: PlcStatus): Indicator & { hidden: boolean } {
  if (p.kind === 'opcua') return { label: '', tone: 'idle', tooltip: '', hidden: true }
  if (p.layout.ok === false) {
    const why = p.layout.mismatches.map(mismatchText)
    return {
      label: '레이아웃 불일치',
      tone: 'degraded',
      tooltip: why.length ? why.join(SEP) : (p.layout.detail ?? '레이아웃 불일치'),
      hidden: false,
    }
  }
  if (p.layout.ok === true)
    return { label: '레이아웃 OK', tone: 'ok', tooltip: '계약과 DB 레이아웃 일치', hidden: true }
  return { label: '미검사', tone: 'idle', tooltip: '레이아웃 미검사', hidden: true }
}

/** PLC 이름 — 상태바·툴팁에서 부른다(`GR1`, OPC UA 는 라벨). */
export function plcName(p: PlcStatus): string {
  return p.name ?? p.label
}

/**
 * 상태바의 레이아웃 종합 — 어느 PLC 가 불일치인지 **이름으로** 말한다(`레이아웃 불일치 GR1 · GR2`).
 * 모두 OK면 `레이아웃 OK`, 목록 전이면 null(그리지 않는다), 일부 미검사면 `레이아웃 미검사`.
 */
export function layoutSummary(list: readonly PlcStatus[], loaded: boolean): Indicator | null {
  const s7 = list.filter((p) => p.kind === 's7')
  if (!loaded || s7.length === 0) return null
  const bad = s7.filter((p) => p.layout.ok === false)
  if (bad.length) {
    return {
      label: `레이아웃 불일치 ${bad.map(plcName).join(SEP)}`,
      tone: 'degraded',
      tooltip: bad.map((p) => `${plcName(p)}: ${plcLayout(p).tooltip}`).join('\n'),
    }
  }
  if (s7.every((p) => p.layout.ok === true))
    return { label: '레이아웃 OK', tone: 'ok', tooltip: s7.map(plcName).join(SEP) }
  const un = s7.filter((p) => p.layout.ok !== true).map(plcName)
  return { label: '레이아웃 미검사', tone: 'idle', tooltip: `미검사: ${un.join(SEP)}` }
}

/** SSE 스트림(상태바) — `상태 GR2 연결` / `끊김` / `연결 중`. */
export function feedIndicator(subject: string, connected: boolean, error: string | null): Indicator {
  if (connected) return { label: `${subject} 연결`, tone: 'ok', tooltip: `${subject} 스트림 연결` }
  if (error) return { label: `${subject} 끊김`, tone: 'danger', tooltip: error }
  return { label: `${subject} 연결 중`, tone: 'idle', tooltip: `${subject} 스트림 연결 중` }
}

// ── 로봇 ──────────────────────────────────────────────────────────────────────

/**
 * 로봇의 **하나뿐인** 상태 — 제출할 수 있나. 사유는 백엔드가 로봇 이름을 달아 준 그대로 툴팁에
 * (`GR1: Task.Status.Accept = FALSE · GR1: AUTO 모드가 아님`).
 * 막힌 이유가 통신 끊김(S7 · OPC UA 명령 경로 오류)이면 danger, 그 밖(Accept·모드·레이아웃·버퍼)은 degraded.
 */
export function robotGate(r: Robot): Indicator {
  if (r.gate.can_submit) return { label: '제출 가능', tone: 'ok', tooltip: `${r.name} 제출 가능` }
  const lost = !r.plc_connected || (!r.cmd_ready && !!r.cmd_error)
  const reasons = r.gate.reasons.length ? r.gate.reasons : [r.cmd_error ?? '사유 없음']
  return { label: '제출 불가', tone: lost ? 'danger' : 'degraded', tooltip: reasons.join(SEP) }
}

/** 운전 모드 칩 — 모를 때(스트림 전) null. AUTO=ok · FAULT=danger · 그 밖 idle. */
export function modeIndicator(name: string | null | undefined): Indicator | null {
  if (!name) return null
  const tone: Tone = name === 'AUTO' ? 'ok' : name === 'FAULT' ? 'danger' : 'idle'
  return { label: name, tone, tooltip: `운전 모드 ${name}` }
}
