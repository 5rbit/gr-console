// 라벨+컨트롤+힌트 한 벌 — `Input`·`Select`가 안에서 하던 배치를 **밖으로** 꺼낸 것.
//
// 킷의 입력에는 이미 `label`·`hint`가 있지만, 스위치·세그먼트·버튼 묶음·읽기 전용 값처럼 라벨이
// 필요한 다른 컨트롤에는 없다. 그래서 화면마다 `<span className="text-2xs …">`을 손으로 붙였고,
// 간격이 자리마다 달라졌다(`mb-1` · `gap-1` · `mb-2`가 한 폼 안에 섞여 있었다).
//
// 여기서 정하는 것은 **간격 한 벌**뿐이다: 라벨 11px 위, 컨트롤, 힌트 10px 아래. 컨트롤이 무엇인지는
// 모른다. `label`을 쓰므로 라벨을 눌러도 컨트롤이 잡힌다(장갑 낀 손의 과녁이 넓어진다).
import type { ReactNode } from 'react'
import { cn } from '../utils'

export interface FieldProps {
  label?: string
  /** 컨트롤 아래 한 줄 — 단위·기본값·제약. 규칙 안내문은 여기 쓰지 않는다. */
  hint?: ReactNode
  /** 왼쪽 라벨 + 오른쪽 컨트롤(대화상자의 좁은 폼). 기본은 라벨이 위에 선다. */
  inline?: boolean
  className?: string
  children: ReactNode
}

export function Field({ label, hint, inline = false, className = '', children }: FieldProps) {
  if (inline) {
    return (
      <label className={cn('flex items-center gap-2', className)}>
        {/* 라벨 폭은 글자에 맞춘다 — 고정 폭으로 두면 `Kind` 한 단어짜리 라벨과 컨트롤 사이에
            빈 칸이 생겨 둘이 한 짝으로 읽히지 않는다. 여러 줄을 세로로 맞춰야 하는 폼은
            바깥에서 격자를 쓴다. */}
        {label ? (
          <span className="flex-none text-2xs font-medium whitespace-nowrap text-content-muted">
            {label}
          </span>
        ) : null}
        <span className="flex min-w-0 flex-1 items-center gap-2">{children}</span>
        {hint ? <span className="flex-none text-3xs text-content-faint">{hint}</span> : null}
      </label>
    )
  }
  return (
    <label className={cn('flex min-w-0 flex-col gap-1', className)}>
      {label ? <span className="text-2xs font-medium text-content-muted">{label}</span> : null}
      {children}
      {hint ? <span className="text-3xs text-content-faint">{hint}</span> : null}
    </label>
  )
}
