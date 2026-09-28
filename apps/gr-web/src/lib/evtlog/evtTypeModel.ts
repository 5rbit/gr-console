// ErrorList 유형(Alarm · Warn · Operator · Info · Task)과 문구 언어.
//
// 유형 · 코드 · 전이는 서버가 행마다 붙인다(`etype` · `ecode` · `trans`, docs/evtlog/encoding-v2.md) — 화면은
// 다시 판정하지 않는다. 영어 문구(`text_en`)는 한국어와 다를 때만 온다.
import type { Status } from '../ui/status'
import type { EventRow } from './api'

export type EvtType = 'Alarm' | 'Warn' | 'Operator' | 'Info' | 'Task'
export type EvtTrans = 'raise' | 'clear' | 'momentary' | 'summary'
export type EvtLang = 'ko' | 'en'

export const EVT_TYPES: { id: EvtType; tone: Status }[] = [
  { id: 'Alarm', tone: 'fault' },
  { id: 'Warn', tone: 'warn' },
  { id: 'Operator', tone: 'info' },
  { id: 'Info', tone: 'neutral' },
  { id: 'Task', tone: 'neutral' },
]

export const EVT_TRANS: { id: EvtTrans; label: string }[] = [
  { id: 'raise', label: '발생' },
  { id: 'clear', label: '해제' },
  { id: 'momentary', label: '순간' },
  { id: 'summary', label: '억제 요약' },
]

export function typeTone(t: string | null | undefined): Status {
  return EVT_TYPES.find((x) => x.id === t)?.tone ?? 'neutral'
}

/** 대소문자 무시, 모르는 이름은 버린다. */
export function parseTypes(list: readonly string[]): EvtType[] {
  const out: EvtType[] = []
  for (const s of list) {
    const t = EVT_TYPES.find((x) => x.id.toLowerCase() === s.trim().toLowerCase())?.id
    if (t && !out.includes(t)) out.push(t)
  }
  return out
}

/** `F0101` · `W1101` · `O0105` · `I0301` — ErrorList 코드. */
export function isErrorCode(token: string): boolean {
  return /^[FWOI]\d{4}$/i.test(token.trim())
}

/** 보는 언어의 문구 — 영어가 없으면(카탈로그 문장) 한국어 그대로. */
export function rowText(r: Pick<EventRow, 'text' | 'text_en'>, lang: EvtLang): string {
  return lang === 'en' && r.text_en ? r.text_en : r.text
}

const LANG_KEY = 'gr.evt.lang'

/** 이 브라우저의 문구 언어 — 저장소가 막혀 있으면 한국어. */
export function loadLang(): EvtLang {
  try {
    return localStorage.getItem(LANG_KEY) === 'en' ? 'en' : 'ko'
  } catch {
    return 'ko'
  }
}

export function saveLang(l: EvtLang): void {
  try {
    localStorage.setItem(LANG_KEY, l)
  } catch {
    // 저장이 막힌 브라우저 — 이번 창에서만 바뀐다
  }
}
