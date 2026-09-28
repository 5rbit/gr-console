// 알림 — 규칙 편집 폼 ↔ API 본문, 규칙 요약, 받은 알림 목록 합치기·확인.
import type { AlertRec, AlertRule, AlertRuleBody, EvtCatalog, RuleMatch } from './api'
import { codeTokens } from './evtFilterModel'
import { EVT_TRANS, isErrorCode } from './evtTypeModel'

export interface RuleForm {
  name: string
  enabled: boolean
  plc: string
  cat: string
  /** 쉼표 목록 — 이벤트 이름 또는 코드. */
  codes: string
  /** 레벨 이름, 빈 값 = 전부. */
  minLvl: string
  text: string
  /** A 값 조건, 빈 값 = 무관 (on/off 이벤트는 1 = ON). */
  aEq: string
  /** ErrorList 유형, 비면 무관. */
  types: string[]
  /** 전이(raise · clear · momentary · summary), 빈 값 = 무관. */
  trans: string
  cooldown: string
}

export const EMPTY_RULE_FORM: RuleForm = {
  name: '',
  enabled: true,
  plc: '',
  cat: '',
  codes: '',
  minLvl: '',
  text: '',
  aEq: '',
  types: [],
  trans: '',
  cooldown: '60',
}

export function ruleToForm(r: AlertRule): RuleForm {
  const m = r.match
  return {
    name: r.name,
    enabled: r.enabled,
    plc: m.plc ?? '',
    cat: m.cat ?? '',
    codes: (m.codes ?? []).join(', '),
    minLvl: m.min_lvl ?? '',
    text: m.text_contains ?? '',
    aEq: m.a_eq === undefined ? '' : String(m.a_eq),
    types: m.types ?? [],
    trans: m.trans ?? '',
    cooldown: String(r.cooldown_s),
  }
}

export function formToBody(f: RuleForm): AlertRuleBody {
  const match: RuleMatch = {}
  if (f.plc.trim()) match.plc = f.plc.trim()
  if (f.cat.trim()) match.cat = f.cat.trim()
  const codes = codeTokens(f.codes)
  if (codes.length) match.codes = codes
  if (f.minLvl.trim()) match.min_lvl = f.minLvl.trim()
  if (f.text.trim()) match.text_contains = f.text.trim()
  if (f.aEq.trim()) match.a_eq = Number(f.aEq.trim())
  if (f.types.length) match.types = [...f.types]
  if (f.trans) match.trans = f.trans
  return { name: f.name.trim(), enabled: f.enabled, match, cooldown_s: Number(f.cooldown) }
}

/** 저장할 수 없는 이유 — 없으면 `undefined`. 카탈로그가 있으면 이벤트 이름도 본다(서버도 다시 본다). */
export function ruleFormWhy(f: RuleForm, catalog?: EvtCatalog | null): string | undefined {
  if (!f.name.trim()) return '이름을 적으세요'
  if (f.aEq.trim() && !/^-?\d+$/.test(f.aEq.trim())) return 'A 는 정수입니다'
  if (!/^\d+$/.test(f.cooldown.trim()) || Number(f.cooldown) > 86_400)
    return 'cooldown 은 0~86400 초입니다'
  if (catalog) {
    const bad = codeTokens(f.codes).find(
      (t) => !/^\d+$/.test(t) && !isErrorCode(t) && !catalog.events.some((e) => e.name === t),
    )
    if (bad) return `모르는 이벤트: ${bad}`
  }
  return undefined
}

/** 규칙의 조건 한 줄 — 비면 `전체`. */
export function matchSummary(m: RuleMatch): string {
  const parts: string[] = []
  if (m.plc) parts.push(m.plc)
  if (m.types?.length) parts.push(m.types.join('/'))
  if (m.trans) parts.push(EVT_TRANS.find((t) => t.id === m.trans)?.label ?? m.trans)
  if (m.cat) parts.push(m.cat)
  if (m.min_lvl) parts.push(`≥ ${m.min_lvl}`)
  if (m.codes?.length) parts.push(m.codes.join(', '))
  if (m.text_contains) parts.push(`"${m.text_contains}"`)
  if (m.a_eq !== undefined) parts.push(`A = ${m.a_eq}`)
  return parts.length ? parts.join(' · ') : '전체'
}

/** 들어온 알림을 합친다 — id 로 중복을 빼고 최신 먼저, 상한까지. */
export function mergeAlerts(
  cur: readonly AlertRec[],
  incoming: readonly AlertRec[],
  cap = 200,
): AlertRec[] {
  const seen = new Set<number>()
  const out: AlertRec[] = []
  for (const r of [...incoming, ...cur]) {
    if (seen.has(r.id)) continue
    seen.add(r.id)
    out.push(r)
  }
  out.sort((a, b) => b.id - a.id)
  return out.slice(0, cap)
}

/** 확인된 알림을 뺀다 — 빈 목록 = 전부. */
export function applyAck(cur: readonly AlertRec[], ids: readonly number[]): AlertRec[] {
  if (ids.length === 0) return []
  const set = new Set(ids)
  return cur.filter((r) => !set.has(r.id))
}

/** 브라우저 알림의 본문 — 이벤트가 남아 있으면 그 문구. */
export function alertBody(a: AlertRec): string {
  const e = a.event
  return e ? `${e.plc} ${e.ecode ? `${e.ecode} ` : ''}${e.text}` : a.rule_name
}
