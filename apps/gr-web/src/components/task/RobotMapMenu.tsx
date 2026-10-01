// 맵의 로봇 표식 우클릭 — 그 로봇의 운전 명령(사이드바 로봇 행과 같은 목록 · 같은 확인 규칙)과 **화물 처리**.
//
// 화물 처리는 콘솔 재고(Hand)만 고친다 — PLC 그리퍼 데이터에는 쓰지 않는다(서버 `stock/routes.rs`):
//   DROP 명령 작성   → 놓을 셀·스테이션을 맵에서 한 번 클릭(작성 카드로, 그 로봇이 선택된다)
//   Hand 지정·수정…  → 품목 · 개수 손 정정(진행 중 PICK/DROP 이 있으면 서버가 거부)
//   PLC 기준으로 맞춤 → PLC 는 들고 있는데 콘솔이 모를 때(마지막 PICK 으로 채움)
//   화물 제거…       → 사람이 그리퍼에서 들어냄: Hand 비움 + 살아 있는 DROP 취소 + 이송 지시 중단(재고 반영 없음)
import { useState } from 'react'
import { api } from '../../lib/api'
import { allStatus } from '../../lib/feeds'
import { modeName } from '../../lib/gr/const'
import { itemLabel } from '../../lib/items/model'
import { sendRobotAction } from '../../lib/robotCommand'
import {
  ROBOT_ACTIONS,
  describeRobotAction,
  robotActionDisabled,
  robotActionSpec,
} from '../../lib/robotCommandModel'
import { robots } from '../../lib/robots'
import { schedApi } from '../../lib/sched'
import { stock as stockStore } from '../../lib/stock'
import { plcHoldOf, robotCargo, type RobotCargo } from '../../lib/task/robotCargoModel'
import type { Item, Robot, RobotAction } from '../../lib/types'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { toast } from '../../lib/ui/toast'
import { ItemPicker } from '../shared/ItemPicker'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** 로봇 하나의 화물 판정 — 콘솔 Hand + 그 로봇 상태 스트림. */
export function cargoOf(r: Robot): RobotCargo {
  const { hold, detect } = plcHoldOf(allStatus.get(r.id)?.webmon)
  return robotCargo(stockStore.hand(r.plc), hold, detect)
}

type Pending =
  | { kind: 'action'; robot: Robot; action: RobotAction }
  | { kind: 'remove'; robot: Robot; cargo: RobotCargo }

