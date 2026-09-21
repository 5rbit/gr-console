// 인디케이터 범례 — 섹션 제목 옆 `?`. 톤 넷과 그 어휘를 `lib/indicators.TONE_LEGEND` 그대로 그린다
// (본문에 늘 서는 설명이 아니라 필요할 때 여는 말풍선 — `docs/DESIGN.md` 4절 ②).
import { TONE_LEGEND } from '../../lib/indicators'
import { HelpTip } from '../../lib/ui/HelpTip'
import { IndicatorChip } from '../../lib/ui/IndicatorChip'

export function IndicatorLegend({ testid, extra }: { testid: string; extra?: string }) {
  return (
    <HelpTip title="표시 범례" testid={testid}>
      <span className="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1">
        {TONE_LEGEND.map((l) => (
          <span key={l.tone} className="contents">
            <IndicatorChip ind={{ label: l.name, tone: l.tone, tooltip: '' }} />
            <span>
              <span className="block text-content-secondary">{l.words}</span>
              <span className="block text-content-faint">{l.meaning}</span>
            </span>
          </span>
        ))}
      </span>
      {extra ? <span className="mt-1.5 block text-content-faint">{extra}</span> : null}
    </HelpTip>
  )
}
