// 표준 텍스트/숫자 입력 — 컴포넌트마다 복붙된 border/bg 클래스(~20곳) 대체. label·hint·mono 옵션.
//
// 인덱스 시그니처를 쓰지 않는다 — 초과 속성 검사가 꺼져 오타가 조용히 통과한다.
import type * as React from 'react'

export interface InputProps extends Omit<
  React.InputHTMLAttributes<HTMLInputElement>,
  'value' | 'type'
> {
  value?: string | number
  onValueChange?: (v: string) => void
  label?: string
  hint?: string
  placeholder?: string
  mono?: boolean
  type?: string
  className?: string
}

export function Input({
  value = '',
  onValueChange,
  label = '',
  hint = '',
  placeholder = '',
  mono = false,
  type = 'text',
  className = '',
  onChange,
  ...rest
}: InputProps) {
  return (
    <label className={`flex flex-col gap-1 ${className}`}>
      {label && (
        <span className="text-2xs font-medium text-content-muted">{label}</span>
      )}
      <input
        type={type}
        placeholder={placeholder}
        value={value}
        onChange={(e) => {
          onValueChange?.(e.target.value)
          onChange?.(e)
        }}
        className={`h-control-md rounded-md border border-line-strong bg-transparent px-2 text-sm text-content-primary transition-colors placeholder:text-content-faint focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none ${
          mono ? 'font-mono tabular-nums' : ''
        }`}
        {...rest}
      />
      {hint && <span className="text-3xs text-content-faint">{hint}</span>}
    </label>
  )
}
