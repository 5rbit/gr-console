import { describe, expect, it } from 'vitest'
import {
  activatePane,
  closePane,
  emptyLayout,
  findZone,
  layoutOf,
  movePane,
  normalize,
  openPane,
  openPanes,
  parseLayout,
  renderedPanes,
  resizeZone,
  serializeLayout,
  toggleMaximize,
  toggleZone,
  ZONE_LIMITS,
} from './model'

const KNOWN = ['task', 'taskmgr', 'measure', 'scenario', 'robots', 'plcs', 'status']

function base() {
  return layoutOf({
    left: ['robots', 'plcs'],
    center: ['task', 'taskmgr'],
    right: ['status'],
  })
}

describe('layoutOf', () => {
  it('첫 패널을 활성 탭으로 세운다', () => {
    const l = base()
    expect(l.zones.center.active).toBe('task')
    expect(l.zones.left.active).toBe('robots')
  })

  it('빈 존은 접어 둔다 — 탭 없는 빈 면이 폭을 먹지 않게', () => {
    const l = base()
    expect(l.zones.bottom.panes).toEqual([])
    expect(l.zones.bottom.collapsed).toBe(true)
    expect(l.zones.left.collapsed).toBe(false)
  })

  it('중앙은 비어 있어도 접히지 않는다', () => {
    expect(emptyLayout().zones.center.collapsed).toBe(false)
  })
})

describe('findZone / openPanes / renderedPanes', () => {
  it('패널이 든 존을 찾고, 없으면 null', () => {
    const l = base()
    expect(findZone(l, 'plcs')).toBe('left')
    expect(findZone(l, 'measure')).toBe(null)
  })

  it('열린 패널은 존 순서·탭 순서로 늘어놓는다', () => {
    expect(openPanes(base())).toEqual(['robots', 'plcs', 'task', 'taskmgr', 'status'])
  })

  it('그려지는 것은 펼쳐진 존의 활성 탭뿐이다', () => {
    expect(renderedPanes(base())).toEqual(['robots', 'task', 'status'])
  })

  it('접힌 존은 그리지 않는다', () => {
    expect(renderedPanes(toggleZone(base(), 'left'))).toEqual(['task', 'status'])
  })

  it('최대화 중이면 그 하나만 그린다', () => {
    expect(renderedPanes(toggleMaximize(base(), 'task'))).toEqual(['task'])
  })
})

describe('openPane', () => {
  it('새 패널은 그 존의 탭 끝에 붙고 활성이 된다', () => {
    const l = openPane(base(), 'measure', 'bottom')
    expect(l.zones.bottom.panes).toEqual(['measure'])
    expect(l.zones.bottom.active).toBe('measure')
    expect(l.zones.bottom.collapsed).toBe(false)
  })

  it('이미 열린 패널은 중복 탭을 만들지 않고 그 자리에서 활성화된다', () => {
    const l = openPane(base(), 'taskmgr', 'bottom')
    expect(l.zones.bottom.panes).toEqual([])
    expect(l.zones.center.active).toBe('taskmgr')
  })

  it('접힌 존에 열면 펼쳐진다', () => {
    const l = openPane(base(), 'measure', 'bottom')
    expect(l.zones.bottom.collapsed).toBe(false)
  })
})

describe('activatePane', () => {
  it('접힌 존의 탭을 고르면 존이 펼쳐진다', () => {
    const collapsed = toggleZone(base(), 'left')
    const l = activatePane(collapsed, 'plcs')
    expect(l.zones.left.collapsed).toBe(false)
    expect(l.zones.left.active).toBe('plcs')
  })

  it('다른 것이 최대화돼 있으면 최대화를 푼다 — 누른 탭이 안 보이는 일은 없다', () => {
    const max = toggleMaximize(base(), 'task')
    expect(activatePane(max, 'plcs').maximized).toBe(null)
  })

  it('열려 있지 않은 패널은 그대로 둔다', () => {
    const l = base()
    expect(activatePane(l, 'measure')).toBe(l)
  })
})

