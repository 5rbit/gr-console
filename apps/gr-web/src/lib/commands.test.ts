import { describe, expect, it } from 'vitest'
import { groupHits, matchCommands, type Command } from './commands'

function cmd(id: string, label: string, group: string, keywords?: string): Command {
  return { id, label, group, keywords, run: () => {} }
}

const LIST: Command[] = [
  cmd('open.task', '작업 명령 열기', '보기', 'issue command'),
  cmd('open.taskmgr', 'Task 관리 열기', '보기', 'manager 목록'),
  cmd('open.measure', '측정 모니터 열기', '보기', 'measure'),
  cmd('preset.standard', '기본 배치', '레이아웃', 'preset layout'),
  cmd('layout.reset', '레이아웃 초기화', '레이아웃', 'reset'),
  cmd('theme.toggle', '테마 전환', '설정', 'dark light'),
]

const ids = (q: string): string[] => matchCommands(LIST, q).map((h) => h.command.id)

describe('matchCommands', () => {
  it('질의가 비면 레지스트리 순서 그대로 — 그냥 열었을 때는 그룹 순서가 목차다', () => {
    expect(ids('')).toEqual(LIST.map((c) => c.id))
    expect(ids('   ')).toEqual(LIST.map((c) => c.id))
  })

  it('라벨에 그대로 든 것이 별칭으로만 맞은 것보다 앞선다', () => {
    expect(ids('task')[0]).toBe('open.taskmgr')
  })

  it('별칭으로도 찾힌다 — 라벨에 없는 말로 부를 수 있게', () => {
    expect(ids('dark')).toEqual(['theme.toggle'])
    expect(ids('preset')).toEqual(['preset.standard'])
  })

  it('그룹 이름으로 그룹 전체를 부른다', () => {
    expect(ids('레이아웃')).toEqual(['layout.reset', 'preset.standard'])
  })

  it('라벨 앞쪽에 든 것이 뒤쪽에 든 것보다 앞이다', () => {
    const l = [cmd('a', '레이아웃 초기화', 'x'), cmd('b', '초기화', 'x')]
    expect(matchCommands(l, '초기화').map((h) => h.command.id)).toEqual(['b', 'a'])
  })

  it('부분 수열도 맞는다(붙어 나온 것이 먼저)', () => {
    // '측정' 이 라벨에 그대로 들었으므로 먼저, '측모'는 부분 수열로 같은 항목을 집는다.
    expect(ids('측모')).toEqual(['open.measure'])
  })

  it('대소문자·공백을 무시한다', () => {
    expect(ids('TASK 관리')).toContain('open.taskmgr')
    expect(ids('MANAGER')).toContain('open.taskmgr')
  })

  it('라벨과 별칭을 가로질러 맞추지는 않는다 — 걸쳐 읽으면 아무거나 다 맞는다', () => {
    // `task`(라벨) + `manager`(별칭)를 이어 붙인 질의는 맞지 않는다.
    expect(ids('taskmanager')).toEqual([])
  })

  it('안 맞는 질의는 빈 목록', () => {
    expect(ids('zzzz')).toEqual([])
  })

  it('점수가 같으면 레지스트리 순서를 지킨다 — 목록이 흔들리면 두 번째를 고를 수 없다', () => {
    const l = [cmd('a', '가 열기', 'g'), cmd('b', '가 열기', 'g')]
    expect(matchCommands(l, '열기').map((h) => h.command.id)).toEqual(['a', 'b'])
  })

  it('원본 목록을 갈지 않는다', () => {
    const before = LIST.map((c) => c.id)
    matchCommands(LIST, '레이아웃')
    expect(LIST.map((c) => c.id)).toEqual(before)
  })
})

describe('groupHits', () => {
  it('연속한 같은 그룹을 묶고 등장 순서를 지킨다', () => {
    const groups = groupHits(matchCommands(LIST, ''))
    expect(groups.map((g) => g.group)).toEqual(['보기', '레이아웃', '설정'])
    expect(groups[0].hits).toHaveLength(3)
  })

  it('그룹이 떨어져 나오면 따로 묶인다 — 점수 정렬을 거스르지 않는다', () => {
    const hits = matchCommands(
      [cmd('a', '가', 'A'), cmd('b', '나', 'B'), cmd('c', '다', 'A')],
      '',
    )
    expect(groupHits(hits).map((g) => g.group)).toEqual(['A', 'B', 'A'])
  })

  it('빈 목록은 빈 결과', () => {
    expect(groupHits([])).toEqual([])
  })
})
