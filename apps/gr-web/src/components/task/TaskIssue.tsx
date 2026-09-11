// 작업 명령 화면 — 왼쪽 레지스트리 레일(품목·셀·스테이션), 오른쪽 작성 카드(미리보기·게이트·Ack).
//
// 세 목록은 여기서 한 번 받아 레일과 작성 카드(피커)가 나눠 쓴다 — 레일에서 고치면 피커 목록도 같이 바뀐다.
import { useCallback, useEffect, useState } from 'react'
import { Send } from 'lucide-react'
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import type { Cell, Defaults, Item, Station } from '../../lib/types'
import { ComposeCard } from './ComposeCard'
import { DefaultsDialog } from './DefaultsDialog'
import { GateBanner, useGate } from './GateBanner'
import { RegistryRail } from './RegistryRail'

export default function TaskIssue() {
  const items = useRegistry<Item>(api.items)
  const cells = useRegistry<Cell>(api.cells)
  const stations = useRegistry<Station>(api.stations)
  const { gate, error: gateError } = useGate()
  const [defaults, setDefaults] = useState<Defaults | null>(null)
  const [defaultsOpen, setDefaultsOpen] = useState(false)

  const loadDefaults = useCallback(async () => {
    try {
      setDefaults(await api.defaults())
    } catch {
      /* 카드가 "기본값 없음"으로 그린다 */
    }
  }, [])
  useEffect(() => {
    void loadDefaults()
  }, [loadDefaults])

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-task">
      <ScreenHeader
        title="작업 명령"
        icon={<Send className="h-4 w-4" />}
        items={[
          { label: '품목', value: String(items.items.length) },
          { label: '셀', value: String(cells.items.length) },
          { label: '스테이션', value: String(stations.items.length) },
          { label: '게이트', value: gate ? (gate.can_submit ? '열림' : `닫힘 ${gate.reasons.length}`) : '…' },
        ]}
      />
      <div className="flex min-h-0 flex-1">
        <section className="flex min-h-0 min-w-0 flex-[3] flex-col border-r border-slate-200 dark:border-slate-700" aria-label="레지스트리">
          <RegistryRail items={items} cells={cells} stations={stations} />
        </section>
        <section className="flex min-h-0 w-[440px] min-w-[380px] flex-[2] flex-col gap-3 overflow-y-auto p-3" aria-label="작업 작성">
          <GateBanner gate={gate} error={gateError} />
          <ComposeCard items={items.items} cells={cells.items} stations={stations.items} defaults={defaults} gate={gate} onOpenDefaults={() => setDefaultsOpen(true)} />
        </section>
      </div>
      <DefaultsDialog open={defaultsOpen} onOpenChange={setDefaultsOpen} defaults={defaults} onSaved={setDefaults} />
    </div>
  )
}
