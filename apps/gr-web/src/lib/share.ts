// 같은 조회를 하나로 합친다 — 소비자는 각자 부르고, 나가는 요청은 하나다.

/** 합치는 창(ms). 가장 빠른 폴(500ms)보다 짧게 — 오래된 값을 나눠 갖지 않도록. */
export const SHARE_MS = 200

const inflight = new Map<string, { p: Promise<unknown>; at: number }>()

/** 같은 `key`의 조회를 [`SHARE_MS`] 창 안에서 하나로 합친다. */
export function shareGet<T>(key: string, run: () => Promise<T>): Promise<T> {
  const now = Date.now()
  const hit = inflight.get(key)
  if (hit && now - hit.at < SHARE_MS) return hit.p as Promise<T>
  const p = run()
  inflight.set(key, { p: p as Promise<unknown>, at: now })
  const drop = () => {
    const cur = inflight.get(key)
    if (cur && cur.p === p && Date.now() - cur.at >= SHARE_MS) inflight.delete(key)
  }
  // 실패는 창을 기다리지 않고 버린다 — 오류를 나눠 주면 재시도가 그만큼 늦다.
  p.catch(() => {
    if (inflight.get(key)?.p === p) inflight.delete(key)
  })
  setTimeout(drop, SHARE_MS)
  return p
}

/** 합치기 창을 버린다 — 명령 직후 조회가 명령 이전 값을 보지 않게. */
export function invalidateShared(): void {
  inflight.clear()
}
