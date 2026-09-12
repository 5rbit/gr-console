// PLC UDT 구조 표 — 멤버 · 타입 · 값 (· 주석). 셀 정보 패널과 계획 스텝 펼침에서 쓴다.
import type { PlcRow } from '../../lib/gr/plcShape'

export function PlcStructView({
  rows,
  title,
  dense = true,
}: {
  rows: readonly PlcRow[]
  title?: string
  dense?: boolean
}) {
  return (
    <div className="rounded border border-slate-200 dark:border-slate-700" data-testid="plc-struct">
      {title ? (
        <div className="border-b border-slate-200 bg-surface-panel px-2 py-1 font-mono text-[11px] font-semibold dark:border-slate-700">
          {title}
        </div>
      ) : null}
      <table className={dense ? 'w-full text-[11px]' : 'w-full text-xs'}>
        <tbody>
          {rows.map((r) => (
            <tr
              key={r.member}
              className="border-b border-slate-100 last:border-0 dark:border-slate-800"
            >
              <td className="px-2 py-0.5 font-mono text-slate-700 dark:text-slate-300">
                {r.member}
              </td>
              <td className="px-1 py-0.5 text-slate-400">{r.type}</td>
              <td className="px-2 py-0.5 text-right font-mono tabular-nums">
                {typeof r.value === 'boolean'
                  ? r.value
                    ? 'TRUE'
                    : 'FALSE'
                  : typeof r.value === 'number'
                    ? Number.isInteger(r.value)
                      ? r.value
                      : r.value.toFixed(1)
                    : r.value}
              </td>
              <td className="px-2 py-0.5 text-slate-400">{r.comment ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
