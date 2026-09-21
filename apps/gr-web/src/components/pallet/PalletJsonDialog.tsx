// 패턴 JSON 내보내기·가져오기 — `GET /api/pallet/export.json`, `POST /api/pallet/import?dry_run=`.
// 형식은 spec_r4.json 과 같다(flows[].patterns[].slots). 흐름 id + 패턴 번호로 병합하고, 문서에 없는
// 흐름·패턴은 그대로 둔다. 적용은 같은 본문으로 미리보기(dry-run)가 통과한 뒤에만 켜진다.
import { useState } from 'react'
import { ClipboardCopy, Download, Eye, Upload } from 'lucide-react'
import { copyText } from '../../lib/clipboard'
import { palletApi } from '../../lib/pallet/api'
import type { ImportReport } from '../../lib/pallet/model'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Pairs } from '../../lib/ui/Pair'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { JSON_IMPORT_HELP } from './help'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

const ACTION_LABEL: Record<string, string> = {
  created: '새로 만듦',
  updated: '바뀜',
  unchanged: '같음',
  invalid: '오류',
  added: '추가',
  changed: '바뀜',
}

export interface PalletJsonDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onImported: () => void
}

export function PalletJsonDialog({ open, onOpenChange, onImported }: PalletJsonDialogProps) {
  const [text, setText] = useState('')
  const [report, setReport] = useState<ImportReport | null>(null)
  const [checked, setChecked] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const changeText = (v: string) => {
    setText(v)
    setReport(null)
    setChecked(null)
    setError(null)
  }

  function parse(): unknown | null {
    try {
      return JSON.parse(text)
    } catch (e) {
      setError(`JSON 이 아닙니다 — ${errMsg(e)}`)
      return null
    }
  }

  async function run<T>(f: () => Promise<T>): Promise<T | null> {
    setBusy(true)
    try {
      return await f()
    } catch (e) {
      setError(errMsg(e))
      return null
    } finally {
      setBusy(false)
    }
  }

  async function download() {
    const url = await run(() => palletApi.exportUrl())
    if (!url) return
    const a = document.createElement('a')
    a.href = url
    a.download = 'pallet-patterns.json'
    document.body.appendChild(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(url), 2000)
  }

  async function copy() {
    const doc = await run(() => palletApi.exportJson())
    if (!doc) return
    if (await copyText(JSON.stringify(doc, null, 2))) toast.ok(`흐름 ${doc.flows.length}개를 JSON 으로 복사했습니다`)
    else toast.error('클립보드에 복사하지 못했습니다')
  }

  async function loadCurrent() {
    const doc = await run(() => palletApi.exportJson())
    if (doc) changeText(JSON.stringify(doc, null, 2))
  }

  async function dryRun() {
    const doc = parse()
    if (doc === null) return
    const r = await run(() => palletApi.importJson(doc, true))
    if (r) {
      setReport(r)
      setChecked(text)
    }
  }

  async function apply() {
    const doc = parse()
    if (doc === null) return
    const r = await run(() => palletApi.importJson(doc, false))
    if (r) {
      setReport(r)
      toast.ok(`가져오기 — 새 흐름 ${r.created} · 바뀐 흐름 ${r.updated} · 같음 ${r.unchanged}`)
      onImported()
    }
  }

  const canApply = report !== null && report.dry_run && report.ok && checked === text && report.created + report.updated > 0
  const applyReason = canApply ? undefined : '미리보기가 통과하고 바뀌는 것이 있어야 적용할 수 있습니다'

  const reportColumns: Column<ImportReport['flows'][number]>[] = [
    { key: 'id', label: 'Flow', get: (f) => f.id, priority: 1 },
    { key: 'action', label: '결과', get: (f) => ACTION_LABEL[f.action] ?? f.action, priority: 1 },
    {
      key: 'patterns',
      label: 'Pattern',
      get: (f) =>
        f.patterns
          .filter((p) => p.action !== 'unchanged')
          .map((p) => `P${p.pattern} ${ACTION_LABEL[p.action] ?? p.action}`)
          .join(' · ') || '—',
      priority: 2,
    },
    {
      key: 'issues',
      label: '경고 · 오류',
      get: (f) => f.errors.length + f.warnings.length,
      cell: (f) => (
        <span className="flex flex-col">
          {f.errors.map((e) => (
            <span key={e} className="text-danger-text">
              {e}
            </span>
          ))}
          {f.warnings.map((w) => (
            <span key={w} className="text-warn-fg">
              {w}
            </span>
          ))}
        </span>
      ),
      priority: 2,
    },
  ]

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title="패턴 JSON"
      meta={
        <span className="inline-flex items-center gap-1">
          내보내기 · 가져오기
          <HelpTip title="가져오기" text={`${JSON_IMPORT_HELP} 미리보기(dry-run)가 통과해야 적용할 수 있습니다.`} />
        </span>
      }
      size="lg"
      dirty={text.trim() !== ''}
      busy={busy}
      submitLabel="가져오기 적용"
      cancelLabel="닫기"
      disabledReason={applyReason}
      onSubmit={() => void apply()}
      extra={
        <Button size="sm" intent="neutral" icon={<Eye size={14} />} disabled={busy || text.trim() === ''} onClick={() => void dryRun()} data-testid="pallet-import-dry">
          미리보기
        </Button>
      }
      testid="pallet-json"
    >
      <div className="flex flex-col gap-3 text-2xs">
        <section className="flex flex-col gap-2">
          <h4 className="font-semibold text-content-secondary">내보내기</h4>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" intent="neutral" icon={<Download size={14} />} disabled={busy} onClick={() => void download()}>
              파일로 받기
            </Button>
            <Button size="sm" intent="ghost" icon={<ClipboardCopy size={14} />} disabled={busy} onClick={() => void copy()}>
              JSON 복사
            </Button>
            <Button size="sm" intent="ghost" disabled={busy} onClick={() => void loadCurrent()} title="현재 저장소 JSON 을 아래 칸에 넣어 고친 뒤 가져올 수 있습니다">
              아래 칸에 불러오기
            </Button>
          </div>
        </section>
        <section className="flex flex-col gap-2 border-t border-line-default pt-2">
          <h4 className="font-semibold text-content-secondary">가져오기</h4>
          <label className="flex items-center gap-2">
            <Upload size={14} aria-hidden />
            <input
              type="file"
              accept="application/json,.json"
              className="text-2xs"
              onChange={(e) => {
                const f = e.currentTarget.files?.[0]
                if (f) void f.text().then(changeText)
              }}
            />
          </label>
          <textarea
            value={text}
            onChange={(e) => changeText(e.currentTarget.value)}
            spellCheck={false}
            aria-label="가져올 JSON"
            placeholder='{"flows": [{"id": "HP_IN", "drag_kind": "in", "patterns": [...]}]}'
            className="h-48 w-full rounded-md border border-line-default bg-surface-inset p-2 font-mono text-2xs text-content-primary"
            data-testid="pallet-import-text"
          />
          {error && <p className="text-danger-text">{error}</p>}
          {report && (
            <div className="flex flex-col gap-1" data-testid="pallet-import-report">
              <Pairs
                size="2xs"
                items={[
                  {
                    label: report.dry_run ? '미리보기' : '적용됨',
                    value: report.ok ? '통과' : '오류 있음',
                    tone: report.ok ? undefined : 'fault',
                  },
                  { label: '새 흐름', value: report.created, tone: report.ok ? undefined : 'fault' },
                  {
                    label: '바뀐 흐름',
                    value: report.updated,
                    tone: report.ok ? undefined : 'fault',
                  },
                  { label: '같음', value: report.unchanged, tone: report.ok ? undefined : 'fault' },
                ]}
              />
              {report.errors.map((e) => (
                <p key={e} className="text-danger-text">
                  {e}
                </p>
              ))}
              <DataTable
                rows={report.flows}
                columns={reportColumns}
                rowKey={(f) => f.id}
                density="compact"
                empty="바뀌는 흐름 없음"
                testid="pallet-import-flows"
              />
            </div>
          )}
        </section>
      </div>
    </FormDialog>
  )
}
