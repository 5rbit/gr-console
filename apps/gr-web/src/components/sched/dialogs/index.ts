// 스케줄러 대화상자 모음 — 모두 "열 때만 그린다"(부모가 `{open ? <X … /> : null}`), 닫기는 `onClose`.

/** `{ rule, takenIds?, onClose, onSave(rule) }` — 사용자 규칙 편집. 저장은 호출자(보통 `schedApi.save({...cfg, rules})`). */
export { RuleDialog, PlaceEditor } from './RuleDialog'
/** `{ cfg, onClose, onSave(cfg) }` — 대상 · 품목 · 로봇 가중치. 받은 설정 타입 그대로 돌려준다(정책 포함 설정도 됨). */
export { WeightsDialog } from './WeightsDialog'
/** `{ onClose }` — 제출 · 스케줄링 파라미터(ParamsPanel, 바로 적용). */
export { ParamsDialog } from './ParamsDialog'
/** `{ config: GenConfig & {policy}, title?, onClose }` — 연 순간의 설정으로 `schedApi.simulate` 한 번(다시 판정 버튼). 정책 없는 설정은 `withPolicy(cfg)` 로. */
export { SimulateDialog } from './SimulateDialog'
/** `{ title, value, source?, onClose, onApply(pick) }` — CellPick 하나 편집. `CellPickFields` 는 폼 안에 넣는 입력 묶음. */
export { CellPickDialog, CellPickFields } from './CellPickFields'
/** `{ rows, derived, empty?, testid? }` — 후보 표(결정 탭 · 시뮬레이션 공용). `OriginChip { origin }`. */
export { CandidateTable, OriginChip } from './CandidateTable'
