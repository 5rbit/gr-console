// 카드 **안의** 소제목 — 카드를 쪼개는 대신 제목 한 줄과 1px 선으로 가른다.
//
// 전에는 묶음마다 `Card` 를 하나씩 썼다(측정 진행 탭에만 카드가 셋). 카드 안 카드는 라운딩 중첩
// 금지에 걸리고, 카드를 늘어놓으면 면과 그림자가 값보다 눈에 띈다(면 예산: `docs/DESIGN.md` 5절).
// 같은 카드 안에서 소제목으로 가르면 묶음은 그대로 읽히고 면은 하나로 남는다.
//
// 자리는 셋뿐이다: 제목 · 제목 옆 `?` · 오른쪽 끝 값 하나. **조작은 두지 않는다** — 조작이
// 필요해지면 그 묶음은 제 도구 띠(`Toolbar`)를 가진 카드다.
import type { ReactNode } from 'react'
import { HelpTip, type HelpSection } from './HelpTip'

export interface SectionProps {
  title: string
  /** 제목 줄 오른쪽 — 짧은 값 하나(개수·시각·조건)만. 조작은 두지 않는다. */
  right?: ReactNode
  /**
   * 제목 옆 `?` — **묶음을 읽는 법**(셀 형식·판정 규칙·근사의 한계). 화면에 문단으로 풀어 쓰면
   * 처음 한 번 읽고 다시는 안 읽는 글이 값과 같은 자리를 상시로 먹는다.
   */
  help?: string
  /** 갈래가 있는 설명 — `?` 안이 소제목+본문 여러 칸이 된다. */
  helpSections?: readonly HelpSection[]
  children: ReactNode
  /** 카드의 첫 묶음이면 위 여백과 구분선을 두지 않는다. */
  first?: boolean
  testid?: string
}

export function Section({
  title,
  right,
  help,
  helpSections,
  children,
  first = false,
  testid,
}: SectionProps) {
  return (
    <section
      className={first ? '' : 'mt-3 border-t border-line-subtle pt-2'}
      data-testid={testid}
    >
      <div className="mb-1 flex items-baseline gap-2">
        <h3 className="text-xs font-semibold text-content-muted">{title}</h3>
        <HelpTip title={title} text={help} sections={helpSections} />
        {right ? <span className="ml-auto text-2xs text-content-faint">{right}</span> : null}
      </div>
      {children}
    </section>
  )
}
