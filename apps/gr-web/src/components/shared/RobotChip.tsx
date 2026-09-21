// 로봇 칩 — **이 자리는 어느 호기의 것인가**를 말하는 한 조각. 색 점 + 이름 + 그 로봇의 상태 PLC.
//
// 한 컴포넌트로 모은 이유는 일관성이다(`docs/DESIGN.md` 일관성 다섯 축). 작업 카드 머리줄·쓰기 확인·
// 제출 확인·시나리오 실행이 각자 `로봇 GR2` 를 다르게 적고 있었고, 그중 몇 자리는 아예 적지 않았다.
// 적지 않은 자리에서 사고가 났다 — 사이드바가 GR1 을 고른 줄 모르고 GR2 앞에서 제출을 눌렀다.
//
// 칩은 **면을 만들지 않는다**(1px 보더뿐, 면 예산: `docs/DESIGN.md` 5절). 색 점은 맵의 작업 테두리·
// 사이드바 견본과 같은 색이라 "이 칩 = 저 색으로 그려지는 그 로봇"이 눈으로 이어진다.
import { plcShown, robotLabel, type RobotChipModel } from '../../lib/robotContext'
import type { FieldItem } from '../../lib/ui/FieldList'
import { cn } from '../../lib/utils'

export interface RobotChipProps {
  chip: RobotChipModel
  /** 앞에 붙는 말 — `대상` 처럼 이 칩이 무엇의 로봇인지 한 낱말로. */
  prefix?: string
  /** 테두리 없이 글자만 — 라벨+값 줄(`FieldList`) 안에 들어갈 때. */
  bare?: boolean
  title?: string
  testid?: string
}

/** 색 점 — 맵·사이드바와 같은 색. 값이 아니라 **식별**이라 글자 옆에 붙는다. */
function Dot({ color }: { color: string }) {
  return (
    <span
      aria-hidden="true"
      className="h-2.5 w-2.5 flex-none rounded-sm"
      style={{ background: color }}
    />
  )
}

export function RobotChip({ chip, prefix, bare = false, title, testid = 'robot-chip' }: RobotChipProps) {
  const plc = plcShown(chip)
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 whitespace-nowrap text-2xs',
        !bare && 'h-control-sm rounded border border-line-default px-1.5',
      )}
      title={title ?? `${prefix ? `${prefix} — ` : ''}${robotLabel(chip)}`}
      data-testid={testid}
      data-robot={chip.name}
    >
      {prefix ? <span className="text-content-faint">{prefix}</span> : null}
      <Dot color={chip.color} />
      <span className="font-medium text-content-primary">{chip.name}</span>
      {plc ? <span className="font-mono text-content-muted">{plc}</span> : null}
    </span>
  )
}

/**
 * 확인 창의 라벨+값 한 줄 — 되돌릴 수 없는 조작 앞에서는 칩이 **질문의 일부**여야 한다.
 *
 * 대화상자마다 `{ label: '로봇', value: robots.current?.name }` 을 손으로 적고 있었고, 그래서
 * 어떤 창은 이름만, 어떤 창은 PLC 만, 어떤 창은 아무것도 없었다.
 */
export function robotField(chip: RobotChipModel, label = '로봇'): FieldItem {
  return { label, value: <RobotChip chip={chip} bare testid="robot-field" /> }
}