export function useRobotMapMenu({
  items,
  onDropPick,
}: {
  items: readonly Item[]
  /** DROP 놓을 곳 고르기 시작(다음 맵 클릭이 DROP 작성). */
  onDropPick: (robotId: number) => void
}) {
  const [pending, setPending] = useState<Pending | null>(null)
  const [handEdit, setHandEdit] = useState<{ robot: Robot; cargo: RobotCargo } | null>(null)

  async function adopt(r: Robot) {
    try {
      await api.stockSyncResolve(r.id, 'adopt_plc')
      toast.ok(`${r.name}: PLC 기준으로 Hand 를 맞췄습니다`)
    } catch (e) {
      toast.error(`${r.name}: Hand 맞춤 실패 — ${errText(e)}`)
    }
  }

  function open(robotId: number, e: React.MouseEvent) {
    const r = robots.list.find((x) => x.id === robotId)
    if (!r) return
    const wm = allStatus.get(r.id)?.webmon ?? null
    const mode = wm ? modeName(wm.Mode) : null
    const c = cargoOf(r)
    const holding = c.state !== 'none'
    const menu: MenuItem[] = [
      { label: `${r.name} · ${c.text}` },
      { label: '운전 명령' },
      ...ROBOT_ACTIONS.map((s) => ({
        label: s.label,
        danger: s.danger,
        disabled: robotActionDisabled(s.action, r, mode),
        testid: `map-robot-cmd-${r.id}-${s.action}`,
        run: () =>
          s.confirm
            ? setPending({ kind: 'action', robot: r, action: s.action })
            : void sendRobotAction(r, s.action),
      })),
      { label: '화물' },
      {
        label: 'DROP 명령 작성 — 놓을 곳 클릭',
        disabled: holding ? undefined : '들고 있는 화물이 없습니다',
        testid: `map-robot-drop-${r.id}`,
        run: () => onDropPick(r.id),
      },
      {
        label: c.count > 0 ? 'Hand 수정…' : 'Hand 지정…',
        testid: `map-robot-hand-${r.id}`,
        run: () => setHandEdit({ robot: r, cargo: c }),
      },
      ...(c.state === 'plc_only'
        ? [
            {
              label: 'PLC 기준으로 Hand 맞춤',
              testid: `map-robot-adopt-${r.id}`,
              run: () => void adopt(r),
            },
          ]
        : []),
      {
        label: '화물 제거 — 사람이 들어냄…',
        danger: true,
        disabled: c.count > 0 ? undefined : '콘솔 Hand 가 비어 있습니다',
        testid: `map-robot-remove-${r.id}`,
        run: () => setPending({ kind: 'remove', robot: r, cargo: c }),
      },
    ]
    ctxMenu.show(e, menu)
  }

  async function removeCargo(r: Robot) {
    try {
      const res = await schedApi.handRemove(r.id)
      toast.ok(
        `${r.name}: Hand 비움${res.canceled.length ? ` · DROP 취소 ${res.canceled.length}건` : ''}`,
      )
    } catch (e) {
      toast.error(`${r.name}: 화물 제거 실패 — ${errText(e)}`)
    }
  }

  const dialogs = (
    <>
      {pending?.kind === 'action' ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope="single-robot"
          title={`${robotActionSpec(pending.action).label} — ${pending.robot.name}`}
          danger={robotActionSpec(pending.action).danger}
          confirmLabel={robotActionSpec(pending.action).label}
          onConfirm={() => void sendRobotAction(pending.robot, pending.action)}
        >
          {/* design-lint-allow: no-panel-paragraph — 대화상자의 확인 문구다(실 로봇에 가는 명령을 말한다) */}
          <p className="m-0 text-content-primary">
            {describeRobotAction(pending.action, pending.robot.name)}
          </p>
        </ConfirmDialog>
      ) : null}
      {pending?.kind === 'remove' ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope="single-robot"
          title={`화물 제거 — ${pending.robot.name}`}
          danger
          confirmLabel="화물 제거"
          onConfirm={() => void removeCargo(pending.robot)}
        >
          {/* design-lint-allow: no-panel-paragraph — 대화상자의 확인 문구다(무엇이 바뀌는지 말한다) */}
          <p className="m-0 text-content-primary">
            {`${pending.robot.name} 그리퍼의 ${pending.cargo.itemCode || '품목 모름'} × ${pending.cargo.count} 을 사람이 들어낸 것으로 기록합니다. 콘솔 Hand 를 비우고, 이 로봇의 살아 있는 DROP 을 취소하고, 이송 지시를 중단합니다. 셀 재고에는 반영하지 않고 PLC 에도 쓰지 않습니다.`}
          </p>
        </ConfirmDialog>
      ) : null}
      <HandDialog edit={handEdit} items={items} onClose={() => setHandEdit(null)} />
    </>
  )

  return { open, dialogs }
}

function HandDialog({
  edit,
  items,
  onClose,
}: {
  edit: { robot: Robot; cargo: RobotCargo } | null
  items: readonly Item[]
  onClose: () => void
}) {
  const [item, setItem] = useState<number | null>(null)
  const [count, setCount] = useState(1)
  const [busy, setBusy] = useState(false)
  const [seed, setSeed] = useState<typeof edit>(null)
  // 열릴 때 지금 Hand 로 채운다(렌더 중 초기화 — effect 한 박자 늦게 빈 값이 보이지 않게).
  if (edit !== seed) {
    setSeed(edit)
    setItem(edit?.cargo.itemCode || null)
    setCount(edit?.cargo.count || 1)
  }
  const it = item ? items.find((i) => i.code === item) : undefined
  const r = edit?.robot
  async function save() {
    if (!r || !item) return
    setBusy(true)
    try {
      await api.stockSetHand(r.id, item, count)
      toast.ok(`${r.name}: Hand ${item} × ${count}`)
      onClose()
    } catch (e) {
      toast.error(`${r.name}: Hand 저장 실패 — ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }
  return (
    <FormDialog
      open={!!edit}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`${r?.name ?? ''} · Hand`}
      size="sm"
      busy={busy}
      submitLabel="저장"
      disabledReason={!item ? '품목을 고르세요' : count < 1 ? '개수는 1 이상' : undefined}
      onSubmit={() => void save()}
      testid="map-robot-hand"
    >
      <div className="flex flex-col gap-2 text-xs">
        <span className="text-content-muted">
          {edit?.cargo.text} — 콘솔 재고만 고칩니다(PLC 그리퍼 데이터는 그대로). 비우려면 메뉴의
          화물 제거.
        </span>
        <ItemPicker value={item} onChange={setItem} items={[...items]} allowNone={false} />
        {it ? <span className="text-content-muted">{itemLabel(it)}</span> : null}
        <span className="flex items-center gap-2">
          <span className="w-14 text-content-muted">Count</span>
          <Input
            type="number"
            inputMode="numeric"
            mono
            className="w-20 text-center"
            value={String(count)}
            min={1}
            max={99}
            onValueChange={(v) => setCount(Math.max(0, Math.round(Number(v) || 0)))}
            data-testid="map-robot-hand-count"
          />
        </span>
      </div>
    </FormDialog>
  )
}
