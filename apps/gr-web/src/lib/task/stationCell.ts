// 스테이션을 재고 자리(Cell 모양)로 — 스테이션도 재고(컨베이어 화물 코드)를 가진다. 재고 표·재고 편집
// 창은 Cell 을 받으므로 스테이션의 Info 에 출처·수정·시각만 붙여 넘긴다. 코드 이동은 백엔드
// `stock::conveyor`, 저장은 같은 `PUT /api/stock/{id}`.
import type { Cell, Station } from '../types'

export function stationAsCell(s: Station): Cell {
  return { ...s.info, source: s.source, dirty: s.dirty, updated_at: s.updated_at }
}
