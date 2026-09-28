// 제출 · 스케줄링 파라미터 팝업 — ParamsPanel 을 그대로 싸고, 값은 바로 적용되므로 닫기 하나.
import { Dialog } from '../../../lib/ui/Dialog'
import { ParamsPanel } from '../../task/ParamsPanel'

export function ParamsDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="스케줄링 · 생성 파라미터"
      size="lg"
      closeLabel="닫기"
      testid="sched-params-dialog"
    >
      <ParamsPanel />
    </Dialog>
  )
}
