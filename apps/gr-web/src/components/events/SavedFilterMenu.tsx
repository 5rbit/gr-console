// 저장된 필터 — 이름 붙인 화면 상태(`e.*` 쿼리)를 서버에 두고 드롭다운으로 연다. 옆의 링크 버튼은 지금 화면의 주소.
import { useCallback, useState } from 'react'
import { Bookmark, ChevronDown, Link2, X } from 'lucide-react'
import { copyText } from '../../lib/clipboard'
import type { SavedFilter } from '../../lib/evtlog/api'
import { Button } from '../../lib/ui/Button'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { toast } from '../../lib/ui/toast'
import { useDismiss } from '../../lib/ui/useDismiss'
import { cn } from '../../lib/utils'

export function SavedFilterMenu({
  saved,
  current,
  onApply,
  onSave,
  onDelete,
}: {
  saved: readonly SavedFilter[]
  /** 지금 화면의 `e.*` 쿼리. */
  current: string
  onApply: (f: SavedFilter) => void
  onSave: (name: string) => Promise<unknown>
  onDelete: (f: SavedFilter) => Promise<unknown>
}) {
  const [open, setOpen] = useState(false)
  const [naming, setNaming] = useState(false)
  const [name, setName] = useState('')
  const [confirmDel, setConfirmDel] = useState<number | null>(null)
  const close = useCallback(() => {
    setOpen(false)
    setConfirmDel(null)
  }, [])
  const root = useDismiss<HTMLDivElement>(open, close)
  const active = saved.find((f) => f.query === current)

  async function copyLink() {
    const ok = await copyText(location.href)
    if (ok) toast.ok('링크를 복사했습니다')
    else toast.warn('복사하지 못했습니다 — 주소창의 주소를 쓰세요')
  }

  return (
    <>
      <div ref={root} className="relative">
        <Button
          size="sm"
          intent={active ? 'outline' : 'ghost'}
          icon={<Bookmark size={12} />}
          aria-expanded={open}
          title={active ? `저장된 필터: ${active.name}` : '저장된 필터'}
          onClick={() => setOpen((o) => !o)}
          data-testid="evt-saved"
        >
          <span className="max-w-28 truncate">{active?.name ?? '필터'}</span>
          <ChevronDown size={12} className="opacity-60" />
        </Button>
        {open ? (
          <div
            role="menu"
            aria-label="저장된 필터"
            className="absolute top-8 right-0 z-30 max-h-80 w-60 overflow-y-auto rounded-lg border border-line-default bg-surface-panel py-1 text-xs shadow-lg"
            data-testid="evt-saved-menu"
          >
            {saved.map((f) => (
              <div key={f.id} className="flex items-center hover:bg-surface-inset">
                <button
                  type="button"
                  role="menuitem"
                  className={cn(
                    'min-w-0 flex-1 truncate px-2.5 py-1 text-left',
                    f.query === current && 'text-accent-text',
                  )}
                  title={f.query || '(조건 없음)'}
                  onClick={() => {
                    onApply(f)
                    close()
                  }}
                >
                  {f.name}
                </button>
                <button
                  type="button"
                  className={cn(
                    'mr-1 rounded px-1 text-2xs',
                    confirmDel === f.id
                      ? 'text-fault-fg'
                      : 'text-content-faint hover:text-content-secondary',
                  )}
                  aria-label={`${f.name} 삭제`}
                  title={confirmDel === f.id ? '한 번 더 누르면 삭제' : '삭제'}
                  onClick={() => {
                    if (confirmDel === f.id) {
                      setConfirmDel(null)
                      void onDelete(f).catch((e: unknown) => toast.error(String(e)))
                    } else setConfirmDel(f.id)
                  }}
                >
                  {confirmDel === f.id ? '삭제' : <X size={12} />}
                </button>
              </div>
            ))}
            {saved.length === 0 ? (
              <div className="px-2.5 py-1 text-content-faint">저장된 필터 없음</div>
            ) : null}
            <div className="my-1 border-t border-line-subtle" />
            <button
              type="button"
              role="menuitem"
              className="w-full px-2.5 py-1 text-left hover:bg-surface-inset"
              onClick={() => {
                setName(active?.name ?? '')
                setNaming(true)
                close()
              }}
              data-testid="evt-saved-save"
            >
              현재 조건 저장…
            </button>
          </div>
        ) : null}
      </div>
      <Button
        size="icon-sm"
        intent="ghost"
        icon={<Link2 size={14} />}
        aria-label="링크 복사"
        title="링크 복사 — 이 주소를 열면 같은 보기·조건으로 열립니다"
        onClick={() => void copyLink()}
        data-testid="evt-copy-link"
      />
      <FormDialog
        open={naming}
        onOpenChange={setNaming}
        title="현재 조건 저장"
        size="sm"
        submitLabel="저장"
        disabledReason={name.trim() ? undefined : '이름을 적으세요'}
        onSubmit={() => {
          void onSave(name.trim())
            .then(() => {
              toast.ok(`필터 저장: ${name.trim()}`)
              setNaming(false)
            })
            .catch((e: unknown) => toast.error(e instanceof Error ? e.message : String(e)))
        }}
        testid="evt-saved-dialog"
      >
        <Input label="Name" value={name} onValueChange={setName} hint="같은 이름이면 덮어씁니다" />
      </FormDialog>
    </>
  )
}
