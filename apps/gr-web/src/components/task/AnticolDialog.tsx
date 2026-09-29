// 두 로봇 영역 간격 바꾸기 — 영역 대기 띠(`AreaStrip`)에서 바로 연다. 값은 파라미터 한 곳(`params`)에 이력과 함께 남는다.
// 하한은 PLC 계산 간격(p11/p12 + p16 + p13) — 서버가 한 번 더 거부한다. 지금 Task 가 나가려면 얼마인지 같이 보인다.
import { useEffect, useState } from 'react'
import { api } from '../../lib/api'
import { separationToPass } from '../../lib/task/areaStripModel'
import type { AreaView } from '../../lib/types'
import { FieldList } from '../../lib/ui/FieldList'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'

const SAFE_MM = 5000
const mm = (v: number) => v.toFixed(0)

export function AnticolDialog({
  open,
  onOpenChange,
  view,
  onSaved,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  view: AreaView
  onSaved: (cfg: { separation_mm: number; enabled: boolean }) => void
}) {
  const [text, setText] = useState(mm(view.separation_mm))
  const [enabled, setEnabled] = useState(view.enabled)
  const [busy, setBusy] = useState(false)
  // 열 때마다 지금 값에서 시작한다(닫은 사이에 다른 화면이 바꿨을 수 있다).
  useEffect(() => {
    if (open) {
      setText(mm(view.separation_mm))
      setEnabled(view.enabled)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 열 때만
  }, [open])
  const n = Number(text)
  const floor = view.plc_min === null ? null : Math.ceil(view.plc_min)
  const error = !Number.isFinite(n)
    ? '숫자를 넣으세요'
    : floor !== null && n < floor
      ? `PLC 계산 간격 ${floor} mm 보다 작을 수 없습니다`
      : n <= 0
        ? '0 보다 커야 합니다'
        : undefined
  const dirty = n !== view.separation_mm || enabled !== view.enabled
  const pass = separationToPass(view)
  const c = view.check
  const other = c ? view.robots.find((r) => r.id === c.nearest) : undefined

  async function save() {
    setBusy(true)
    try {
      const cfg = await api.anticolSet({ separation_mm: n, enabled })
      toast.ok(
        cfg.enabled
          ? `두 로봇 영역 간격 ${mm(cfg.separation_mm)} mm`
          : '두 로봇 영역 검사 꺼짐 — PLC 충돌 방지만 남습니다',
      )
      onSaved(cfg)
      onOpenChange(false)
    } catch (e) {
      toast.error(`간격 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title="두 로봇 영역 간격"
      size="sm"
      dirty={dirty}
      busy={busy}
      danger={!enabled || n < SAFE_MM}
      submitLabel="저장"
      disabledReason={error ?? (dirty ? undefined : '바뀐 값이 없습니다')}
      onSubmit={() => void save()}
      testid="anticol-dialog"
    >
      <Input
        label="간격 (mm)"
        type="number"
        mono
        value={text}
        onValueChange={setText}
        hint={
          error ??
          `두 로봇 X 사이 최소 거리 · 안전 기본값 ${SAFE_MM}${floor !== null ? ` · 하한(PLC 계산 간격) ${floor}` : ''}`
        }
        data-testid="anticol-sep"
      />
      <Switch
        inline
        label="영역 검사 사용"
        checked={enabled}
        onCheckedChange={setEnabled}
        title="끄면 콘솔이 두 로봇 영역을 보지 않습니다 — PLC 충돌 방지(회피·정지)는 그대로 동작합니다"
        testid="anticol-enabled"
      />
      <FieldList
        columns={1}
        dense
        labelWidth={96}
        items={[
          ...(c && other && c.gap !== null && c.need !== null
            ? [
                {
                  label: '다음 스텝',
                  value: `${other.name} 와 거리 ${mm(c.gap)} mm / 필요 ${mm(c.need)} mm${c.blocked ? ' — 영역 대기' : ''}`,
                },
                ...(c.blocked
                  ? [
                      {
                        label: '풀려면',
                        value:
                          pass !== null
                            ? `간격 ${pass} mm 이하 — 또는 ${other.name} 를 ${mm(c.need - c.gap)} mm 더 비키게`
                            : `간격으로는 못 풉니다(PLC 하한) — ${other.name} 를 ${mm(c.need - c.gap)} mm 더 비키게`,
                      },
                    ]
                  : []),
              ]
            : []),
          ...(!enabled || n < SAFE_MM
            ? [
                {
                  label: '주의',
                  value: !enabled
                    ? '검사를 끄면 두 로봇이 같은 구간에 명령을 받아 PLC 회피 대기가 늘어납니다. 한 대만 쓸 때만 끄세요'
                    : `안전 기본값 ${SAFE_MM} mm 보다 작습니다 — 현장 확인 후 사용`,
                },
              ]
            : []),
        ]}
      />
    </FormDialog>
  )
}
