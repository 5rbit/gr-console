// 레지스트리 툴바 — 띠에는 **추가 · PLC 읽기 · ⋯** 셋만 서고 나머지는 넘침 메뉴로 접힌다.
//
// 열한 개의 버튼이 한 줄에 서 있을 때, 그중 손이 실제로 가는 것은 "표를 PLC 값으로 채우는" PLC
// 읽기와 "행 하나 만드는" 추가였다. 나머지(편집·삭제·쓰기·차이·Excel·대상 고르기·새로고침)는
// 하루에 몇 번이고, 그것들이 늘 보이면 매번 열한 개를 훑어야 두 개를 찾는다. 그래서 자주 쓰는
// 둘만 남기고 나머지는 ⋯ 안으로 넣었다 — **사라진 기능은 없다**(메뉴가 전부 그대로 연다).
//
// 편집·삭제는 원래도 다이얼로그를 여는 조작이었다(`forms.tsx`) — 띠에서 메뉴로 자리만 옮겼고,
// 표에서 행을 두 번 누르는 길은 각 레지스트리가 따로 낸다.
//
// 세 레지스트리(품목·셀·스테이션)와 화물 규격 화면이 같은 띠를 쓴다. PLC 조작은 셀·스테이션에만 있다(품목은
// 콘솔 전용). 다이얼로그 상태(쓰기 확인·차이·가져오기 미리보기)는 이 컴포넌트가 들고, 결과는 토스트 + 다이얼로그로 낸다.
//
// PLC 대상은 `/api/plcs`의 S7 목록에서 만든다(설정에 GR1이 있으면 GR1도 뜬다). 기본은 선택된 로봇의 PLC다 —
// 사용자가 한 번 고르면 그 값을 쓰고, 그 PLC가 목록에서 사라지면 기본으로 돌아간다.
//
// **Excel 로 추가하기(화물 규격).** `io.templateUrl` 이 있는 레지스트리는 `추가`가 두 쪽 버튼이 된다 —
// 왼쪽은 오늘처럼 폼 하나, 오른쪽 `▾` 는 직접 입력 · Excel 가져오기 · 양식 받기 · Excel 내보내기.
// 품목은 수십 개씩 들여오는 일이 폼보다 잦은데 그 길이 `⋯` 속에 있어서 아무도 찾지 못했다.
// 그런 레지스트리의 `⋯` 에서는 Excel 항목을 뺀다(같은 조작이 두 메뉴에 있으면 어느 쪽이 맞는지 묻게 된다).
import type * as React from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, Download, Plus } from 'lucide-react'
import { plcs } from '../../lib/plcs'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import {
  defaultTarget,
  effectiveTarget,
  readTarget,
  s7Plcs,
  targetLabel,
  targetOptions,
} from '../../lib/task/plcTarget'
import { Button } from '../../lib/ui/Button'
import { Toolbar } from '../../lib/ui/Toolbar'
import { toast } from '../../lib/ui/toast'
import type { DiffRow } from '../../lib/types'
import type { FileImportResult, PlcTarget, PushResult } from '../../lib/task/types'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import type { HelpSection } from '../../lib/ui/HelpTip'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { menuItems, type MenuEntry } from '../../lib/task/menuEntries'
import { SHEET_ACCEPT, importOutcome, outcomeSummary } from '../../lib/task/importPreview'
import { DiffDialog, PushDialog, pushSummary } from './registryDialogs'
import { ImportDialog } from './ImportDialog'

/** 한 레지스트리의 PLC/Excel 백엔드 묶음. 품목은 `plc`가 없다. */
export interface RegistryIo<T> {
  plc?: {
    import: (plc: PlcTarget) => Promise<FileImportResult>
    push: (plc: PlcTarget, force: boolean) => Promise<PushResult>
    diff: (plc: PlcTarget) => Promise<DiffRow<T>[]>
    summarize: (v: T) => string
  }
  exportUrl: string
  importFile: (file: File, dryRun: boolean) => Promise<FileImportResult>
  /** 빈 양식 — 있으면 `추가`가 두 쪽 버튼(폼 · Excel 메뉴)이 되고 Excel 항목이 `⋯` 에서 그리로 옮긴다. */
  templateUrl?: string
  /** 가져오기 창의 `?` 본문(시트·필수 열 설명). */
  importHelp?: readonly HelpSection[]
}

/** 툴바 밖(표의 빈 상태 버튼 등)에서 같은 조작을 부르는 손잡이 — 가져오기 창이 둘이 되지 않게. */
export interface RegistryActions {
  importExcel: () => void
}

