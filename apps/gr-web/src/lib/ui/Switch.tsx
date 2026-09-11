// 토글 스위치 — **지금 어느 쪽인가**가 한눈에 보여야 하는 자리
//
// ON/OFF 버튼 두 개는 「지금 값」과 「누를 값」이 같은 모양이라 눌러 보기 전에 상태를 못
// 읽는다. 강제처럼 걸린 채로 남는 것은 그 차이가 곧 사고다 — 스위치는 위치가 곧 상태다.
//
// `pending`은 명령을 보내 놓고 응답을 기다리는 동안이다. 낙관적으로 옮겨 놓으면 로봇이
// 거부했을 때 화면만 켜져 있다 — 그래서 옮기지 않고 흐리게만 둔다.
export interface SwitchProps {
  checked?: boolean
  label?: string
  disabled?: boolean
  pending?: boolean
  title?: string
  testid?: string
  onCheckedChange?: (next: boolean) => void
  /** `onCheckedChange` 별칭 — Svelte 원본 계약 유지 */
  onchange?: (next: boolean) => void
}

export function Switch({
  checked = false,
  label = '',
  disabled = false,
  pending = false,
  title = '',
  testid = '',
  onCheckedChange,
  onchange,
}: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label ? `${label} 토글` : '토글'}
      data-testid={testid || undefined}
      title={title}
      disabled={disabled}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-40 ${
        checked ? 'bg-indigo-600' : 'bg-slate-300 dark:bg-slate-600'
      } ${pending ? 'animate-pulse' : ''}`}
      onClick={() => (onCheckedChange ?? onchange)?.(!checked)}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${
          checked ? 'left-[18px]' : 'left-0.5'
        }`}
      />
    </button>
  )
}
