// 레이아웃 편집기 — 구간별 생성 규칙(시작 id·원점·지름·간격·열/행·격자/허니콤·방향·번호 순서)으로 셀 좌표를
// 만들고 맵에 "생성 예정" 으로 미리 본 뒤 레지스트리에 적용한다(로컬 → PLC 쓰기는 셀 탭의 PLC 쓰기).
import { useEffect, useMemo, useState } from 'react'
import { Wand2 } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULT_RULE,
  MODE_LABEL,
  annotate,
  applyButtonLabel,
  extent,
  fitCount,
  generate,
  inferRule,
  previewApply,
  previewLabel,
  ruleWarnings,
  validateRule,
  type LayoutRule,
  type PreviewCell,
} from '../../lib/task/layoutGen'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'
import type { Cell, CellBulkMode, CellBulkResult } from '../../lib/types'
import { PlcWriteBar } from './PlcWriteBar'

const RULE_KEY = 'gr-layout-rule'
const APPLY_KEY = 'gr-layout-apply'

function loadRule(): LayoutRule {
  try {
    const s = localStorage.getItem(RULE_KEY)
    return s ? { ...DEFAULT_RULE, ...(JSON.parse(s) as Partial<LayoutRule>) } : DEFAULT_RULE
  } catch {
    return DEFAULT_RULE
  }
}

/** 적용 방식은 한 번 고르면 기억한다 — 기본은 **추가**(아무것도 지우지 않는다). */
function loadApply(): { mode: CellBulkMode; autoId: boolean } {
  try {
    const s = localStorage.getItem(APPLY_KEY)
    const v = s ? (JSON.parse(s) as { mode?: CellBulkMode; autoId?: boolean }) : {}
    return { mode: v.mode ?? 'append', autoId: v.autoId ?? false }
  } catch {
    return { mode: 'append', autoId: false }
  }
}

export interface LayoutEditorProps {
  cells: readonly Cell[]
  /** 미리보기 셀이 바뀔 때(맵에 그린다 — 충돌 판정이 실려 있다). */
  onPreview: (cells: PreviewCell[]) => void
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
  const [{ mode, autoId }, setApplyOpts] = useState(loadApply)
  const [area, setArea] = useState({ w: 10000, h: 4000 })
  const [confirm, setConfirm] = useState(false)
  const [busy, setBusy] = useState(false)
  /** 적용 뒤 서버가 돌려준 줄별 결과 — 판정의 **진실**은 서버다. */
  const [result, setResult] = useState<CellBulkResult | null>(null)
  const set = (p: Partial<LayoutRule>) => setRule((r) => ({ ...r, ...p }))
  const num = (k: keyof LayoutRule) => (s: string) => set({ [k]: Number(s) } as Partial<LayoutRule>)
  const setApply = (p: { mode?: CellBulkMode; autoId?: boolean }) =>
    setApplyOpts((cur) => {
      const next = { ...cur, ...p }
      try {
        localStorage.setItem(APPLY_KEY, JSON.stringify(next))
      } catch {
        /* 저장 못 해도 동작 */
      }
      return next
    })

  const problems = useMemo(() => validateRule(rule), [rule])
  // 바닥 Z ≤ 0 은 막지 않는다 — 바닥 평탄도 보정이라 음수가 맞을 수 있고, PLC 가 거부하는 건 그 셀로 나간 작업이다.
  const warns = useMemo(() => ruleWarnings(rule), [rule])
  const gen = useMemo(() => generate(rule), [rule])
  const ext = useMemo(() => extent(gen, rule.diameter), [gen, rule.diameter])
  const plan = useMemo(
    () =>
      previewApply(gen, cells, { mode, section: rule.section, diameter: rule.diameter, autoId }),
    [gen, cells, mode, rule.section, rule.diameter, autoId],
  )
  const skipped = plan.rows.filter((r) => r.outcome === 'skipped')
  const marked = useMemo(() => annotate(gen, plan), [gen, plan])
  const inSection = cells.filter((c) => c.section === rule.section).length
  const dirty = cells.filter((c) => c.dirty).length

  useEffect(() => {
    onPreview(marked)
    try {
      localStorage.setItem(RULE_KEY, JSON.stringify(rule))
    } catch {
      /* 저장 못 해도 동작 */
    }
  }, [marked, rule, onPreview])
  useEffect(() => () => onPreview([]), [onPreview])