describe('closePane', () => {
  it('닫은 자리의 이웃이 활성이 된다', () => {
    const l = closePane(layoutOf({ center: ['task', 'taskmgr', 'measure'] }), 'taskmgr')
    expect(l.zones.center.panes).toEqual(['task', 'measure'])
    // 활성이던 것은 task라 그대로.
    expect(l.zones.center.active).toBe('task')
  })

  it('활성 탭을 닫으면 같은 자리의 다음 탭이 활성이 된다', () => {
    const l = closePane(layoutOf({ center: ['task', 'taskmgr', 'measure'] }), 'task')
    expect(l.zones.center.active).toBe('taskmgr')
  })

  it('마지막 탭을 닫으면 앞 탭이 활성이 된다', () => {
    const start = activatePane(layoutOf({ center: ['task', 'taskmgr'] }), 'taskmgr')
    expect(closePane(start, 'taskmgr').zones.center.active).toBe('task')
  })

  it('중앙의 마지막 탭은 닫히지 않는다 — 작업면이 비면 안 된다', () => {
    const l = layoutOf({ center: ['task'] })
    expect(closePane(l, 'task')).toBe(l)
  })

  it('사이드 존이 비면 접힌다', () => {
    const l = closePane(base(), 'status')
    expect(l.zones.right.panes).toEqual([])
    expect(l.zones.right.collapsed).toBe(true)
  })

  it('최대화된 패널을 닫으면 최대화가 풀린다', () => {
    const max = toggleMaximize(base(), 'plcs')
    expect(closePane(max, 'plcs').maximized).toBe(null)
  })
})

describe('movePane', () => {
  it('다른 존으로 옮기면 원래 존에서 빠지고 새 존의 활성이 된다', () => {
    const l = movePane(base(), 'plcs', 'right')
    expect(l.zones.left.panes).toEqual(['robots'])
    expect(l.zones.right.panes).toEqual(['status', 'plcs'])
    expect(l.zones.right.active).toBe('plcs')
  })

  it('index로 탭 순서를 지정한다', () => {
    const l = movePane(base(), 'plcs', 'right', 0)
    expect(l.zones.right.panes).toEqual(['plcs', 'status'])
  })

  it('같은 존 안에서 순서를 바꾼다', () => {
    const l = movePane(base(), 'taskmgr', 'center', 0)
    expect(l.zones.center.panes).toEqual(['taskmgr', 'task'])
  })

  it('index가 범위를 넘으면 끝으로 조인다', () => {
    const l = movePane(base(), 'plcs', 'right', 99)
    expect(l.zones.right.panes).toEqual(['status', 'plcs'])
  })

  it('중앙의 마지막 탭은 밖으로 나가지 않는다', () => {
    const l = layoutOf({ center: ['task'], left: ['robots'] })
    expect(movePane(l, 'task', 'left')).toBe(l)
  })

  it('열려 있지 않은 패널을 옮기라면 그 존에 새로 연다', () => {
    const l = movePane(base(), 'measure', 'bottom')
    expect(l.zones.bottom.panes).toEqual(['measure'])
  })

  it('옮기고 나서 원래 존이 비면 접힌다', () => {
    const l = movePane(base(), 'status', 'center')
    expect(l.zones.right.collapsed).toBe(true)
    expect(l.zones.center.panes).toEqual(['task', 'taskmgr', 'status'])
  })
})

describe('toggleZone', () => {
  it('접고 펼친다', () => {
    const closed = toggleZone(base(), 'left')
    expect(closed.zones.left.collapsed).toBe(true)
    expect(toggleZone(closed, 'left').zones.left.collapsed).toBe(false)
  })

  it('빈 존은 펼치지 않는다 — 빈 면을 열어 줄 이유가 없다', () => {
    const l = base()
    expect(toggleZone(l, 'bottom', false)).toBe(l)
  })

  it('중앙은 접히지 않는다', () => {
    const l = base()
    expect(toggleZone(l, 'center')).toBe(l)
  })

  it('같은 상태를 요청하면 새 객체를 만들지 않는다', () => {
    const l = base()
    expect(toggleZone(l, 'left', false)).toBe(l)
  })
})

describe('resizeZone', () => {
  it('한계로 조인다', () => {
    expect(resizeZone(base(), 'left', 10).zones.left.size).toBe(ZONE_LIMITS.left.min)
    expect(resizeZone(base(), 'left', 9999).zones.left.size).toBe(ZONE_LIMITS.left.max)
  })

  it('중앙은 크기가 없다', () => {
    const l = base()
    expect(resizeZone(l, 'center', 300)).toBe(l)
  })

  it('같은 값이면 새 객체를 만들지 않는다 — 드래그가 매 프레임 부른다', () => {
    const l = resizeZone(base(), 'left', 300)
    expect(resizeZone(l, 'left', 300)).toBe(l)
  })
})

describe('toggleMaximize', () => {
  it('같은 패널을 다시 부르면 풀린다', () => {
    const max = toggleMaximize(base(), 'task')
    expect(max.maximized).toBe('task')
    expect(toggleMaximize(max, 'task').maximized).toBe(null)
  })

  it('null이면 무조건 푼다', () => {
    expect(toggleMaximize(toggleMaximize(base(), 'task'), null).maximized).toBe(null)
  })

  it('열려 있지 않은 패널은 최대화하지 않는다', () => {
    const l = base()
    expect(toggleMaximize(l, 'measure')).toBe(l)
  })
})

