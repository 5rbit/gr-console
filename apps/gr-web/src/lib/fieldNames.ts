// 데이터 필드 표시 이름 — 콘솔 모델 키(snake_case) → PLC/데이터 멤버 이름(영문 그대로).
// 규칙: 필드·데이터 이름은 번역하지 않는다. 설명 문구(버튼·안내·경고)만 한글이다.
// 예외: PLC `Lenth` 오타는 화면에 `Length` 로 보인다.

export const FIELD = {
  id: 'Id',
  use: 'Use',
  blend_use: 'BlendUse',
  section: 'Section',
  row: 'Row',
  col: 'Col',
  length: 'Length',
  width: 'Width',
  position: 'Position',
  conv_no: 'ConvNo',
  task_type: 'TaskType',
  rotate_type: 'RotateType',
  group: 'Group',
  group_index: 'GroupIndex',
  connection_prev: 'ConnectionPrev',
  connection_next: 'ConnectionNext',
  io_block_no: 'IOBlockNo',
  io_link_master_module: 'IOLinkMasterModule',
  io_link_master_port_l: 'IOLinkMasterPort_L',
  io_link_master_port_r: 'IOLinkMasterPort_R',
  detection_factor: 'DetectionFactor',
  allow_range: 'AllowRange',
  l_sensor_offset: 'L_SensorOffset',
  r_sensor_offset: 'R_SensorOffset',
  cell_id: 'CellId',
  count: 'Count',
  item_code: 'ItemCode',
  stack_height: 'StackHeight',
  updated_at: 'UpdatedAt',
  state: 'State',
  code: 'Code',
  name: 'Name',
  inner_diameter: 'InnerDiameter',
  outer_diameter: 'OuterDiameter',
  height: 'Height',
  lower_bead_height: 'LowerBidHeight',
  upper_bead_height: 'UpperBidHeight',
  deflection_factor: 'DeflectionFactor',
  note: 'Note',
} as const

export type FieldKey = keyof typeof FIELD

/** 대상 종류 표시 이름. */
export const TARGET_NAME = { cell: 'Cell', station: 'Station' } as const

/** `Use=false` 표시 — 미사용 셀·스테이션 요약에 붙인다. */
export const USE_FALSE = 'Use=false'
