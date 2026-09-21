// 로컬 → PLC 두 단계를 **보이게** 하는 띠 — 레이아웃 편집 화면 전용.
//
// 적용(레이아웃 생성·그리드 저장)은 콘솔의 로컬 사본만 바꾼다. 그 뒤 PLC 에 쓰는 일은 표의 ⋯ 안에
// 숨어 있어서 "적용했는데 로봇이 모른다"가 반복됐다. 그래서 (1) PLC 와 다른 행 수를 늘 보여 주고,
// (2) 쓰기를 적용 옆에 세우고, (3) 쓰기 전에 로컬 vs PLC 차이를 확인 내용으로 보여 준다.
//
// PLC 에 **무엇을 쓰는지는 바뀌지 않는다**(배열 먼저 · Count 마지막 · 되읽기 검증 · AUTO 가드는 백엔드 그대로).
import { useEffect, useMemo, useState } from 'react'
import { Upload } from 'lucide-react'
import { plcs } from '../../lib/plcs'
import { robots } from '../../lib/robots'
import { robotFailure, robotLabel } from '../../lib/robotContext'
import { RobotChip } from '../shared/RobotChip'
import { useStore } from '../../lib/store'
import { taskApi } from '../../lib/task/api'
import {
  defaultTarget,
  effectiveTarget,
  isWriteAll,
  readTarget,
  s7Plcs,
  targetLabel,
  targetOptions,
} from '../../lib/task/plcTarget'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { FieldList } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { toast } from '../../lib/ui/toast'
import type { PlcTarget } from '../../lib/task/types'
import type { Cell, DiffRow, Station } from '../../lib/types'
import { pushSummary } from './registryDialogs'

const TARGET_KEY = 'gr-plc-write-target'

export interface PlcWriteBarProps {
  what: '셀' | '스테이션'
  /** 전체 행 수. */
  rows: number
  /** PLC 미반영(로컬에서 고친) 행 수. */
  dirty: number
  /** 쓰기 뒤 목록 다시 받기 — 검증이 끝나면 미반영 표시가 사라진다. */
  reload: () => Promise<void>
  /** 칩만 내고 버튼은 아이콘으로(좁은 머리줄). */
  compact?: boolean
}

type AnyDiff = DiffRow<Cell> | DiffRow<Station>

/** 차이 한 줄 요약 — 쓰기 확인의 본문. */
function diffCounts(rows: readonly AnyDiff[]): {
  changed: number
  localOnly: number
  plcOnly: number
  ids: number[]
} {
  const ids = rows.filter((d) => d.status !== 'same').map((d) => d.id)
  return {
    changed: rows.filter((d) => d.status === 'changed').length,
    localOnly: rows.filter((d) => d.status === 'local_only').length,
    plcOnly: rows.filter((d) => d.status === 'plc_only').length,
    ids,
  }
}

