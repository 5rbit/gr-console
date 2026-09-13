// 알람 코드 → 짧은 설명 (콘솔 툴팁용). 원문은 HMI 알람 텍스트이며, 여기에는 그리퍼 측정·센서 관련 코드만 둔다.

export const ALARM_LABEL: Record<number, string> = {
  4025: 'G 그리퍼 타임아웃 (허용 범위 밖 멈춤)',
  6016: 'PICK 아이템 미검출 (허용 범위 안 멈춤인데 미검출 포함)',
  4201: 'GID L 높이 편차 (설치·교정 확인)',
  4202: 'GID F 높이 편차 (설치·교정 확인)',
  4203: 'GID R 높이 편차 (설치·교정 확인)',
  4204: 'GID B 높이 편차 (설치·교정 확인)',
  4205: 'GID L 무응답 (렌즈·케이블·IO-Link)',
  4206: 'GID F 무응답 (렌즈·케이블·IO-Link)',
  4207: 'GID R 무응답 (렌즈·케이블·IO-Link)',
  4208: 'GID B 무응답 (렌즈·케이블·IO-Link)',
  4209: 'GID L 신호 불안정',
  4210: 'GID F 신호 불안정',
  4211: 'GID R 신호 불안정',
  4212: 'GID B 신호 불안정',
  4213: '레이저 Z 오프셋 교정 실패',
}

export function alarmLabel(code: number): string | undefined {
  return ALARM_LABEL[code]
}
