// 작업 명령 화면 — **스텁**. 담당 에이전트가 이 파일(과 이 폴더)을 갈아 끼운다.
import { Send } from 'lucide-react'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { EmptyState } from '../../lib/ui/EmptyState'

export default function TaskIssue() {
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-task">
      <ScreenHeader title="작업 명령" icon={<Send className="h-4 w-4" />} />
      <div className="min-h-0 flex-1 overflow-auto">
        <EmptyState title="작업 명령" hint="아직 구현되지 않은 화면입니다." />
      </div>
    </div>
  )
}
