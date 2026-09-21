// 레이아웃 편집기 — 구간별 생성 규칙(시작 id·원점·지름·간격·열/행·격자/허니콤·방향·번호 순서)으로 셀 좌표를
// 만들고 맵에 "생성 예정" 으로 미리 본 뒤 레지스트리에 적용한다(로컬 → PLC 쓰기는 셀 탭의 PLC 쓰기).
import { useEffect, useMemo, useState } from 'react'
import { Wand2 } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULT_RULE,
  conflicts,
  extent,
  fitCount,
  generate,
  inferRule,
  validateRule,
  type LayoutRule,
} from '../../lib/task/layoutGen'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'
import type { Cell, CellUpsert } from '../../lib/types'

const RULE_KEY = 'gr-layout-rule'

function loadRule(): LayoutRule {
  try {
    const s = localStorage.getItem(RULE_KEY)
    return s ? { ...DEFAULT_RULE, ...(JSON.parse(s) as Partial<LayoutRule>) } : DEFAULT_RULE
  } catch {
    return DEFAULT_RULE
  }
}

export interface LayoutEditorProps {
  cells: readonly Cell[]
  /** 미리보기 셀이 바뀔 때(맵에 그린다). */
  onPreview: (cells: CellUpsert[]) => void
  /** 적용 뒤 목록 다시 받기. */
  onApplied: () => void
  onClose?: () => void
  /** 우측 사이드바에 넣을 때 — 자체 머리줄·고정 폭·왼쪽 테두리를 뺀다. */
  embedded?: boolean
}

