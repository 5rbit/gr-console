// 통계 탭 — MEASLOG.Stat[1..5] 종류별 편차 통계.
import { KIND } from '../../lib/gr/const'
import type { MeasLogSnapshot } from '../../lib/types'
import { TrendCell } from './helpers'

export function Stats({ snap }: { snap: MeasLogSnapshot | null }) {
  const stat = snap?.stat ?? []
  return (
    <div className="overflow-auto rounded border border-line-default">
      <table className="w-full text-xs">
        <thead className="bg-surface-panel">
          <tr className="text-content-muted">
            {['종류', '기록', '에러', 'Δ내경', 'Δ높이', 'ΔZ', '편심', 'Δ단수'].map((h, i) => (
              <th key={h} className={`px-1.5 py-1 ${i === 0 ? 'text-left' : 'text-right'}`}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {stat.map((s, i) => (
            <tr key={i} className="border-t border-line-default">
              <td className="px-1.5 py-1 font-semibold">{KIND[i + 1]}</td>
              <td className="px-1.5 text-right font-mono tabular-nums">{s.Count}</td>
              <td className="px-1.5 text-right font-mono tabular-nums">{s.ErrorCount}</td>
              <td className="px-1.5 text-right align-top"><TrendCell t={s.InnerDia} /></td>
              <td className="px-1.5 text-right align-top"><TrendCell t={s.Height} /></td>
              <td className="px-1.5 text-right align-top"><TrendCell t={s.Z} /></td>
              <td className="px-1.5 text-right align-top"><TrendCell t={s.Offset} /></td>
              <td className="px-1.5 text-right align-top"><TrendCell t={s.StackCount} /></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
