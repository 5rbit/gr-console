// 로딩 스켈레톤 — 초기 fetch·폴링 첫 tick 전 자리표시(내부 도구의 '무표시 로딩' 갭 봉합).
export interface SkeletonProps {
  className?: string
}

export function Skeleton({ className = 'h-4 w-full' }: SkeletonProps) {
  return <div className={`animate-pulse rounded bg-slate-200 dark:bg-slate-700 ${className}`} />
}
