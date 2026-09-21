// 화면에서 걷어 낸 설명문이 사는 한 자리 — 같은 문장을 두 화면에 두 벌로 두면 한쪽만 고쳐진다.
//
// 이 문장들은 화면 본문에 문단으로 서지 않는다. `HelpTip`(`?`)의 말풍선 또는 컨트롤 `title` 로만
// 나간다(`docs/DESIGN.md` 4절 ② — 요약은 라벨+값 짝이고, 설명은 손이 찾아올 때만 뜬다).

/** DragDir 격자·DirPicker 공통 — 코드의 뜻과 PLC 바이트 규칙. */
export const DRAG_DIR_HELP =
  '칸 = 드래그 변위가 가리키는 쪽(지금 보기 방향). PLC DragDelta 와 같은 코드이고, 아래 글자는 DragType 바이트 = 1 << (DragDir − 1).'

/** 스테이션 프로파일이 무엇을 담는가. */
export const PROFILE_HELP =
  '스테이션을 고르면 Flow · Gap · Rotation · Mirror · PalletSize 를 그 스테이션 프로파일로 저장합니다.'

/** Enabled 스위치가 작업 명령에 무엇을 바꾸는가. */
export const ENABLED_HELP =
  'Enabled 인 스테이션만 작업 명령이 팔렛 슬롯·드래그를 쓰고 트래킹 보정을 건너뜁니다.'

/** 패턴 편집 그림의 마우스·키보드 조작. */
export const EDIT_CANVAS_HELP =
  '원을 끌어 옮기고(스냅 적용), 두 번 누르거나 D 키로 DragDir 을 순환합니다. 방향키는 스냅 단위로 옮깁니다. 점선 원은 생성 결과입니다.'

/** 슬롯 표의 행 끌기. */
export const SLOT_ROW_HELP = '행을 끌어 놓으면 Seq 가 바뀝니다.'

/** JSON 가져오기의 형식·병합 규칙. */
export const JSON_IMPORT_HELP =
  'spec_r4.json 과 같은 형식입니다. 흐름 id + Pattern 번호로 합치고, 문서에 없는 흐름·패턴은 그대로 둡니다.'