export function PlcWriteBar({ what, rows, dirty, reload, compact = false }: PlcWriteBarProps) {
  useStore(plcs, robots)
  useEffect(() => {
    const offPlcs = plcs.start()
    const offRobots = robots.start()
    return () => {
      offPlcs()
      offRobots()
    }
  }, [])
  const refs = useMemo(() => s7Plcs(plcs.list), [plcs.list])
  const fallback = defaultTarget(refs, robots.current?.plc) ?? 'GR2'
  const [chosen, setChosen] = useState<PlcTarget | null>(() => {
    try {
      return (localStorage.getItem(TARGET_KEY) as PlcTarget | null) ?? null
    } catch {
      return null
    }
  })
  const plc = effectiveTarget(chosen, refs, fallback)
  const one = readTarget(plc, fallback)
  const label = targetLabel(plc, refs)
  const options = refs.length > 0 ? targetOptions(refs) : [{ value: fallback, label: fallback }]

  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [force, setForce] = useState(false)
  const [diff, setDiff] = useState<{ rows: AnyDiff[] | null; error: string | null }>({
    rows: null,
    error: null,
  })

  const connected = (name: string): boolean => {
    const hit = plcs.list.find(
      (p) => (p.name ?? p.id.replace(/_s7$/i, '')).toLowerCase() === name.toLowerCase(),
    )
    return hit?.connected ?? false
  }
  const offline = isWriteAll(plc)
    ? refs.filter((r) => !connected(r.name))
    : connected(one)
      ? []
      : [{ name: one }]
  // 비활성은 침묵하지 않고 **이유**를 말한다(DESIGN.md). AUTO 가드는 미리 알 수 없어 확인 창의 강제 쓰기로 푼다.
  const why =
    rows === 0
      ? `쓸 ${what} 행이 없습니다`
      : dirty === 0
        ? `PLC 미반영이 없습니다 — 마지막 쓰기·읽기 뒤로 고친 행이 없습니다`
        : !plcs.loaded
          ? 'PLC 목록을 받는 중…'
          : offline.length
            ? `${offline.map((p) => p.name).join(' · ')} 연결 끊김`
            : busy
              ? '쓰는 중…'
              : undefined

  function choose(v: PlcTarget) {
    setChosen(v)
    try {
      localStorage.setItem(TARGET_KEY, v)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  async function openDialog() {
    setForce(false)
    setDiff({ rows: null, error: null })
    setOpen(true)
    try {
      const r = what === '셀' ? await taskApi.cellsDiff(one) : await taskApi.stationsDiff(one)
      setDiff({ rows: r as AnyDiff[], error: null })
    } catch (e) {
      setDiff({ rows: [], error: e instanceof Error ? e.message : String(e) })
    }
  }

  /** 검증이 어긋나면 어느 행인지 알려 준다 — 되읽기는 바이트 오프셋만 주므로 차이를 다시 받아 id 를 센다. */
  async function mismatchIds(): Promise<string> {
    try {
      const r = (
        what === '셀' ? await taskApi.cellsDiff(one) : await taskApi.stationsDiff(one)
      ) as AnyDiff[]
      const ids = r.filter((d) => d.status !== 'same').map((d) => d.id)
      return ids.length
        ? ` — 아직 다른 ${what}: ${ids
            .slice(0, 5)
            .map((i) => `#${i}`)
            .join(' ')}${ids.length > 5 ? ` 외 ${ids.length - 5}` : ''}`
        : ''
    } catch {
      return ''
    }
  }

  async function write() {
    setBusy(true)
    const tid = toast.pending(`${label} 에 ${what} 쓰는 중…`)
    try {
      const r =
        what === '셀' ? await taskApi.cellsPush(plc, force) : await taskApi.stationsPush(plc, force)
      await reload()
      if (r.verified) {
        toast.resolve(tid, 'ok', `${label} · ${what} ${r.count}건 반영 · 검증 OK`)
      } else {
        toast.resolve(tid, 'warn', `${pushSummary(r)}${await mismatchIds()}`)
      }
      const w = r.warnings ?? []
      if (w.length) toast.warn(`${w[0]}${w.length > 1 ? ` 외 ${w.length - 1}건` : ''}`)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // 백엔드 거부는 `GR2: AUTO 모드 …` 로 온다 — 그러지 못한 오류(네트워크 등)에도 대상 이름을 단다.
      toast.resolve(
        tid,
        'error',
        robotFailure(
          label,
          `${what} 쓰기 실패`,
          `${msg}${/AUTO/i.test(msg) ? ' (확인 창의 “AUTO 모드여도 강제로 쓰기”로 다시)' : ''}`,
        ),
      )
    } finally {
      setBusy(false)
    }
  }

  const c = diff.rows ? diffCounts(diff.rows) : null

  return (
    <>
      <div className="flex items-center gap-1.5" data-testid="plc-write-bar">
        <StatusBadge status={dirty ? 'warn' : 'ok'} data-testid="plc-pending">
          로컬 {rows}
          {dirty ? ` · PLC 미반영 ${dirty}` : ' · PLC 반영됨'}
        </StatusBadge>
        <HelpTip
          title="로컬 저장 → PLC 쓰기"
          text={`적용·저장은 콘솔의 로컬 사본만 바꿉니다. 로봇이 쓰는 값이 되려면 PLC 쓰기까지 해야 합니다 — ${what} 배열을 Id 순으로 쓰고 Count 를 마지막에 쓴 뒤 되읽어 검증합니다.`}
        />
        <span className="flex-1" />
        <Button
          size="sm"
          intent={dirty ? 'primary' : 'ghost'}
          icon={<Upload className="h-3.5 w-3.5" />}
          disabled={!!why}
          loading={busy}
          title={why ?? `${label} 에 ${what} ${rows}건을 씁니다 (대상은 확인 창에서 바꿉니다)`}
          onClick={() => void openDialog()}
          data-testid="plc-write"
        >
          {compact ? null : 'PLC 쓰기'}
        </Button>
      </div>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        scope="single-robot"
        danger
        title={`${what} 테이블 PLC 쓰기 — ${label}`}
        confirmLabel={`${label} 에 쓰기`}
        onConfirm={() => void write()}
      >
        <div className="flex flex-col gap-2 text-xs">
          {/* 대상 PLC 는 고를 수 있지만 **지금 고른 로봇**이 무엇인지도 같이 보인다 — 둘이 어긋나면
              (GR1 을 골라 두고 GR2 테이블을 쓰는 것) 여기서 눈에 띈다. */}
          <div>
            <RobotChip
              chip={robots.chip}
              prefix="선택된 로봇"
              title={`사이드바 선택 — ${robotLabel(robots.chip)} (쓰기 대상은 아래에서 고릅니다)`}
            />
          </div>
          <Select
            label="PLC"
            value={plc}
            onValueChange={(v) => choose(v as PlcTarget)}
            data-testid="plc-write-target"
          >
            {options.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
          <FieldList
            columns={2}
            dense
            labelWidth={64}
            items={[
              { label: '대상', value: label },
              { label: '행', value: `${rows}건` },
              { label: 'PLC 미반영', value: `${dirty}건` },
              { label: '값 다름', value: c ? `${c.changed}건` : '…' },
              { label: '로컬에만', value: c ? `${c.localOnly}건` : '…' },
              { label: 'PLC에만', value: c ? `${c.plcOnly}건` : '…' },
            ]}
          />
          {diff.error ? (
            <div className="text-fault-fg">차이를 받지 못했습니다 — {diff.error}</div>
          ) : null}
          {c && c.ids.length ? (
            <div className="font-mono text-2xs text-content-tertiary" data-testid="plc-write-ids">
              다른 Id{' '}
              {c.ids
                .slice(0, 12)
                .map((i) => `#${i}`)
                .join(' ')}
              {c.ids.length > 12 ? ` 외 ${c.ids.length - 12}` : ''}
            </div>
          ) : null}
          {/* design-lint-allow: no-panel-paragraph — 대화상자의 확인 문구다(되돌릴 수 없는 조작을 말한다) */}
          <p className="m-0 text-fault-fg">
            PLC 의 기존 테이블은 되돌릴 수 없습니다 — Id 오름차순으로 쓰고 남은 슬롯은 0, Count 는
            마지막, 쓴 뒤 되읽어 대조합니다.
          </p>
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={force}
              onChange={(e) => setForce(e.currentTarget.checked)}
              data-testid="plc-write-force"
            />
            AUTO 모드여도 강제로 쓰기
            <HelpTip
              title="강제 쓰기"
              text="GR PLC 가 AUTO 모드일 때도 씁니다 — 작업 검증 중에 테이블이 바뀔 수 있습니다."
            />
          </label>
        </div>
      </ConfirmDialog>
    </>
  )
}
