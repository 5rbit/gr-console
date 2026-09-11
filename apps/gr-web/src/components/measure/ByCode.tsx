// 규격별 탭 — ByCode 슬롯(동일 Item Code 누적). 행 클릭 → Code 필터.
import { dtl, f1 } from '../../lib/meas/format'
import type { MeasLogSnapshot } from '../../lib/types'
import { TrendCell } from './helpers'

const HEAD = ['Code', '사양 ID', '사양 OD', '사양 H', '단', '기록', '마지막', '내경(정합)', '레이저 내경', '토크 내경', '상단비드', '타이어높이', '편심', 'Pick 비드-Z', 'Pick 내경', 'Sku 단당', 'Sku 스택', 'Δ내경 Item', 'Δ높이 Item', 'ΔZ Pick']

export function ByCode({ snap, onPick }: { snap: MeasLogSnapshot | null; onPick: (code: number) => void }) {
  const slots = (snap?.by_code ?? []).filter((b) => b.Code).sort((a, b) => a.Code - b.Code)
  return (
    <div>
      <div className="max-h-[70vh] overflow-auto rounded border border-line-default">
        <table className="w-full text-xs">
          <thead className="sticky top-0 bg-surface-panel">
            <tr className="text-content-muted">
              {HEAD.map((h, i) => (
                <th key={h} className={`px-1.5 py-1 ${i === 0 || i === 6 ? 'text-left' : 'text-right'}`}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {slots.length === 0 ? (
              <tr>
                <td colSpan={HEAD.length} className="px-2 py-3 text-content-muted">
                  코드별 기록 없음
                </td>
              </tr>
            ) : (
              slots.map((b) => (
                <tr key={b.Code} className="cursor-pointer border-t border-line-default hover:bg-slate-50 dark:hover:bg-slate-800/60" onClick={() => onPick(b.Code)}>
                  <td className="px-1.5 py-1 font-mono tabular-nums">{b.Code}</td>
                  <td className="px-1.5 text-right font-mono tabular-nums">{f1(b.Item?.InnerDiameter)}</td>
                  <td className="px-1.5 text-right font-mono tabular-nums">{f1(b.Item?.OuterDiameter)}</td>
                  <td className="px-1.5 text-right font-mono tabular-nums">{f1(b.Item?.Height)}</td>
                  <td className="px-1.5 text-right font-mono tabular-nums">{b.Item?.Count}</td>
                  <td className="px-1.5 text-right font-mono tabular-nums">{b.Count}</td>
                  <td className="px-1.5">{dtl(b.LastTime)}</td>
                  {(['InnerDia', 'LaserInnerDia', 'TorqInnerDia', 'UpperBeadHeight', 'TireHeight', 'Offset', 'PickBeadPos', 'PickInnerDia', 'SkuEachHeight', 'SkuStackHeight'] as const).map((k) => (
                    <td key={k} className="px-1.5 text-right align-top">
                      <TrendCell t={b.Meas?.[k]} />
                    </td>
                  ))}
                  <td className="px-1.5 text-right align-top">
                    <TrendCell t={b.Stat?.[0]?.InnerDia} />
                  </td>
                  <td className="px-1.5 text-right align-top">
                    <TrendCell t={b.Stat?.[0]?.Height} />
                  </td>
                  <td className="px-1.5 text-right align-top">
                    <TrendCell t={b.Stat?.[3]?.Z} />
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
      <div className="mt-1 text-2xs text-content-muted">각 셀 = 평균 / EMA (최소 ~ 최대, n). 행을 클릭하면 해당 Code 로 필터됩니다.</div>
    </div>
  )
}
