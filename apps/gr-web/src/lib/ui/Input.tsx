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
        <span className="text-[11px] font-medium text-slate-500 dark:text-slate-400">{label}</span>
      )}
      <input
        type={type}
        placeholder={placeholder}
        value={value}
        onChange={(e) => {
          onValueChange?.(e.target.value)
          onChange?.(e)
        }}
        className={`h-control-md rounded-md border border-slate-300 bg-transparent px-2 text-sm text-slate-900 transition-colors placeholder:text-slate-400 focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none dark:border-slate-600 dark:text-slate-100 ${
          mono ? 'font-mono tabular-nums' : ''
        }`}
        {...rest}
      />
      {hint && <span className="text-[10px] text-slate-400">{hint}</span>}
    </label>
  )
}