describe('normalize', () => {
  it('모르는 패널 id를 걷어 낸다 — 백엔드가 안 내는 화면의 죽은 탭', () => {
    const l = normalize(layoutOf({ center: ['task', 'ghost'] }), KNOWN)
    expect(l.zones.center.panes).toEqual(['task'])
  })

  it('두 존에 겹친 패널은 앞선 존만 남긴다', () => {
    const dup = layoutOf({ left: ['status'], center: ['task'], right: ['status'] })
    const l = normalize(dup, KNOWN)
    expect(l.zones.left.panes).toEqual(['status'])
    expect(l.zones.right.panes).toEqual([])
  })

  it('활성 탭이 그 존에 없으면 첫 탭으로 맞춘다', () => {
    const broken = layoutOf({ center: ['task', 'taskmgr'] })
    broken.zones.center.active = 'measure'
    expect(normalize(broken, KNOWN).zones.center.active).toBe('task')
  })

  it('중앙이 비면 fallback을 넣는다 — 작업면 없는 레이아웃은 존재할 수 없다', () => {
    const l = normalize(layoutOf({ left: ['robots'] }), KNOWN, 'measure')
    expect(l.zones.center.panes).toEqual(['measure'])
    expect(l.zones.center.active).toBe('measure')
  })

  it('fallback이 다른 존에 있으면 끌어온다(중복을 만들지 않는다)', () => {
    const l = normalize(layoutOf({ left: ['robots', 'measure'] }), KNOWN, 'measure')
    expect(l.zones.center.panes).toEqual(['measure'])
    expect(l.zones.left.panes).toEqual(['robots'])
    expect(l.zones.left.active).toBe('robots')
  })

  it('fallback을 모르면 알려진 첫 패널로 채운다', () => {
    const l = normalize(layoutOf({}), KNOWN, 'ghost')
    expect(l.zones.center.panes).toEqual(['task'])
  })

  it('빈 사이드 존은 접고, 크기는 한계로 조인다', () => {
    const wide = layoutOf({ center: ['task'], left: ['robots'], sizes: { left: 9999 } })
    const l = normalize(wide, KNOWN)
    expect(l.zones.left.size).toBe(ZONE_LIMITS.left.max)
    expect(l.zones.right.collapsed).toBe(true)
  })

  it('사라진 패널을 최대화 중이었으면 최대화를 푼다', () => {
    const broken = layoutOf({ center: ['task'] })
    broken.maximized = 'ghost'
    expect(normalize(broken, KNOWN).maximized).toBe(null)
  })

  it('원본을 갈지 않는다', () => {
    const src = layoutOf({ center: ['task', 'ghost'] })
    normalize(src, KNOWN)
    expect(src.zones.center.panes).toEqual(['task', 'ghost'])
  })
})

describe('serializeLayout / parseLayout', () => {
  it('왕복한다', () => {
    const l = base()
    expect(parseLayout(serializeLayout(l))).toEqual(l)
  })

  it('빈 값·깨진 JSON·다른 버전은 null', () => {
    expect(parseLayout(null)).toBe(null)
    expect(parseLayout('{')).toBe(null)
    expect(parseLayout(JSON.stringify({ v: 2, layout: base() }))).toBe(null)
  })

  it('존이 빠진 값은 null — 렌더가 undefined를 만지지 않게', () => {
    expect(parseLayout(JSON.stringify({ v: 1, layout: { maximized: null, zones: {} } }))).toBe(null)
  })

  it('필드 타입이 틀린 값은 안전한 기본으로 떨어진다', () => {
    const dirty = {
      v: 1,
      layout: {
        maximized: 7,
        zones: {
          left: { panes: ['robots', 3], active: 9, collapsed: 'yes', size: 'wide' },
          center: { panes: ['task'], active: 'task', collapsed: false, size: 0 },
          right: { panes: [], active: null, collapsed: true, size: 280 },
          bottom: { panes: [], active: null, collapsed: true, size: 200 },
        },
      },
    }
    const l = parseLayout(JSON.stringify(dirty))
    expect(l).not.toBe(null)
    expect(l?.maximized).toBe(null)
    expect(l?.zones.left.panes).toEqual(['robots'])
    expect(l?.zones.left.active).toBe(null)
    expect(l?.zones.left.collapsed).toBe(false)
    expect(l?.zones.left.size).toBe(240)
  })
})
