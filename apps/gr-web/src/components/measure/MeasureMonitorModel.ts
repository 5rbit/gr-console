// 측정 모니터 머리 숫자 띠의 **순수 판정** — WEBMON 한 장에서 "지금 무엇을 보고 있나"를 뽑는다.
//
// 이것이 화면에서 제일 중요한 줄이라 DOM 없이 테스트로 못 박는다. 전에는 같은 값들이 하위 탭마다
// 다른 카드 격자로 서 있었다(대시보드 9 · 측정 진행 7 · 이력 3+). 탭을 옮길 때마다 값이 자리와
// 모양을 바꾸면 눈이 매번 다시 찾는다 — 그래서 **하위 탭과 무관하게 한 줄**로 세운다.
import { modeName } from '../../lib/gr/const'
import { f1, tt } from '../../lib/meas/format'
import type { Status } from '../../lib/ui/status'
import type { MeasLogSnapshot, WebMon } from '../../lib/types'

/** 띠 항목 하나 — `lib/ui/StatRow`의 `StatItem`과 같은 모양이되 ReactNode를 쓰지 않는다(순수). */
export interface BandItem {
  label: string
  value: string
  hint?: string
  tone?: Status
  testid?: string
}

/** 측정 네 가지(Item·Sku·Floor·LastBead) 중 지금 도는 것 — 없으면 마지막으로 끝난 것. */
function measurePhase(m: WebMon['Measure'] | undefined): { text: string; tone: Status } {
  const names: readonly [string, Record<string, unknown> | undefined][] = [
    ['Item', m?.Item],
    ['Sku', m?.Sku],
    ['Floor', m?.Floor],
    ['LastBead', m?.LastBead],
  ]
  const busy = names.filter(([, o]) => Boolean(o?.Busy)).map(([n]) => n)
  if (busy.length) return { text: `${busy.join(' · ')} 측정중`, tone: 'info' }
  const done = names.filter(([, o]) => Boolean(o?.Done)).map(([n]) => n)
  if (done.length) return { text: `${done.join(' · ')} 완료`, tone: 'ok' }
  return { text: '대기', tone: 'neutral' }
}

/** 알람 — 코드가 있으면 코드를, 없으면 비트만 말한다. 정상은 색을 쓰지 않는다. */
function alarmItem(a: WebMon['Alarm'] | undefined): BandItem {
  const fault = (a?.FaultCode ?? []).filter((c) => c)
  const warn = (a?.WarnCode ?? []).filter((c) => c)
  if (a?.Fault || fault.length)
    return { label: '알람', value: fault.join(' · ') || 'Fault', tone: 'fault', testid: 'band-alarm' }
  if (a?.Warn || warn.length)
    return { label: '알람', value: warn.join(' · ') || 'Warn', tone: 'warn', testid: 'band-alarm' }
  return { label: '알람', value: '없음', testid: 'band-alarm' }
}

/**
 * 머리 숫자 띠 — 로봇 상태(모드·상태·Task·Step) · 측정 · 축 · 기록 수.
 *
 * 순서는 **묻는 순서**다: 지금 어느 모드인가 → 장비가 무슨 상태인가 → 무슨 작업 중인가 →
 * 어디까지 갔나 → 측정은 어떤가 → 높이·그립은 어디인가 → 기록이 몇 건인가.
 */
export function bandItems(wm: WebMon | null, snap: MeasLogSnapshot | null): BandItem[] {
  if (!wm)
    return [
      { label: '모드', value: '—', testid: 'band-mode' },
      { label: '상태', value: 'PLC 상태 없음', tone: 'neutral', testid: 'band-state' },
      { label: 'MeasLog', value: String(snap?.total ?? '—'), testid: 'band-measlog' },
    ]

  const st = wm.Stat?.Status ?? {}
  const ts = wm.Stat?.Task?.Status ?? {}
  const now = wm.Stat?.Task?.Now
  const axis = wm.Axis ?? []
  const fault = Boolean(st.Fault)
  const warn = Boolean(st.Warn)
  const meas = measurePhase(wm.Measure)

  return [
    {
      label: '모드',
      value: modeName(wm.Mode),
      tone: wm.Mode === 32 ? 'ok' : wm.Mode === 128 ? 'fault' : 'warn',
      hint: 'MACHINE.Mode',
      testid: 'band-mode',
    },
    {
      label: '상태',
      value: fault ? 'FAULT' : warn ? 'WARN' : st.Busy ? 'BUSY' : st.Idle ? 'IDLE' : '-',
      tone: fault ? 'fault' : warn ? 'warn' : 'ok',
      hint: `StatusCode ${wm.Stat?.StatusCode?.Main ?? ''}/${wm.Stat?.StatusCode?.Sub ?? ''}`,
      testid: 'band-state',
    },
    {
      label: 'Task',
      value: now?.WorkId ? `${now.WorkId} / ${now.TaskId}` : '-',
      tone: ts.Inprogress ? 'info' : 'neutral',
      hint: `${tt(now?.TaskType)} Cell ${now?.Cell?.Id ?? ''} Code ${now?.Item?.Code ?? ''}`,
      testid: 'band-task',
    },
    {
      label: 'Step',
      value: String(wm.Proc?.Step?.Now ?? ''),
      hint: `${f1(wm.Proc?.Step?.ElapseTime)} s ${wm.Proc?.Msg ?? ''}`.trim(),
      testid: 'band-step',
    },
    { label: '측정', value: meas.text, tone: meas.tone, testid: 'band-meas' },
    alarmItem(wm.Alarm),
    {
      label: 'Z / G (mm)',
      value: `${f1(axis[2]?.Position)} / ${f1(axis[3]?.Position)}`,
      tone: axis[2]?.Running || axis[3]?.Running ? 'info' : 'neutral',
      hint: `→ ${f1(axis[2]?.Target)} / ${f1(axis[3]?.Target)}`,
      testid: 'band-axis',
    },
    {
      label: 'Item',
      value: wm.Gripper?.ItemDetect ? '감지' : '없음',
      tone: wm.Gripper?.ItemDetect ? 'ok' : 'neutral',
      hint: `Speed ${st.Speed ?? ''} %`,
      testid: 'band-item',
    },
    {
      label: 'MeasLog',
      value: String(wm.MeasLog?.Total ?? snap?.total ?? '-'),
      hint: `버퍼 ${wm.MeasLog?.Count ?? snap?.count ?? '-'} / ${snap?.capacity ?? '-'}`,
      testid: 'band-measlog',
    },
  ]
}
