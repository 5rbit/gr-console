// 맵 더블 클릭 재고 팝업 — 셀·스테이션 공통, 작업자가 몇 초 안에 끝내는 것만 담는다.
//
//   비어 있음 → 품목(최근 쓴 품목 버튼 + 목록) · 개수(기본 1) → Enter
//   재고 있음 → 지금 품목·개수 한 줄 · 개수 −/+ → Enter 로 수량 저장, [비우기] 로 삭제
// 폼 안의 버튼은 모두 type="button" — 킷 Button 은 기본이 submit 이라, 비활성 [−] 가 폼의 기본 버튼이 되면
// Count 에서 Enter 가 제출되지 않는다(2026-09-21 확인).
// 품목 바꾸기·새 품목 등록은 [자세히…] 로 기존 재고 편집 창(`StockEditDialog`)에 넘긴다.
import { useEffect, useRef, useState } from 'react'
import { Minus, Plus, Trash2 } from 'lucide-react'
import { api } from '../../lib/api'
import { itemLabel } from '../../lib/items/model'
import {
  clampCount,
  loadRecent,
  pushRecent,
  quickAction,
  saveRecent,
} from '../../lib/task/quickStockModel'
import type { Item, StockEntry } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { toast } from '../../lib/ui/toast'
import { ItemPicker } from '../shared/ItemPicker'

export interface QuickStockTarget {
  id: number
  kind: 'cell' | 'station'
  stock: StockEntry | null
}

