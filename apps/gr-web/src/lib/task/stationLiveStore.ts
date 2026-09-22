// GRM 스테이션 실시간 상태 — `/api/stations/live/stream`(SSE). 백엔드가 **값이 바뀐 때만** 한 번 보내므로
// 폴링도, 같은 값으로 다시 그리는 일도 없다(`issue::station_live::publish`). 첫 메시지는 지금 값.
// 참조 세기 — 맵이 여러 번 잡아도 EventSource 는 하나. 표현 규칙은 `stationLiveModel`.
import { SseFeed } from '../sse'
import type { Subscribable } from '../store'
import type { StationLive } from './types'

const feed = new SseFeed<StationLive[]>('/api/stations/live/stream', 'stations')

let lastData: StationLive[] | null = null
let lastMap: ReadonlyMap<number, StationLive> = new Map()

export const stationLive: Subscribable & {
  readonly map: ReadonlyMap<number, StationLive>
  start(): () => void
} = {
  subscribe: feed.subscribe,
  getSnapshot: feed.getSnapshot,
  /** Id → 상태. 메시지가 바뀔 때만 새 Map 을 만든다. */
  get map() {
    if (feed.data !== lastData) {
      lastData = feed.data
      lastMap = new Map((lastData ?? []).map((r) => [r.id, r]))
    }
    return lastMap
  },
  start: () => feed.start(),
}
