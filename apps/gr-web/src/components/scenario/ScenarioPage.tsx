// 시나리오 화면 — **스텁**. 담당 에이전트가 이 파일(과 이 폴더)을 갈아 끼운다.
import { ListOrdered } from 'lucide-react'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { EmptyState } from '../../lib/ui/EmptyState'

export default function ScenarioPage() {
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-scenario">
      <ScreenHeader title="시나리오" icon={<ListOrdered className="h-4 w-4" />} />
      <div className="min-h-0 flex-1 overflow-auto">
        <EmptyState title="시나리오" hint="아직 구현되지 않은 화면입니다." />
      </div>
    </div>
  )
}
