// 표시 인프라 호스트 — panels 스택을 모드별 크롬으로 렌더한다(App에 1회 마운트). 팝업=중앙 모달, 사이드바=우측
// 드로어, 페이지=전체 오버레이. Escape·백드롭(버튼)은 최상단만 닫는다. 컴포넌트는 props와 함께 동적 렌더.
//
// sh4w-web의 PanelHost에서 **별 창으로 떼기(pop-out)** 를 뺐다 — gr-web에는 detach 레지스트리가 없다.
import { useEffect } from 'react'
import { X } from 'lucide-react'
import { panels } from '../lib/panels'
import type { PanelSpec } from '../lib/panels'
import { useStore } from '../lib/store'
import { ErrorBoundary } from '../lib/ui/ErrorBoundary'
import { useFocusTrap } from '../lib/ui/focusTrap'

const headerCls =
  'flex items-center gap-2 border-b border-slate-200 px-3 py-2 dark:border-slate-700'

function PanelHeader({ panel }: { panel: PanelSpec }) {
  return (
    <header className={headerCls}>
      <h3 className="truncate text-sm font-semibold">{panel.title}</h3>
      <button
        className="ml-auto text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
        onClick={() => panels.close(panel.id)}
        aria-label="닫기"
      >
        <X className="h-4 w-4" />
      </button>
    </header>
  )
}

/** 패널 1건의 크롬 — 포커스 트랩이 훅이라 스택 항목마다 컴포넌트로 쪼갠다(반복 안에서 훅을 못 부른다). */
function PanelFrame({ panel }: { panel: PanelSpec }) {
  const box = useFocusTrap<HTMLDivElement>(true)
  const Comp = panel.component
  const body = (
    <ErrorBoundary label={panel.title} resetKey={panel.props}>
      <Comp {...(panel.props ?? {})} />
    </ErrorBoundary>
  )

  if (panel.mode === 'popup') {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <button
          className="absolute inset-0 bg-black/40"
          onClick={() => panels.close(panel.id)}
          aria-label="배경 닫기"
          tabIndex={-1}
        ></button>
        <div
          ref={box}
          className="relative flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900"
          role="dialog"
          aria-modal="true"
          aria-label={panel.title}
          tabIndex={-1}
        >
          <PanelHeader panel={panel} />
          <div className="min-h-0 flex-1 overflow-auto p-3">{body}</div>
        </div>
      </div>
    )
  }

  if (panel.mode === 'sidebar') {
    return (
      <div className="fixed inset-0 z-50">
        <button
          className="absolute inset-0 bg-black/30"
          onClick={() => panels.close(panel.id)}
          aria-label="배경 닫기"
          tabIndex={-1}
        ></button>
        {/* 사이드바·페이지도 모달이다(백드롭이 뒤를 막는다) — `aria-modal`을 popup과 같이 단다. */}
        <div
          ref={box}
          className="absolute inset-y-0 right-0 flex w-full max-w-md flex-col border-l border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900"
          role="dialog"
          aria-modal="true"
          aria-label={panel.title}
          tabIndex={-1}
        >
          <PanelHeader panel={panel} />
          <div className="min-h-0 flex-1 overflow-auto p-3">{body}</div>
        </div>
      </div>
    )
  }

  return (
    <div
      className="fixed inset-0 z-40 flex flex-col bg-white dark:bg-slate-900"
      ref={box}
      role="dialog"
      aria-modal="true"
      aria-label={panel.title}
    >
      <PanelHeader panel={panel} />
      <div className="min-h-0 flex-1 overflow-auto p-4">{body}</div>
    </div>
  )
}

export function PanelHost() {
  useStore(panels)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && panels.stack.length) {
        e.preventDefault()
        panels.closeTop()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <>
      {panels.stack.map((panel) => (
        <PanelFrame key={panel.id} panel={panel} />
      ))}
    </>
  )
}
