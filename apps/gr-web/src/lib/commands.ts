// 명령 레지스트리와 검색 — 명령 팔레트(`Ctrl/⌘+K`)가 읽는 한 벌.
//
// VSCode의 관용구를 그대로 가져온다: **화면에 있는 조작은 전부 명령으로도 있다.** 이유는 발견성이다.
// 도킹·프리셋·밀도·패널 넷을 다 메뉴에 늘어놓으면 메뉴가 창고가 되고, 메뉴에서 빼면 아무도 못 찾는다.
// 팔레트가 있으면 메뉴는 자주 쓰는 것만 들고, 나머지는 이름으로 부른다.
//
// 여기 있는 것은 **모양과 검색**뿐이다 — 실제 명령 목록은 셸(`components/CommandPalette`)이 그때의
// 레이아웃·프리셋·테마를 보고 만든다. 목록을 모듈 상태로 들면 스토어가 바뀔 때 갱신할 사람이 없다.

/** 명령 1건. */
export interface Command {
  id: string
  /** 팔레트·메뉴에 보이는 이름. 조작이라 동사로 끝난다("…열기" · "…초기화"). */
  label: string
  /** 분류 — 팔레트에서 `보기: 상태 열기`처럼 접두로 붙고, 같은 그룹끼리 묶여 보인다. */
  group: string
  /** 단축키 표기(오른쪽 정렬). */
  hint?: string
  /** 검색 별칭 — 라벨에 없는 말로도 찾히게(`plc` · `dock` · `레이아웃`). */
  keywords?: string
  /** 켜짐 표시(토글 명령) — 체크로 그린다. */
  checked?: boolean
  /** 비활성 사유. 있으면 눌리지 않고 **그 이유를 말한다**(회색으로 침묵하지 않는다). */
  disabled?: string
  /** 바로 실행되는 명령. `prompt`가 있으면 없어도 된다. */
  run?: () => void
  /**
   * 값을 받아야 하는 명령(레이아웃 저장 같은 것) — 팔레트가 **두 번째 걸음**으로 입력을 받는다.
   * `window.prompt`를 쓰지 않는 이유: 브라우저 모달은 셸의 포커스 트랩 밖이고, 내용을 설명할 자리가
   * 없어서 "무엇을 적으라는 것인가"가 안 남는다.
   */
  prompt?: {
    label: string
    placeholder?: string
    initial?: string
    run: (text: string) => void
  }
}

/** 검색 결과 1건 — 점수는 정렬에만 쓴다. */
export interface CommandHit {
  command: Command
  score: number
}

/** 검색용으로 접은 문자열 — 공백·대소문자를 지운다. 한글은 자모 분해 없이 글자 단위로 본다. */
function fold(s: string): string {
  return s.toLowerCase().replace(/\s+/g, '')
}

/**
 * 부분 수열 판정 — 질의의 글자가 순서대로 나타나면 맞은 것으로 본다(`tsk` → `task`).
 * 점수는 **간격의 합**이 작을수록 높다: 붙어 나온 것이 흩어진 것보다 앞선다.
 */
function subsequence(hay: string, needle: string): number | null {
  let at = 0
  let gaps = 0
  for (const ch of needle) {
    const i = hay.indexOf(ch, at)
    if (i < 0) return null
    gaps += i - at
    at = i + 1
  }
  return gaps
}

/**
 * 명령을 질의로 걸러 정렬한다 — 순수 함수(팔레트 컴포넌트가 매 입력마다 부른다).
 *
 * 순서 규칙:
 * ① 라벨에 그대로 든 것이 먼저고, 앞쪽에 든 것이 더 앞이다(`task`는 `Task 관리`를 먼저 준다).
 * ② 그다음이 그룹·별칭에 든 것.
 * ③ 마지막이 부분 수열(`tm` → `Task 관리`).
 * 질의가 비면 **레지스트리 순서 그대로** 돌려준다 — 팔레트를 그냥 열었을 때는 그룹 순서가 목차다.
 */
export function matchCommands(list: readonly Command[], query: string): CommandHit[] {
  const q = fold(query)
  if (!q) return list.map((command) => ({ command, score: 0 }))

  const hits: (CommandHit & { seq: number })[] = []
  list.forEach((command, seq) => {
    const label = fold(command.label)
    const extra = fold(`${command.group} ${command.keywords ?? ''}`)

    const inLabel = label.indexOf(q)
    if (inLabel >= 0) {
      hits.push({ command, score: 3000 - inLabel, seq })
      return
    }
    const inExtra = extra.indexOf(q)
    if (inExtra >= 0) {
      hits.push({ command, score: 2000 - inExtra, seq })
      return
    }
    const gaps = subsequence(label, q) ?? subsequence(extra, q)
    if (gaps !== null) hits.push({ command, score: 1000 - Math.min(gaps, 900), seq })
  })

  // 점수 같으면 레지스트리 순서 — 같은 질의에 목록이 흔들리면 두 번째 타자는 못 고른다.
  hits.sort((a, b) => b.score - a.score || a.seq - b.seq)
  return hits.map(({ command, score }) => ({ command, score }))
}

/** 그룹별로 묶는다 — 팔레트가 머리줄을 그리기 위해. 그룹의 등장 순서를 지킨다. */
export function groupHits(hits: readonly CommandHit[]): { group: string; hits: CommandHit[] }[] {
  const out: { group: string; hits: CommandHit[] }[] = []
  for (const h of hits) {
    const last = out.at(-1)
    if (last && last.group === h.command.group) last.hits.push(h)
    else out.push({ group: h.command.group, hits: [h] })
  }
  return out
}
