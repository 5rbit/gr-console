// 대화상자의 순수 판정 — 폭 단계와 **저장 안 한 편집을 조용히 버리지 않는** 규칙.
//
// 컴포넌트 밖에 두는 이유는 `table.ts`와 같다: 규칙을 DOM 없이 테스트로 못 박는다. 팝업으로 입력을
// 옮기면 값이 화면에서 사라지므로, "무엇이 닫기를 요청했나"에 따라 버림을 되묻는 자리가 필요하다.
// 파일 이름이 `*Model.ts`인 이유는 NTFS 대소문자 때문이다(`Dialog.tsx` 옆의 `dialog.ts`는 같은 이름이다).

/** 닫기를 요청한 경로. 무엇이 눌렸는지에 따라 되묻는 기준이 다르다. */
export type CloseIntent = 'esc' | 'backdrop' | 'cancel' | 'submit'

/**
 * 편집을 버리기 전에 한 번 되물어야 하나.
 *
 * `submit`은 값이 살아서 나가므로 절대 되묻지 않는다(되물으면 저장이 두 번 누르는 일이 된다).
 * 나머지 셋은 편집이 남아 있을 때만 되묻는다 — 깨끗한 폼에서 Escape가 확인창을 띄우면
 * "닫기가 안 되는 창"으로 배운다.
 */
export function shouldConfirmDiscard(dirty: boolean, intent: CloseIntent): boolean {
  if (intent === 'submit') return false
  return dirty
}

/** 대화상자 폭 단계 — 넷뿐이다(임의 폭을 열면 화면마다 다른 폭이 선다). */
export type DialogSize = 'sm' | 'md' | 'lg' | 'xl'

/** 폭 단계 → 최대 폭 클래스. 값은 킷의 컨테이너 스케일에서 고른다. */
export const DIALOG_WIDTH: Record<DialogSize, string> = {
  sm: 'max-w-md',
  md: 'max-w-2xl',
  lg: 'max-w-4xl',
  xl: 'max-w-6xl',
}
