// 집계 — 규격(Item Code)별 누적과 종류별 편차 통계. 전에는 탭 둘(`규격별`·`통계`)이었다.
//
// 둘 다 MEASLOG 스냅샷 한 장을 다르게 접은 표이고, 통계 쪽은 다섯 줄짜리라 탭 하나를 혼자 쓰기에는
// 값이 너무 적었다. 위아래로 세우면 "이 규격이 튀는데 종류 전체로도 튀나"를 한 화면에서 본다.
//
// 자리 정리(2026-09-18): 손으로 짠 `<table>` 둘을 `DataTable` 로 바꿨다 — 정렬·좁은 폭 열 접기·빈
// 상태가 공짜로 따라오고, 제목에 얹혀 있던 셀 형식 설명("셀은 평균 / EMA …")과 클릭 힌트는 각각
// `?` 와 행 툴팁으로 내려갔다(제목은 이름만 말한다).
import { KIND } from '../../lib/gr/const'
import { MEAS_KEYS, MEAS_KEY_LABEL } from '../../lib/meas/const'
import { dtl, pos } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import { HelpTip } from '../../lib/ui/HelpTip'
import type { Column } from '../../lib/ui/table'
import type { ByCode, MeasLogSnapshot, MeasStat } from '../../lib/types'
import { TrendCell } from './helpers'

/** 추세 셀 형식 — 제목에 붙어 있던 문장. 표마다 한 번, `?` 뒤에 둔다. */
const CELL_HELP =
  '추세 칸은 평균 / EMA (최소~최대, n) 입니다. 단위는 mm 이고 소수 2자리(편차 규칙)입니다.'

/** 규격별 — 행을 알아보는 Code·치수가 1, 누적·시각이 2, 추세 열 전부가 3(좁으면 행 펼치기로). */
const BY_CODE_COLS: Column<ByCode>[] = [
  { key: 'code', label: 'Code', get: (b) => b.Code, numeric: true, priority: 1 },
  {
    key: 'id',
    label: 'InnerDiameter (mm)',
    get: (b) => pos(b.Item?.InnerDiameter),
    numeric: true,
    priority: 1,
  },
  {
    key: 'od',
    label: 'OuterDiameter (mm)',
    get: (b) => pos(b.Item?.OuterDiameter),
    numeric: true,
    priority: 2,
  },
  { key: 'h', label: 'Height (mm)', get: (b) => pos(b.Item?.Height), numeric: true, priority: 2 },
  { key: 'icnt', label: 'Item.Count', get: (b) => b.Item?.Count, numeric: true, priority: 3 },
  { key: 'cnt', label: 'Count', get: (b) => b.Count, numeric: true, priority: 1 },
  { key: 'last', label: 'LastTime', get: (b) => dtl(b.LastTime), priority: 2 },
  ...MEAS_KEYS.map(
    (k): Column<ByCode> => ({
      key: k,
      label: MEAS_KEY_LABEL[k],
      get: (b) => b.Meas?.[k]?.Avg ?? null,
      cell: (b) => <TrendCell t={b.Meas?.[k]} />,
      numeric: true,
      priority: 3,
    }),
  ),
  {
    key: 'sItemId',
    label: 'Stat[Item].InnerDia',
    get: (b) => b.Stat?.[0]?.InnerDia?.Avg ?? null,
    cell: (b) => <TrendCell t={b.Stat?.[0]?.InnerDia} />,
    numeric: true,
    priority: 3,
  },
  {
    key: 'sItemH',
    label: 'Stat[Item].Height',
    get: (b) => b.Stat?.[0]?.Height?.Avg ?? null,
    cell: (b) => <TrendCell t={b.Stat?.[0]?.Height} />,
    numeric: true,
    priority: 3,
  },
  {
    key: 'sPickZ',
    label: 'Stat[Pick].Z',
    get: (b) => b.Stat?.[3]?.Z?.Avg ?? null,
    cell: (b) => <TrendCell t={b.Stat?.[3]?.Z} />,
    numeric: true,
    priority: 3,
  },
]

/** 종류별 편차 통계 — 행이 다섯뿐이라 전부 우선순위 1·2다. */
const STAT_COLS: Column<StatRow>[] = [
  { key: 'kind', label: 'Kind', get: (r) => r.kind, priority: 1 },
  { key: 'count', label: 'Count', get: (r) => r.s.Count, numeric: true, priority: 1 },
  { key: 'err', label: 'ErrorCount', get: (r) => r.s.ErrorCount, numeric: true, priority: 1 },
  {
    key: 'id',
    label: 'Δ InnerDia',
    get: (r) => r.s.InnerDia?.Avg ?? null,
    cell: (r) => <TrendCell t={r.s.InnerDia} />,
    numeric: true,
    priority: 2,
  },
  {
    key: 'h',
    label: 'Δ Height',
    get: (r) => r.s.Height?.Avg ?? null,
    cell: (r) => <TrendCell t={r.s.Height} />,
    numeric: true,
    priority: 2,
  },
  {
    key: 'z',
    label: 'Δ Z',
    get: (r) => r.s.Z?.Avg ?? null,
    cell: (r) => <TrendCell t={r.s.Z} />,
    numeric: true,
    priority: 2,
  },
  {
    key: 'off',
    label: 'Offset',
    get: (r) => r.s.Offset?.Avg ?? null,
    cell: (r) => <TrendCell t={r.s.Offset} />,
    numeric: true,
    priority: 3,
  },
  {
    key: 'sc',
    label: 'Δ StackCount',
    get: (r) => r.s.StackCount?.Avg ?? null,
    cell: (r) => <TrendCell t={r.s.StackCount} />,
    numeric: true,
    priority: 3,
  },
]

interface StatRow {
  kind: string
  s: MeasStat
}

export function Aggregate({
  snap,
  onPick,
}: {
  snap: MeasLogSnapshot | null
  onPick: (code: number) => void
}) {
  const slots = (snap?.by_code ?? []).filter((b) => b.Code).sort((a, b) => a.Code - b.Code)
  const stat: StatRow[] = (snap?.stat ?? []).map((s, i) => ({ kind: KIND[i + 1] ?? String(i + 1), s }))

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {/* 규격별이 이 보기의 본문이다 — 남는 높이를 전부 먹고, 통계는 제 높이만 쓴다. */}
      <Card padded={false} className="flex min-h-0 flex-1 flex-col">
        <div className="flex flex-none items-center gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">규격별 (ByCode)</h3>
          <HelpTip title="ByCode" text={`${CELL_HELP} 행을 누르면 그 Code 로 이력이 걸립니다.`} />
          <span className="ml-auto text-2xs text-content-faint tabular-nums">{slots.length}</span>
        </div>
        <DataTable
          rows={slots}
          columns={BY_CODE_COLS}
          rowKey={(b) => String(b.Code)}
          onPick={(b) => onPick(b.Code)}
          density="compact"
          stickyHeader
          zebra
          className="min-h-0 flex-1 overflow-auto"
          empty="코드별 기록 없음"
          emptyHint="측정이 쌓이면 규격(Item Code)별로 여기에 모입니다."
        />
      </Card>

      <Card padded={false} className="flex-none">
        <div className="flex items-center gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">종류별 편차 (Stat[1..5])</h3>
          <HelpTip title="Stat" text={CELL_HELP} />
        </div>
        <DataTable
          rows={stat}
          columns={STAT_COLS}
          rowKey={(r) => r.kind}
          density="compact"
          empty="통계 없음"
        />
      </Card>
    </div>
  )
}
