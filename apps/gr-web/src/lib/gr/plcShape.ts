// PLC 구조 그대로 보기 — 콘솔 모델(snake_case)을 PLC UDT 멤버 이름·순서로 펼친다.
//   LGR_Cell_Info : Use, BlendUse, Id, Section, Row, Col, Lenth, Width, Position[X..Z]
//   LGR_Stock_Item: Code, Count, InnerDiameter, OuterDiameter, LowerBidHeight, UpperBidHeight, Height, DeflectionFactor
//   LGR_Task_Data : WorkId, TaskId, TaskType, Position[X..G], Item, Cell, 튜닝값…, 플래그…
// 셀과 스테이션 Info 는 같은 LGR_Cell_Info 라 한 함수로 다룬다.
import type { CellUpsert, Item, PlcCell, PlcItem, PlcTask } from '../types'
import { TASK_TYPE_CODE } from './const'

export interface PlcRow {
  /** PLC 멤버 경로(`Cell.Position[Z]` 처럼). */
  member: string
  type: string
  value: string | number | boolean
  comment?: string
}

/** 콘솔 Cell/Station.info → PLC LGR_Cell_Info (PascalCase, `Lenth` 오타 포함). */
export function toPlcCell(c: Omit<CellUpsert, never>): PlcCell {
  return {
    Use: c.use,
    BlendUse: c.blend_use,
    Id: c.id,
    Section: c.section,
    Row: c.row,
    Col: c.col,
    Lenth: c.length,
    Width: c.width,
    Position: [...c.position],
  }
}

/** PLC LGR_Cell_Info → 콘솔 CellUpsert. */
export function fromPlcCell(p: PlcCell): CellUpsert {
  return {
    id: p.Id,
    use: p.Use,
    blend_use: p.BlendUse,
    section: p.Section,
    row: p.Row,
    col: p.Col,
    length: p.Lenth,
    width: p.Width,
    position: [p.Position[0] ?? 0, p.Position[1] ?? 0, p.Position[2] ?? 0],
  }
}

/** 콘솔 Item → PLC LGR_Stock_Item. */
export function toPlcItem(i: Item, count = 1): PlcItem {
  return {
    Code: i.code,
    Count: count,
    InnerDiameter: i.inner_diameter,
    OuterDiameter: i.outer_diameter,
    LowerBidHeight: i.lower_bead_height,
    UpperBidHeight: i.upper_bead_height,
    Height: i.height,
    DeflectionFactor: i.deflection_factor,
  }
}

export function cellInfoRows(p: PlcCell, prefix = ''): PlcRow[] {
  const P = (m: string) => (prefix ? `${prefix}.${m}` : m)
  return [
    { member: P('Use'), type: 'Bool', value: p.Use },
    { member: P('BlendUse'), type: 'Bool', value: p.BlendUse },
    { member: P('Id'), type: 'UInt', value: p.Id },
    { member: P('Section'), type: 'UInt', value: p.Section },
    { member: P('Row'), type: 'UInt', value: p.Row },
    { member: P('Col'), type: 'UInt', value: p.Col },
    { member: P('Lenth'), type: 'Real', value: p.Lenth, comment: 'PLC 철자 그대로(length)' },
    { member: P('Width'), type: 'Real', value: p.Width },
    { member: P('Position[X]'), type: 'Real', value: p.Position[0] ?? 0 },
    { member: P('Position[Y]'), type: 'Real', value: p.Position[1] ?? 0 },
    { member: P('Position[Z]'), type: 'Real', value: p.Position[2] ?? 0 },
  ]
}

export function stockItemRows(i: PlcItem, prefix = 'Item'): PlcRow[] {
  const P = (m: string) => `${prefix}.${m}`
  return [
    { member: P('Code'), type: 'UDInt', value: i.Code },
    { member: P('Count'), type: 'USInt', value: i.Count },
    { member: P('InnerDiameter'), type: 'Real', value: i.InnerDiameter },
    { member: P('OuterDiameter'), type: 'Real', value: i.OuterDiameter },
    { member: P('LowerBidHeight'), type: 'Real', value: i.LowerBidHeight },
    { member: P('UpperBidHeight'), type: 'Real', value: i.UpperBidHeight },
    { member: P('Height'), type: 'Real', value: i.Height },
    { member: P('DeflectionFactor'), type: 'Real', value: i.DeflectionFactor },
  ]
}

const TYPE_NAME: Record<number, string> = Object.fromEntries(
  Object.entries(TASK_TYPE_CODE).map(([k, v]) => [v, k]),
)

/** LGR_Task_Data 전체를 UDT 선언 순서로. */
export function taskDataRows(t: PlcTask): PlcRow[] {
  const u = (m: keyof PlcTask, type: string, comment?: string): PlcRow => ({
    member: m,
    type,
    value: t[m] as string | number | boolean,
    comment,
  })
  return [
    { member: 'WorkId', type: 'UDInt', value: t.WorkId },
    { member: 'TaskId', type: 'UDInt', value: t.TaskId },
    {
      member: 'TaskType',
      type: 'Byte',
      value: `16#${t.TaskType.toString(16).toUpperCase().padStart(2, '0')}`,
      comment: TYPE_NAME[t.TaskType] ?? '',
    },
    { member: 'Position[X]', type: 'Real', value: t.Position[0] ?? 0 },
    { member: 'Position[Y]', type: 'Real', value: t.Position[1] ?? 0 },
    {
      member: 'Position[Z]',
      type: 'Real',
      value: t.Position[2] ?? 0,
      comment: '재고 기준 적재 높이',
    },
    { member: 'Position[G]', type: 'Real', value: t.Position[3] ?? 0, comment: '그리퍼 열림' },
    ...stockItemRows(t.Item),
    ...cellInfoRows(t.Cell, 'Cell'),
    u('BlendUpDistance', 'UInt'),
    u('BlendDownDistance', 'UInt'),
    u('DragOutHeight', 'UInt'),
    u('DragOutDist', 'UInt'),
    u('DragOutDir', 'Byte'),
    u('DragInHeight', 'UInt'),
    u('DragInDist', 'UInt'),
    u('DragInDir', 'Byte'),
    u('LiftUpCreepDistance', 'UInt'),
    u('LiftDownCreepDistance', 'UInt'),
    u('LiftUpHeight', 'UInt'),
    u('PreGripDelta', 'UInt'),
    u('GripBackDelta', 'UInt'),
    u('GripHeight', 'UInt'),
    u('UseDragOut', 'Bool'),
    u('UseDragIn', 'Bool'),
    u('LiftUpAfterComplete', 'Bool', '작업 완료후 Z축 상승 여부'),
    u('LiftUpPartial', 'Bool'),
    u('MeasureFloor', 'Bool', '바닥 높이 측정 여부 (Move 명령 전용)'),
    u('MeasureItem', 'Bool', '아이템 측정 여부 (비드 높이, 중심 등)'),
    u('MeasureSku', 'Bool', '재고 및 높이 측정 여부'),
    u('AdjustCenter', 'Bool', '아이템 중심 보정 여부 (Pick)'),
    u('FindStationItem', 'Bool', 'Station 아이템 중심 탐색 여부'),
    u('Avoid', 'Bool', '회피 동작 여부 (XY이동, Z= 9999)'),
    u('Outbound', 'Bool'),
  ]
}
