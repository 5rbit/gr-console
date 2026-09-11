// Task 관리 화면 — **스텁**. 담당 에이전트가 이 파일(과 이 폴더)을 갈아 끼운다.
import { ListChecks } from 'lucide-react'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { EmptyState } from '../../lib/ui/EmptyState'

export default function TaskManager() {
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-taskmgr">
      <ScreenHeader title="Task 관리" icon={<ListChecks className="h-4 w-4" />} />
      <div className="min-h-0 flex-1 overflow-auto">
        <EmptyState title="Task 관리" hint="아직 구현되지 않은 화면입니다." />
      </div>
    </div>
  )
}