export interface RegistryToolbarProps<T> {
  /** 좁은 사이드바 — 버튼을 아이콘만으로(글자는 title). */
  compact?: boolean
  /** 행 추가·편집·삭제를 숨긴다(그리드 편집기가 맡을 때). */
  hideCrud?: boolean
  title: string
  icon: React.ReactNode
  what: '셀' | '스테이션' | '품목'
  rows: number
  dirty: number
  selected: boolean
  io: RegistryIo<T>
  onAdd: () => void
  onEdit: () => void
  onDelete: () => void
  /** 편집·삭제가 막힌 이유(선택 없음 등) — `title`로 나간다. */
  disabledReason?: string
  /** 추가·편집·삭제 뒤에 붙는 조작(복제·화면 이동 등). */
  extra?: React.ReactNode
  /** 목록을 다시 받는다(읽기·가져오기·쓰기 뒤). */
  reload: () => Promise<void>
  /** 바깥에서 부를 조작을 여기에 걸어 준다(`RegistryActions`). */
  actions?: React.MutableRefObject<RegistryActions | null>
}

/** 가져오기 결과 한 줄 — 원소가 안 풀리면 오류 수와 첫 사유를 붙인다. */
function importSummary(head: string, r: FileImportResult): string {
  const base = outcomeSummary(head, importOutcome(r))
  if (r.errors.length === 0) return base
  return `${base} · 오류 ${r.errors.length} (${r.errors[0].message})`
}

/** 누른 손잡이 **묶음의 아래 왼쪽**에 메뉴를 연다 — 두 쪽 버튼의 `▾` 에서 열어도 `추가` 아래에 붙는다. */
function openBelow(e: React.MouseEvent<HTMLElement>, items: MenuItem[]): void {
  const r = (e.currentTarget.parentElement ?? e.currentTarget).getBoundingClientRect()
  ctxMenu.show(
    {
      clientX: r.left,
      clientY: r.bottom + 2,
      preventDefault: () => e.preventDefault(),
      stopPropagation: () => e.stopPropagation(),
    },
    items,
  )
}

/** 막지 않는 경고를 한 줄 토스트로 — 첫 건과 나머지 수(가져오기·쓰기 결과에 실려 온다). */
function warnToast(r: { warnings?: string[] }): void {
  const w = r.warnings ?? []
  if (w.length) toast.warn(`${w[0]}${w.length > 1 ? ` 외 ${w.length - 1}건` : ''}`)
}