  async function apply() {
    setBusy(true)
    try {
      const r = await api.cellsBulk(gen, {
        mode,
        section: mode === 'replace_section' ? rule.section : undefined,
        autoId,
        diameter: rule.diameter,
      })
      setResult(r)
      const line = `${MODE_LABEL[r.mode]} — ${previewLabel(r)} (총 ${r.total}칸)`
      if (r.skipped) toast.warn(`${line} · 건너뛴 칸은 아래 사유를 보세요`)
      else toast.ok(`${line} — 로컬 사본입니다. 로봇에 넣으려면 PLC 쓰기`)
      for (const w of (r.warnings ?? []).slice(0, 1)) toast.warn(w)
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
        {warns.length ? (
          <ul className="list-disc pl-4 text-warn-fg" data-testid="rule-warnings">
            {warns.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        ) : null}
        {/* 적용 방식 — 기본은 **추가**(지우지 않는다). 구역 교체만 기존 셀을 지운다. */}
        <div className="flex flex-wrap items-center gap-2">
          <Segmented
            ariaLabel="적용 방식"
            compact
            value={mode}
            onChange={(m) => setApply({ mode: m })}
            options={[
              {
                id: 'append' as CellBulkMode,
                label: MODE_LABEL.append,
                title: '겹치지 않는 칸만 더한다 — 아무것도 지우지 않고 충돌은 건너뛴다',
                testid: 'rule-mode-append',
              },
              {
                id: 'replace_section' as CellBulkMode,
                label: MODE_LABEL.replace_section,
                title: `구간 ${rule.section} 의 기존 ${inSection}칸을 지우고 새로 깐다`,
                testid: 'rule-mode-replace',
              },
              {
                id: 'overwrite' as CellBulkMode,
                label: MODE_LABEL.overwrite,
                title: '겹쳐도 생성한 값으로 덮어쓴다 — 지우지는 않는다',
                testid: 'rule-mode-overwrite',
              },
            ]}
          />
          <Switch
            inline
            label="AutoId"
            checked={autoId}
            disabled={mode !== 'append'}
            onCheckedChange={(b) => setApply({ autoId: b })}
            title="id 가 겹치면 그 구간의 빈 id 로 옮겨 담는다(건너뛰는 대신). 겹침 검사는 그대로"
          />
        </div>
        <div
          className="rounded border border-line-default p-2 font-mono text-2xs tabular-nums"
          data-testid="rule-preview"
        >
          {previewLabel({ ...plan.counts, removed: plan.removed })}
        </div>
        {skipped.length ? (
          <div
            className="rounded border border-warn bg-warn-soft p-2 text-warn-fg"
            data-testid="rule-conflicts"
          >
            <b>건너뜀 {skipped.length}칸</b>
            <ul className="list-disc pl-4">
              {skipped.slice(0, 6).map((c) => (
                <li key={c.id}>
                  #{c.id} — {c.reason}
                </li>
              ))}
              {skipped.length > 6 ? <li>… 외 {skipped.length - 6}칸</li> : null}
            </ul>
          </div>
        ) : null}
        {result ? (
          <div
            className="rounded border border-line-default p-2 text-2xs text-content-tertiary"
            data-testid="rule-result"
          >
            <b>적용 결과(서버)</b> — {MODE_LABEL[result.mode]} · {previewLabel(result)}
            <ul className="list-disc pl-4">
              {result.rows
                .filter((r) => r.outcome === 'skipped' || r.outcome === 'remapped')
                .slice(0, 6)
                .map((r) => (
                  <li key={r.id}>
                    #{r.id}
                    {r.new_id ? ` → #${r.new_id}` : ''} {r.reason ?? ''}
                  </li>
                ))}
            </ul>
          </div>
        ) : null}
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button
            intent="primary"
            disabled={!gen.length || problems.length > 0 || busy}
            loading={busy}
            onClick={() => setConfirm(true)}
            title={`${MODE_LABEL[mode]} 로 레지스트리(로컬 사본)에 적용합니다`}
            data-testid="rule-apply"
          >
            {applyButtonLabel(mode, plan)}
          </Button>
        </div>
        {/* 로컬 저장 → PLC 쓰기 두 단계를 같은 자리에서 끝낸다(표의 ⋯ 안에 숨기지 않는다). */}
        <PlcWriteBar what="셀" rows={cells.length} dirty={dirty} reload={async () => onApplied()} />
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single"
        danger={mode === 'replace_section' && plan.removed > 0}
        title={`레이아웃 적용 — ${MODE_LABEL[mode]}`}
        confirmLabel="적용"
        onConfirm={() => void apply()}
      >
        <p className="text-xs">
          구간 {rule.section} · {previewLabel({ ...plan.counts, removed: plan.removed })}
          {mode === 'append' ? ' (지우지 않습니다)' : ''}
        </p>
      </ConfirmDialog>
    </div>
  )
}
