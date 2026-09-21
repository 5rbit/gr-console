// DragDir 고르기 — 3×3 칸, 사양서 범례와 같은 배치(현재 보기 방향). 칸 = 드래그 변위가 가리키는 쪽,
// 가운데 = 0(드래그 없음). 각 칸에 코드와 PLC DragType 바이트(hex)를 같이 적는다.
import { dirPickerGrid } from '../../lib/pallet/editorModel'
import { dragTypeByte, hexByte, type ScreenAxes } from '../../lib/pallet/model'
import { cn } from '../../lib/utils'

export interface DirPickerProps {
  axes: ScreenAxes
  value: number
  onChange: (dir: number) => void
  disabled?: boolean
}

export function DirPicker({ axes, value, onChange, disabled = false }: DirPickerProps) {
  const grid = dirPickerGrid(axes)
  return (
    <div role="radiogroup" aria-label="DragDir" className="grid w-44 grid-cols-3 gap-1" data-testid="pallet-dir-picker">
      {grid.flat().map((d) => {
        const on = d === value
        return (
          <button
            key={d}
            type="button"
            role="radio"
            aria-checked={on}
            disabled={disabled}
            onClick={() => onChange(d)}
            title={d === 0 ? 'DragDir 0 — 드래그 없음' : `DragDir ${d} · DragType ${hexByte(dragTypeByte(d))}`}
            className={cn(
              'flex h-12 flex-col items-center justify-center rounded border font-mono tabular-nums leading-tight disabled:opacity-50',
              on
                ? 'border-accent bg-accent text-content-on-accent'
                : 'border-line-default bg-surface-inset text-content-primary hover:bg-surface-active',
            )}
            data-testid={`pallet-dir-${d}`}
          >
            <span className="text-xs">{d}</span>
            <span className={cn('text-3xs', on ? 'text-content-on-accent' : 'text-content-faint')}>
              {d === 0 ? '없음' : hexByte(dragTypeByte(d))}
            </span>
          </button>
        )
      })}
    </div>
  )
}
