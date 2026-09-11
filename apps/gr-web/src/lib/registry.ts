// 레지스트리 목록 훅 — 품목·셀·스테이션처럼 "한 번 받아 두고 편집 뒤 다시 받는" 목록의 공통 모양.
//
// `loader`는 마운트 때 한 번 돈다. 인라인 화살표를 넘겨도 렌더마다 다시 받지 않도록 ref에 든다
// (`reload()`가 최신 loader를 쓴다).
import { useCallback, useEffect, useRef, useState } from 'react'

export interface Registry<T> {
  items: T[]
  loading: boolean
  error: string | null
  /** 다시 받는다(편집·import 뒤). */
  reload: () => Promise<void>
  /** 서버 왕복 없이 목록을 바꾼다(낙관적 갱신). */
  setItems: (next: T[]) => void
}

export function useRegistry<T>(loader: () => Promise<T[]>): Registry<T> {
  const loaderRef = useRef(loader)
  loaderRef.current = loader
  const [items, setItems] = useState<T[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    setLoading(true)
    try {
      setItems(await loaderRef.current())
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  return { items, loading, error, reload, setItems }
}
