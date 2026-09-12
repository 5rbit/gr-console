// 표준 선택 입력 — `Input`의 짝.
//
// `Input`과 같은 계약을 쓴다: `label`·`hint`가 있으면 위아래로 붙고, 나머지 속성은 `<select>`로 흘린다.
// 인덱스 시그니처를 쓰지 않는다 — 초과 속성 검사가 꺼져 오타가 조용히 통과한다.
import type * as React from 'react'

export interface SelectProps extends Omit<
  React.SelectHTMLAttributes<HTMLSelectElement>,
  'value' | 'size'
> {
  value?: string | number
  onValueChange?: (v: string) => void
  label?: string
  hint?: string
  /** 조밀한 툴바용(28px). 기본은 폼 높이(32px). `size`는 HTML select의 표시 행 수라 이름을 피한다 */
  dense?: boolean
  className?: string
  children?: React.ReactNode
}

const box =
  'rounded-md border border-slate-300 bg-transparent px-2 text-sm text-slate-900 transition-colors focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-600 dark:text-slate-100'

export function Select({
  value = '',
  onValueChange,
  label = '',
  hint = '',
  dense = false,
  className = '',
  children,
  onChange,
  ...rest
}: SelectProps) {
  return (
    <label className={`flex flex-col gap-1 ${className}`}>
      {label && (
        <span className="text-2xs font-medium text-slate-500 dark:text-slate-400">{label}</span>
      )}
      <select
        value={value}
        onChange={(e) => {
          onValueChange?.(e.target.value)
          onChange?.(e)
        }}
        className={`${box} ${dense ? 'h-control-sm text-xs' : 'h-control-md'}`}
        {...rest}
      >
        {children}
      </select>
      {hint && <span className="text-3xs text-slate-400">{hint}</span>}
    </label>
  )
}