export function QuickStockDialog({
  target,
  items,
  onClose,
  onDetail,
}: {
  target: QuickStockTarget | null
  items: readonly Item[]
  onClose: () => void
  /** [자세히…] — 품목 바꾸기·새 품목 등록(기존 재고 편집 창). */
  onDetail?: (t: QuickStockTarget) => void
}) {
  const cur = target?.stock && target.stock.count > 0 ? target.stock : null
  const mode = cur ? 'edit' : 'add'
  const [item, setItem] = useState<number | null>(null)
  const [count, setCount] = useState(1)
  const [busy, setBusy] = useState(false)
  const [recent, setRecent] = useState<number[]>([])
  const countBox = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!target) return
    const r = loadRecent()
    setRecent(r)
    // 가장 최근 품목을 미리 골라 둔다 — 같은 품목을 연달아 넣을 때 더블 클릭 → Enter 로 끝난다.
    const pre = cur ? null : (r.find((c) => items.some((i) => i.code === c)) ?? null)
    setItem(pre)
    setCount(cur ? cur.count : 1)
    // 첫 포커스: 품목이 정해져 있으면(수정 · 최근 품목) Count, 아니면 품목 목록. 대화상자 기본 포커스(본문 첫
    // 요소 = [−] 버튼)보다 뒤에 돈다 — 버튼에 포커스가 있으면 Enter 가 저장이 아니라 그 버튼을 누른다.
    const t = setTimeout(() => {
      const box = countBox.current?.closest('[role=dialog]')
      if (cur || pre) countBox.current?.querySelector('input')?.select()
      else box?.querySelector<HTMLSelectElement>('select')?.focus()
    }, 30)
    return () => clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 열릴 때만 초기화
  }, [target])
  /** 품목을 고르면 Count 로 — 바로 숫자를 치고 Enter. */
  const pick = (code: number | null) => {
    setItem(code)
    if (code) setTimeout(() => countBox.current?.querySelector('input')?.select(), 0)
  }

  const code = cur?.item_code ?? item
  const it = code ? items.find((i) => i.code === code) : undefined
  const stackMax = it?.spec?.stack_max ?? 0
  const set = (n: number) => setCount(clampCount(n, mode, stackMax))
  const act = quickAction(cur, item, count)
  const place = target ? `${target.kind === 'station' ? '스테이션' : '셀'} #${target.id}` : ''

  async function run(a = act) {
    if (!target || a.kind === 'none') return
    setBusy(true)
    try {
      if (a.kind === 'delete') {
        await api.stockDelete(target.id)
        toast.ok(`${place} 비움`)
      } else {
        await api.stockSet(target.id, { item_code: a.item_code, count: a.count })
        const next = pushRecent(recent, a.item_code)
        saveRecent(next)
        toast.ok(`${place} ${a.item_code} × ${a.count}`)
      }
      onClose()
    } catch (e) {
      toast.error(`재고 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const recentItems = recent
    .map((c) => items.find((i) => i.code === c))
    .filter((i): i is Item => !!i)

  return (
    <FormDialog
      open={!!target}
      onOpenChange={(o) => !o && onClose()}
      title={mode === 'add' ? `${place} · 화물 입력` : `${place} · 재고`}
      size="sm"
      busy={busy}
      submitLabel={mode === 'add' ? '입력' : count === 0 ? '비우기' : '수량 저장'}
      danger={mode === 'edit' && count === 0}
      disabledReason={
        act.kind !== 'none' ? undefined : mode === 'add' ? '품목을 고르세요' : '수량이 그대로입니다'
      }
      onSubmit={() => void run()}
      extra={
        <>
          {mode === 'edit' ? (
            <Button
              size="sm"
              intent="danger"
              icon={<Trash2 className="h-3.5 w-3.5" />}
              onClick={() => void run({ kind: 'delete' })}
              disabled={busy}
              data-testid="quick-stock-delete"
            >
              비우기
            </Button>
          ) : null}
          {onDetail && target ? (
            <Button
              size="sm"
              intent="ghost"
              onClick={() => onDetail(target)}
              data-testid="quick-stock-detail"
            >
              자세히…
            </Button>
          ) : null}
          <span className="flex-1" />
        </>
      }
      testid="quick-stock"
    >
      {mode === 'edit' && cur ? (
        <div className="rounded-md border border-line-default bg-surface-inset px-3 py-2 text-xs">
          <span className="font-semibold text-content-primary tabular-nums">{cur.item_code}</span>
          <span className="text-content-muted"> {it ? itemLabel(it) : '등록되지 않은 품목'}</span>
          <span className="float-right tabular-nums text-content-secondary">
            지금 {cur.count}개
          </span>
        </div>
      ) : (
        // select 안의 Enter 는 브라우저가 폼을 제출하지 않는다 — 여기서 받아 저장한다.
        <div
          className="flex flex-col gap-2"
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'SELECT') {
              e.preventDefault()
              void run()
            }
          }}
        >
          {recentItems.length ? (
            <div className="flex flex-wrap gap-1" data-testid="quick-stock-recent">
              {recentItems.map((i) => (
                <Button
                  key={i.code}
                  type="button"
                  size="sm"
                  intent={item === i.code ? 'primary' : 'outline'}
                  onClick={() => pick(i.code)}
                  data-testid={`quick-stock-recent-${i.code}`}
                >
                  {i.code}
                  <span className="ml-1 max-w-28 truncate font-normal opacity-80">{i.name}</span>
                </Button>
              ))}
            </div>
          ) : null}
          <ItemPicker value={item} onChange={pick} items={[...items]} allowNone={false} />
        </div>
      )}
      <div ref={countBox} className="flex items-center gap-2">
        <span className="w-14 text-xs text-content-muted">Count</span>
        <Button
          type="button"
          // 누르는 동안 포커스를 Count 에 둔다 — 그래야 Enter 가 저장이다(버튼에 있으면 한 번 더 누름).
          tabIndex={-1}
          onMouseDown={(e) => e.preventDefault()}
          size="icon-sm"
          intent="outline"
          icon={<Minus className="h-3.5 w-3.5" />}
          title="하나 빼기"
          onClick={() => set(count - 1)}
          disabled={count <= (mode === 'add' ? 1 : 0)}
          data-testid="quick-stock-minus"
        />
        <Input
          type="number"
          inputMode="numeric"
          mono
          className="w-16 text-center"
          value={String(count)}
          min={mode === 'add' ? 1 : 0}
          max={stackMax > 0 ? stackMax : 99}
          onValueChange={(v) => set(Number(v))}
          onFocus={(e) => e.currentTarget.select()}
          data-testid="quick-stock-count"
        />
        <Button
          type="button"
          // 누르는 동안 포커스를 Count 에 둔다 — 그래야 Enter 가 저장이다(버튼에 있으면 한 번 더 누름).
          tabIndex={-1}
          onMouseDown={(e) => e.preventDefault()}
          size="icon-sm"
          intent="outline"
          icon={<Plus className="h-3.5 w-3.5" />}
          title="하나 더하기"
          onClick={() => set(count + 1)}
          disabled={count >= (stackMax > 0 ? stackMax : 99)}
          data-testid="quick-stock-plus"
        />
        {stackMax > 0 ? (
          <span className="text-2xs text-content-muted">StackMax {stackMax}</span>
        ) : null}
      </div>
    </FormDialog>
  )
}
