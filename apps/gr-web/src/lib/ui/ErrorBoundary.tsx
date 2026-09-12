// 렌더 오류 격리 — 한 화면·패널이 예외를 던져도 앱 전체가 흰 화면이 되지 않게 그 자리만 오류 카드로 바꾼다.
// `resetKey` 가 바뀌면(다른 Task 를 열거나 탭을 옮기면) 다시 그려 본다.
import { Component, type ErrorInfo, type ReactNode } from 'react'

export interface ErrorBoundaryProps {
  children: ReactNode
  /** 이 값이 바뀌면 오류 상태를 풀고 다시 렌더한다. */
  resetKey?: unknown
  /** 오류 카드 제목 — 어디서 났는지. */
  label?: string
}

interface State {
  error: Error | null
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(`[${this.props.label ?? 'ui'}] 렌더 오류`, error, info.componentStack)
  }

  componentDidUpdate(prev: ErrorBoundaryProps): void {
    if (this.state.error && !Object.is(prev.resetKey, this.props.resetKey))
      this.setState({ error: null })
  }

  render(): ReactNode {
    const { error } = this.state
    if (!error) return this.props.children
    return (
      <div
        role="alert"
        className="m-3 space-y-2 rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-800 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-200"
        data-testid="error-boundary"
      >
        <p className="m-0 text-sm font-semibold">
          {this.props.label ?? '화면'}을 그리지 못했습니다
        </p>
        <p className="m-0 font-mono break-all">{error.message}</p>
        <button
          type="button"
          className="h-7 rounded-md border border-red-300 px-2 hover:bg-red-100 dark:border-red-500/40 dark:hover:bg-red-500/20"
          onClick={() => this.setState({ error: null })}
        >
          다시 시도
        </button>
      </div>
    )
  }
}