export function RegistryToolbar<T>({
  title,
  icon,
  what,
  rows,
  dirty,
  selected,
  io,
  onAdd,
  onEdit,
  onDelete,
  disabledReason = '먼저 행을 고르세요',
  extra,
  reload,
  compact = false,
  hideCrud = false,
  actions,
}: RegistryToolbarProps<T>) {
  const hasPlc = !!io.plc
  /** Excel 로 추가하는 레지스트리인가 — `추가`가 두 쪽 버튼이 된다. */
  const addMenu = !!io.templateUrl && !hideCrud
  useStore(plcs, robots)
  useEffect(() => {
    if (!hasPlc) return
    const offPlcs = plcs.start()
    const offRobots = robots.start()
    return () => {
      offPlcs()
      offRobots()
    }
  }, [hasPlc])
  const refs = useMemo(() => s7Plcs(plcs.list), [plcs.list])
  const fallback = defaultTarget(refs, robots.current?.plc) ?? 'GR2'
  const [chosen, setChosen] = useState<PlcTarget | null>(null)
  const plc = effectiveTarget(chosen, refs, fallback)
  /** 읽기·차이는 PLC 한 대만 — '전체'면 기본 대상(선택된 로봇의 PLC)으로 본다. */
  const one = readTarget(plc, fallback)
  const plcLabel = targetLabel(plc, refs)
  const options = refs.length > 0 ? targetOptions(refs) : [{ value: fallback, label: fallback }]

  const [busy, setBusy] = useState<'import' | 'push' | 'diff' | 'file' | null>(null)
  const [pushOpen, setPushOpen] = useState(false)
  const [diff, setDiff] = useState<{
    open: boolean
    rows: DiffRow<T>[] | null
    error: string | null
    plc: string
  }>({ open: false, rows: null, error: null, plc: '' })
  const [imp, setImp] = useState<{
    open: boolean
    file: File | null
    preview: FileImportResult | null
    error: string | null
    applying: boolean
  }>({ open: false, file: null, preview: null, error: null, applying: false })
  const fileRef = useRef<HTMLInputElement>(null)
  // Excel 내보내기는 링크 하나면 되는 일이라 fetch 로 다시 짜지 않는다 — 숨긴 `<a download>`를 누른다.
  const exportRef = useRef<HTMLAnchorElement>(null)
  const templateRef = useRef<HTMLAnchorElement>(null)

  /** 가져오기 창을 비운 채 열고 파일 고르기도 같이 연다 — 고르기를 취소하면 창이 끌어다 놓을 자리로 남는다. */
  function openImport() {
    setImp({ open: true, file: null, preview: null, error: null, applying: false })
    fileRef.current?.click()
  }
  useEffect(() => {
    if (!actions) return
    actions.current = { importExcel: openImport }
    return () => {
      actions.current = null
    }
  })

  async function doImport() {
    if (!io.plc) return
    setBusy('import')
    const tid = toast.pending(`${one}에서 ${what} 읽는 중…`)
    try {
      const r = await io.plc.import(one)
      await reload()
      toast.resolve(tid, r.errors.length ? 'warn' : 'ok', importSummary(`${one} ${what} 읽기 —`, r))
      warnToast(r)
    } catch (e) {
      toast.resolve(
        tid,
        'error',
        `${what} 읽기 실패 — ${e instanceof Error ? e.message : String(e)}`,
      )
    } finally {
      setBusy(null)
    }
  }

  async function doPush(force: boolean) {
    if (!io.plc) return
    setBusy('push')
    const tid = toast.pending(`${plcLabel}에 ${what} 쓰는 중…`)
    try {
      const r = await io.plc.push(plc, force)
      await reload()
      toast.resolve(tid, r.verified ? 'ok' : 'warn', pushSummary(r))
      warnToast(r)
    } catch (e) {
      toast.resolve(
        tid,
        'error',
        `${what} 쓰기 실패 — ${e instanceof Error ? e.message : String(e)}`,
      )
    } finally {
      setBusy(null)
    }
  }

  async function doDiff() {
    if (!io.plc) return
    setBusy('diff')
    setDiff({ open: true, rows: null, error: null, plc: one })
    try {
      const rows = await io.plc.diff(one)
      setDiff({ open: true, rows, error: null, plc: one })
    } catch (e) {
      setDiff({ open: true, rows: [], error: e instanceof Error ? e.message : String(e), plc: one })
    } finally {
      setBusy(null)
    }
  }

  async function pickFile(file: File | null) {
    if (!file) return
    setImp({ open: true, file, preview: null, error: null, applying: false })
    setBusy('file')
    try {
      const preview = await io.importFile(file, true)
      setImp((s) => ({ ...s, preview }))
    } catch (e) {
      setImp((s) => ({ ...s, error: e instanceof Error ? e.message : String(e) }))
    } finally {
      setBusy(null)
    }
  }

  async function applyFile() {
    if (!imp.file) return
    setImp((s) => ({ ...s, applying: true }))
    try {
      const r = await io.importFile(imp.file, false)
      await reload()
      // `품목 12건 추가 · 3건 갱신` — 건너뛴 줄이 있으면 경고색으로 첫 사유까지.
      if (r.errors.length) toast.warn(importSummary(what, r))
      else toast.ok(importSummary(what, r))
      warnToast(r)
      setImp({ open: false, file: null, preview: null, error: null, applying: false })
    } catch (e) {
      setImp((s) => ({ ...s, applying: false, error: e instanceof Error ? e.message : String(e) }))
    }
  }

  // 넘침 메뉴 — 띠에서 내린 조작 전부. 비활성은 회색으로 침묵하지 않고 **이유**를 말한다.
  // PLC 대상은 라디오 대신 목록으로 둔다: 지금 대상은 `지금` 표시가 붙고, 고르면 그 자리에서 바뀐다.
  const menu: MenuEntry[] = [
    !hideCrud && { label: `${what} 조작` },
    !hideCrud && {
      label: '편집…',
      run: onEdit,
      disabled: selected ? undefined : disabledReason,
    },
    !hideCrud && {
      label: '삭제…',
      danger: true,
      run: onDelete,
      disabled: selected ? undefined : disabledReason,
    },
    ...(io.plc
      ? ([
          { label: `PLC 대상 — 읽기·차이는 ${one}` },
          ...options.map((o) => ({
            label: o.label,
            hint: o.value === plc ? '지금' : undefined,
            run: () => setChosen(o.value),
          })),
          {
            label: 'PLC 쓰기…',
            danger: true,
            run: () => setPushOpen(true),
            disabled:
              rows === 0 ? '쓸 행이 없습니다' : busy !== null ? '다른 조작이 도는 중' : undefined,
          },
          {
            label: `차이 보기… (로컬 vs ${one})`,
            run: () => void doDiff(),
            disabled: busy !== null ? '다른 조작이 도는 중' : undefined,
          },
        ] as MenuEntry[])
      : []),
    { label: addMenu ? '목록' : 'Excel · 목록' },
    !addMenu && { label: 'Excel 내보내기', run: () => exportRef.current?.click() },
    !addMenu && {
      label: 'Excel 가져오기…',
      run: openImport,
      disabled: busy !== null ? '다른 조작이 도는 중' : undefined,
    },
    { label: '목록 새로고침', run: () => void reload() },
  ]

  // `추가 ▾` — 행을 만드는 두 길(폼 · Excel)과 Excel 을 쓰는 데 필요한 둘(양식 · 지금 목록).
  const addItems: MenuItem[] = [
    { label: `${what} 추가` },
    { label: '직접 입력…', run: onAdd, testid: 'add-manual' },
    {
      label: 'Excel 가져오기…',
      run: openImport,
      disabled: busy !== null ? '다른 조작이 도는 중' : undefined,
      testid: 'add-excel',
    },
    { label: 'Excel 파일' },
    { label: '양식 받기', run: () => templateRef.current?.click(), testid: 'add-template' },
    {
      label: 'Excel 내보내기',
      run: () => exportRef.current?.click(),
      disabled: rows === 0 ? '내보낼 행이 없습니다 — 양식 받기를 쓰세요' : undefined,
      testid: 'add-export',
    },
  ]

  return (
    <>
      <Toolbar
        icon={icon}
        title={title}
        dense
        meta={
          <span className="tabular-nums">
            {rows}건{dirty ? ` · 수정 ${dirty}` : ''}
          </span>
        }
      >
        {hideCrud ? null : addMenu ? (
          <span className="inline-flex items-center" role="group" aria-label={`${what} 추가`}>
            <Button
              size="sm"
              intent="ghost"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={onAdd}
              className="rounded-r-none"
              data-testid="reg-add"
              title={`${what} 한 건 직접 입력 — 폼이 열립니다 (Excel 은 ▾)`}
            >
              {compact ? null : '추가'}
            </Button>
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<ChevronDown className="h-3.5 w-3.5" />}
              onClick={(e) => openBelow(e, addItems)}
              className="rounded-l-none border-l border-line-subtle"
              aria-label={`${what} 추가 방법`}
              aria-haspopup="menu"
              title="추가 방법 — 직접 입력 · Excel 가져오기 · 양식 받기 · Excel 내보내기"
              data-testid="reg-add-menu"
            />
          </span>
        ) : (
          <Button
            size="sm"
            intent="ghost"
            icon={<Plus className="h-3.5 w-3.5" />}
            onClick={onAdd}
            data-testid="reg-add"
            title={`${what} 한 건 추가 — 폼이 열립니다`}
          >
            {compact ? null : '추가'}
          </Button>
        )}
        {extra}
        {io.plc ? (
          <Button
            size="sm"
            icon={<Download className="h-3.5 w-3.5" />}
            loading={busy === 'import'}
            disabled={busy !== null}
            onClick={() => void doImport()}
            title={`${one} PLC 테이블을 로컬로 읽어 옵니다 (대상은 ⋯ 에서 바꿉니다)`}
            data-testid="reg-plc-read"
          >
            {compact ? null : 'PLC 읽기'}
          </Button>
        ) : null}
        <OverflowMenu
          items={menuItems(menu)}
          title={`${what} 조작 — 편집 · 삭제${io.plc ? ' · PLC 쓰기 · 차이 · 대상' : ''}${addMenu ? '' : ' · Excel'} · 새로고침`}
          testid="reg-more"
        />
        <a ref={exportRef} href={io.exportUrl} download className="hidden" aria-hidden="true">
          Excel
        </a>
        {io.templateUrl ? (
          <a ref={templateRef} href={io.templateUrl} download className="hidden" aria-hidden="true">
            양식
          </a>
        ) : null}
        <input
          ref={fileRef}
          type="file"
          accept={SHEET_ACCEPT}
          className="hidden"
          onChange={(e) => {
            const f = e.currentTarget.files?.[0] ?? null
            e.currentTarget.value = ''
            void pickFile(f)
          }}
        />
      </Toolbar>

      {io.plc ? (
        <>
          <PushDialog
            open={pushOpen}
            onOpenChange={setPushOpen}
            what={what === '품목' ? '셀' : what}
            plc={plcLabel}
            rows={rows}
            dirty={dirty}
            onConfirm={(force) => void doPush(force)}
          />
          <DiffDialog
            open={diff.open}
            onOpenChange={(o) => setDiff((s) => ({ ...s, open: o }))}
            title={`${what} — 로컬 vs ${diff.plc}`}
            rows={diff.rows}
            error={diff.error}
            summarize={io.plc.summarize}
          />
        </>
      ) : null}
      <ImportDialog
        open={imp.open}
        onOpenChange={(o) => setImp((s) => ({ ...s, open: o }))}
        what={what}
        file={imp.file}
        preview={imp.preview}
        error={imp.error}
        applying={imp.applying}
        onPick={() => fileRef.current?.click()}
        onFile={(f) => void pickFile(f)}
        onApply={() => void applyFile()}
        templateUrl={io.templateUrl}
        help={io.importHelp}
      />
    </>
  )
}
