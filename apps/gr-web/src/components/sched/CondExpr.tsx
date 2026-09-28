// 스테이션 신호 조건식 — 항목마다 충족(초록) · 미충족(빨강), `&` 로 잇는다. 예: CVOK & Req & ItemExist & CVNO 0 & Comp 0
import { Fragment } from 'react'
import type { Cond } from '../../lib/sched'
import { condText, condTokens } from '../../lib/sched/condExprModel'
import { cn } from '../../lib/utils'

export default function CondExpr({
  expr,
  className,
}: {
  expr: readonly Cond[] | undefined
  className?: string
}) {
  const tokens = condTokens(expr)
  if (!tokens.length) return null
  return (
    <span
      className={cn('font-mono text-2xs whitespace-nowrap', className)}
      title={condText(expr)}
      data-testid="cond-expr"
    >
      {tokens.map((t, i) => (
        <Fragment key={t.text}>
          {i > 0 ? <span className="text-content-faint"> & </span> : null}
          <span className={t.ok ? 'text-ok-fg' : 'text-danger-text'}>{t.text}</span>
        </Fragment>
      ))}
    </span>
  )
}
