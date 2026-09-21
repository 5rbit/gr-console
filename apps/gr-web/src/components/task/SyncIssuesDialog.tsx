// 동기화 경고 — 로봇 실제 상태(PLC HoldItem · 그리퍼 감지 · 링)와 콘솔 Hand · 이송 지시가 어긋난 것.
// 서버는 재고를 몰래 고치지 않는다: 여기서 사람이 한 번 눌러 맞춘다(콘솔 DB 만, PLC 에는 쓰지 않는다).
import { useState } from 'react'
import { api } from '../../lib/api'
import { robotLabel, type RobotChipModel } from '../../lib/robotContext'
import type { SyncIssue } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Dialog } from '../../lib/ui/Dialog'
import { toast } from '../../lib/ui/toast'
import { RobotChip } from '../shared/RobotChip'

const ACTION_LABEL: Record<string, string> = {
  clear_hand: 'Hand 비움',
  adopt_plc: 'PLC 기준으로 Hand 맞춤',
}

const ACTION_HINT: Record<string, string> = {
  clear_hand: '콘솔 Hand 를 비우고 걸려 있던 이송 지시를 중단으로 남깁니다 (PLC 는 그대로)',
  adopt_plc:
    'PLC Completed 링의 마지막 PICK 품목·개수로 Hand 를 채우고 이송 지시를 엽니다 (PLC 는 그대로)',
}

export function SyncIssuesDialog({
  open,
  onOpenChange,
  robot,
  robotId,
  issues,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  robot: RobotChipModel
  robotId: number | null
  issues: readonly SyncIssue[]
}) {
  const [busy, setBusy] = useState<string | null>(null)
  async function resolve(action: string) {
    if (robotId === null) return
    setBusy(action)
    try {
      await api.stockSyncResolve(robotId, action)
      toast.ok(`${robot.name}: ${ACTION_LABEL[action] ?? action} — 콘솔 재고를 고쳤습니다`)
      onOpenChange(false)
    } catch (e) {
      toast.error(`${robot.name}: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(null)
    }
  }
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`동기화 경고 — ${robot.name}`}
      meta={<RobotChip chip={robot} title={robotLabel(robot)} />}
      size="md"
      closeLabel="닫기"
      testid="sync-dialog"
    >
      <div className="flex flex-col gap-3 text-xs">
        {issues.length === 0 ? (
          <span className="text-content-faint">경고 없음 — 로봇 상태와 콘솔 재고가 맞습니다</span>
        ) : (
          issues.map((i) => (
            <div key={`${i.code}-${i.transfer_order_id ?? ''}`} className="flex flex-col gap-1">
              <span className="text-content-primary">{i.message}</span>
              <span className="text-2xs text-content-faint">
                {i.since.replace('T', ' ').slice(5, 19)}
                {i.transfer_order_id ? ` · 이송 지시 ${i.transfer_order_id}` : ''}
              </span>
              {i.actions.length ? (
                <span className="flex flex-wrap gap-1">
                  {i.actions.map((a) => (
                    <Button
                      key={a}
                      size="sm"
                      intent="outline"
                      loading={busy === a}
                      disabled={busy !== null || robotId === null}
                      title={ACTION_HINT[a]}
                      onClick={() => void resolve(a)}
                      data-testid={`sync-${a}`}
                    >
                      {ACTION_LABEL[a] ?? a}
                    </Button>
                  ))}
                </span>
              ) : null}
            </div>
          ))
        )}
      </div>
    </Dialog>
  )
}
