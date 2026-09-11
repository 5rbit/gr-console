// 거부 코드 표 — PLC `STAT.RES.Data[1..2]`의 검증 코드(LGR_Const_Interface_Validation)와 `Data[0]` 비트를
// 사람의 말로. 백엔드 `ack.reason`은 영문 상수명이라 화면에는 이 표의 한글을 앞세우고 원문을 뒤에 둔다.
import type { TaskAck } from '../types'

/** 검증 코드 → 한글 설명. 1은 유효. */
export const REJECT_TEXT: Record<number, string> = {
  1: '유효한 작업',
  100: '프로토콜 번호 불일치',
  101: '명령 코드 불일치',
  102: '발신지(SRC) 불일치',
  103: '수신지(DST) 불일치',
  104: '중복 명령(같은 헤더가 30초 안에 다시 옴)',
  110: '작업 버퍼 가득 참',
  200: 'WorkId 무효',
  201: 'TaskId 무효',
  202: '작업 셀 무효',
  203: '작업 종류 무효',
  301: 'X 위치 범위 밖',
  302: 'Y 위치 범위 밖',
  303: 'Z 위치 범위 밖',
  304: 'G(그리퍼) 위치 범위 밖',
  305: '셀 Z 위치 무효',
  310: '블렌드 상승 거리 무효',
  311: '블렌드 하강 거리 무효',
  320: '드래그 아웃 사용 조건 무효',
  321: '드래그 아웃 높이 무효',
  322: '드래그 아웃 거리 무효',
  323: '드래그 아웃 방향 무효',
  330: '드래그 인 사용 조건 무효',
  331: '드래그 인 높이 무효',
  332: '드래그 인 거리 무효',
  333: '드래그 인 방향 무효',
  340: '그립 델타 무효',
  401: '셀 Id가 비어 있음',
  409: '영역 미정의',
  410: '스테이션 구역 무효',
  411: '스테이션 X 범위 밖',
  412: '스테이션 Y 범위 밖',
  413: '스테이션 Z 범위 밖',
  418: '스테이션 미사용(Use=false)',
  419: '스테이션이 PLC에 등록되지 않음',
  420: '셀 구역 무효',
  421: '셀 X 범위 밖',
  422: '셀 Y 범위 밖',
  423: '셀 Z 범위 밖',
  428: '셀 미사용(Use=false)',
  429: '셀이 PLC에 등록되지 않음',
  501: '품목 코드 무효',
  502: '품목 수량 무효',
  900: '정의되지 않은 오류',
}

/** `Data[0]` 비트 → 이름(X0 로봇번호, X1 중복, X2 작업 무효, X3 버퍼 풀, X7 오프라인). */
export const REJECT_BITS: readonly { bit: number; text: string }[] = [
  { bit: 0x01, text: '로봇 번호 불일치' },
  { bit: 0x02, text: '중복 명령' },
  { bit: 0x04, text: '작업 데이터 무효' },
  { bit: 0x08, text: '버퍼 가득 참' },
  { bit: 0x80, text: '로봇 오프라인' },
]

/** 검증 코드의 한글(모르는 코드는 숫자 그대로 — 이름을 지어내지 않는다). */
export function rejectText(code: number): string {
  return REJECT_TEXT[code] ?? `알 수 없는 코드 ${code}`
}

/** 켜진 거부 비트의 이름들. */
export function rejectBitsText(bits: number): string[] {
  return REJECT_BITS.filter((b) => (bits & b.bit) !== 0).map((b) => b.text)
}

/** Ack 한 건의 표시 문장 — 수락이면 짧게, 거부면 코드·비트·원문을 모두. */
export function ackText(ack: TaskAck): string {
  if (ack.accepted) return `수락됨 (코드 ${ack.code})`
  const bits = rejectBitsText(ack.reject_bits)
  const head = `거부됨 — ${rejectText(ack.code)} (코드 ${ack.code})`
  return bits.length ? `${head} · ${bits.join(', ')}` : head
}