export function LayoutEditor({
  cells,
  onPreview,
  onApplied,
  onClose,
  embedded = false,
}: LayoutEditorProps) {
  const [rule, setRule] = useState<LayoutRule>(loadRule)
  const [replace, setReplace] = useState(true)
  const [area, setArea] = useState({ w: 10000, h: 4000 })
  const [confirm, setConfirm] = useState(false)
  const [busy, setBusy] = useState(false)
  const set = (p: Partial<LayoutRule>) => setRule((r) => ({ ...r, ...p }))
  const num = (k: keyof LayoutRule) => (s: string) => set({ [k]: Number(s) } as Partial<LayoutRule>)

  const problems = useMemo(() => validateRule(rule), [rule])
  const gen = useMemo(() => generate(rule), [rule])
  const ext = useMemo(() => extent(gen, rule.diameter), [gen, rule.diameter])
  const conf = useMemo(
    () => conflicts(gen, cells, rule.diameter, replace ? rule.section : null),
    [gen, cells, rule.diameter, replace, rule.section],
  )
  const inSection = cells.filter((c) => c.section === rule.section).length

  useEffect(() => {
    onPreview(gen)
    try {
      localStorage.setItem(RULE_KEY, JSON.stringify(rule))
    } catch {
      /* 저장 못 해도 동작 */
    }
  }, [gen, rule, onPreview])
  useEffect(() => () => onPreview([]), [onPreview])

  async function apply() {
    setBusy(true)
    try {
      const r = await api.cellsBulk(gen, replace ? rule.section : null)
      toast.ok(
        `셀 생성 ${r.created} · 갱신 ${r.updated} · 삭제 ${r.removed} (총 ${r.total}) — 로컬 사본, PLC 쓰기는 셀 탭에서`,
      )
      onApplied()
    } catch (e) {
      toast.error(`적용 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className={
        embedded
          ? 'flex min-h-0 flex-col text-xs'
          : 'flex h-full min-h-0 w-[330px] flex-none flex-col overflow-y-auto border-l border-line-default text-xs'
      }
      data-testid="layout-editor"
    >
      <div
        className={`${embedded ? 'hidden' : 'flex'} h-screen-header flex-none items-center gap-2 border-b border-line-default px-3`}
      >
        <Wand2 className="h-4 w-4 text-content-muted" />
        <span className="text-sm font-semibold">레이아웃 생성 규칙</span>
        <span className="flex-1" />
        <Button size="sm" intent="ghost" onClick={onClose}>
          닫기
        </Button>
      </div>
      <div className="flex flex-col gap-3 px-3 py-3">
        <div className="flex flex-wrap items-end gap-2">
          <Select
            label="Section"
            value={String(rule.section)}
            onValueChange={(v) => {
              const sec = Number(v)
              const inf = inferRule(cells, sec)
              set({ section: sec, ...(inf ?? {}) })
            }}
            data-testid="rule-section"
          >
            {[1, 2, 3].map((s) => (
              <option key={s} value={String(s)}>
                Section {s} ({cells.filter((c) => c.section === s).length}칸)
              </option>
            ))}
          </Select>
          <Input
            label="StartId"
            type="number"
            mono
            className="w-20"
            value={String(rule.startId)}
            onValueChange={num('startId')}
            data-testid="rule-start"
          />
          <Select
            label="Pattern"
            value={rule.pattern}
            onValueChange={(v) => set({ pattern: v as LayoutRule['pattern'] })}
            data-testid="rule-pattern"
          >
            <option value="honeycomb">허니콤(지그재그)</option>
            <option value="grid">격자</option>
          </Select>
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <Input
            label="OriginX"
            data-testid="rule-origin-x"
            type="number"
            mono
            className="w-24"
            value={String(rule.originX)}
            onValueChange={num('originX')}
          />
          <Input
            label="OriginY"
            data-testid="rule-origin-y"
            type="number"
            mono
            className="w-24"
            value={String(rule.originY)}
            onValueChange={num('originY')}
          />
          <Input
            label="Z"
            type="number"
            mono
            className="w-20"
            value={String(rule.z)}
            onValueChange={num('z')}
          />
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <Input
            label="Diameter"
            type="number"
            mono
            className="w-20"
            value={String(rule.diameter)}
            onValueChange={num('diameter')}
            data-testid="rule-diameter"
          />
          <Input
            label="Gap"
            type="number"
            mono
            className="w-20"
            value={String(rule.gap)}
            onValueChange={num('gap')}
            data-testid="rule-gap"
          />
          <Input
            label="Cols (Y)"
            type="number"
            mono
            className="w-16"
            value={String(rule.cols)}
            onValueChange={num('cols')}
            data-testid="rule-cols"
          />
          <Input
            label="Rows (X)"
            type="number"
            mono
            className="w-16"
            value={String(rule.rows)}
            onValueChange={num('rows')}
            data-testid="rule-rows"
          />
        </div>
        <div className="rounded border border-line-default p-2">
          <div className="mb-1 text-2xs font-semibold text-content-muted">
            영역에 맞춰 행(X)·열(Y) 채우기
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <Input
              label="FitX (mm)"
              type="number"
              mono
              className="w-24"
              value={String(area.w)}
              onValueChange={(s) => setArea({ ...area, w: Number(s) })}
            />
            <Input
              label="FitY (mm)"
              type="number"
              mono
              className="w-24"
              value={String(area.h)}
              onValueChange={(s) => setArea({ ...area, h: Number(s) })}
            />
            <Button
              onClick={() => {
                const f = fitCount(area.w, area.h, rule.diameter, rule.gap, rule.pattern)
                if (!f.cols || !f.rows) toast.warn('영역이 셀 하나보다 작습니다')
                else set({ cols: f.cols, rows: f.rows })
              }}
              data-testid="rule-fit"
            >
              최대 채움
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Switch
            inline
            label="ShortOddRows"
            checked={rule.shortOddRows}
            onCheckedChange={(b) => set({ shortOddRows: b })}
          />
          <Switch
            inline
            label="Serpentine"
            checked={rule.serpentine}
            onCheckedChange={(b) => set({ serpentine: b })}
          />
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <Select
            label="DirX"
            value={String(rule.dirX)}
            onValueChange={(v) => set({ dirX: Number(v) as 1 | -1 })}
          >
            <option value="1">X 증가</option>
            <option value="-1">X 감소</option>
          </Select>
          <Select
            label="DirY"
            value={String(rule.dirY)}
            onValueChange={(v) => set({ dirY: Number(v) as 1 | -1 })}
          >
            <option value="1">Y 증가</option>
            <option value="-1">Y 감소</option>
          </Select>
          <Select
            label="Order"
            value={rule.order}
            onValueChange={(v) => set({ order: v as 'row' | 'col' })}
          >
            <option value="row">행 우선</option>
            <option value="col">열 우선</option>
          </Select>
          <Input
            label="Slot (0=Diameter)"
            type="number"
            mono
            className="w-20"
            value={String(rule.slot)}
            onValueChange={num('slot')}
          />
        </div>

        <div
          className="rounded border border-line-default p-2 font-mono text-2xs tabular-nums"
          data-testid="rule-summary"
        >
          <div>
            생성 {gen.length}칸 · id {rule.startId}~{rule.startId + gen.length - 1} · 중심 간격{' '}
            {rule.diameter + rule.gap}
          </div>
          <div>
            외곽 {ext.w} × {ext.h} mm · 면적 {(ext.area / 1e6).toFixed(2)} m²
          </div>
          <div>
            행 간격{' '}
            {rule.pattern === 'honeycomb'
              ? ((rule.diameter + rule.gap) * 0.866).toFixed(0)
              : rule.diameter + rule.gap}{' '}
            mm
          </div>
        </div>
        {problems.length ? (
          <ul className="list-disc pl-4 text-fault-fg">
            {problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        ) : null}
        {conf.length ? (
          <div className="rounded border border-warn bg-warn-soft p-2 text-warn-fg">
            <b>충돌 {conf.length}건</b>
            <ul className="list-disc pl-4">
              {conf.slice(0, 6).map((c, i) => (
                <li key={i}>{c.reason}</li>
              ))}
              {conf.length > 6 ? <li>… 외 {conf.length - 6}건</li> : null}
            </ul>
          </div>
        ) : null}
        <Switch
          inline
          label={`구간 ${rule.section} 기존 셀 ${inSection}칸 교체(삭제 후 생성)`}
          checked={replace}
          onCheckedChange={setReplace}
        />
        <div className="flex justify-end gap-2">
          <Button
            intent="primary"
            disabled={!gen.length || problems.length > 0 || busy}
            loading={busy}
            onClick={() => setConfirm(true)}
            data-testid="rule-apply"
          >
            레지스트리에 적용
          </Button>
        </div>
        <p className="text-2xs text-content-muted">
          적용하면 로컬 사본(점선 = PLC 미반영)이 됩니다. 확정하려면 셀 탭에서 <b>PLC 쓰기</b>.
        </p>
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single"
        danger={replace && inSection > 0}
        title="레이아웃 적용"
        confirmLabel="적용"
        onConfirm={() => void apply()}
      >
        <p className="text-xs">
          구간 {rule.section} 에 {gen.length}칸 생성
          {replace && inSection ? ` · 기존 ${inSection}칸 삭제` : ''}
          {conf.length ? ` · 충돌 ${conf.length}건(덮어쓰기/겹침)` : ''}
        </p>
      </ConfirmDialog>
    </div>
  )
}
