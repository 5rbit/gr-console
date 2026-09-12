// 위치 미리보기 — 초안이 바뀔 때마다(300ms 디바운스) `POST /api/issue/compose`로 백엔드가 계산한
// X/Y/Z/G와 확정 파라미터를 FieldList로 보인다. 경고는 목록으로, 오류(미등록 품목 등)는 붉게.
import { useEffect, useRef, useState } from 'react'
import { taskApi } from '../../lib/task/api'
import { previewFields } from '../../lib/task/compose'
import type { ComposePreview } from '../../lib/task/types'
import { FieldList } from '../../lib/ui/FieldList'
import type { FieldItem } from '../../lib/ui/fieldListModel'
import type { TaskRequest } from '../../lib/types'

const DEBOUNCE_MS = 300

export interface PositionPreviewProps {
  /** 미리보기할 요청 — `null`이면(초안이 아직 성립하지 않으면) 요청하지 않는다. */
  request: TaskRequest | null
  /** 미리보기 결과를 부모에게도 준다(제출 확인 다이얼로그가 같은 값을 보인다). */
  onPreview?: (p: ComposePreview | null) => void
}

export function PositionPreview({ request, onPreview }: PositionPreviewProps) {
  const [preview, setPreview] = useState<ComposePreview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const seq = useRef(0)
  const key = request ? JSON.stringify(request) : ''

  useEffect(() => {
    if (!key) {
      setPreview(null)
      setError(null)
      onPreview?.(null)
      return
    }
    const mine = ++seq.current
    const t = setTimeout(() => {
      setLoading(true)
      taskApi
        .compose(JSON.parse(key) as TaskRequest)
        .then((p) => {
          if (mine !== seq.current) return
          setPreview(p)
          setError(null)
          onPreview?.(p)
        })
        .catch((e: unknown) => {
          if (mine !== seq.current) return
          setPreview(null)
          setError(e instanceof Error ? e.message : String(e))
          onPreview?.(null)
        })
        .finally(() => {
          if (mine === seq.current) setLoading(false)
        })
    }, DEBOUNCE_MS)
    return () => clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- onPreview는 부모의 setState라 안정적이다
  }, [key])

  const items: FieldItem[] = previewFields(preview).map((f) => ({
    label: f.label,
    value: f.value,
    loading: loading && !preview,
  }))

  return (
    <div className="flex flex-col gap-2" data-testid="position-preview">
      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-700 dark:border-red-500/40 dark:bg-red-500/10 dark:text-red-300">
          미리보기 실패 — {error}
        </p>
      ) : null}
      {!request ? (
        <p className="text-xs text-slate-400">
          대상과 품목을 고르면 PLC로 갈 위치를 미리 계산합니다.
        </p>
      ) : items.length === 0 && loading ? (
        <p className="text-xs text-slate-400">계산 중…</p>
      ) : (
        <FieldList items={items} columns={2} dense labelWidth={96} />
      )}
      {preview && preview.warnings.length > 0 ? (
        <ul
          className="list-disc pl-4 text-xs text-amber-700 dark:text-amber-300"
          data-testid="preview-warnings"
        >
          {preview.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
