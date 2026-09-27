// 인터록 보기의 조작 — Station(레지스트리) · 창(최근 10분 · 1시간 · 시각 ±5분) · 축소.
import { useEffect, useState } from 'react'
import { ZoomOut } from 'lucide-react'
import { api } from '../../lib/api'
import { localInputToMs, msToLocalInput } from '../../lib/evtlog/evtFilterModel'
import {
  SWIM_HALF_MS,
  aroundRange,
  swimRange,
  type SwimState,
} from '../../lib/evtlog/swimlaneModel'
import { Button } from '../../lib/ui/Button'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'

const MAX_WINDOW_MS = 24 * 3_600_000

function short(ms: number): string {
  return msToLocalInput(ms).slice(5, 19).replace('T', ' ')
}

export function SwimControls({
  value,
  onChange,
}: {
  value: SwimState
  onChange: (s: SwimState) => void
}) {
  const [stations, setStations] = useState<number[] | null>(null)
  const [open, setOpen] = useState(false)
  const [at, setAt] = useState('')
  const [half, setHalf] = useState(String(SWIM_HALF_MS / 60_000))

  useEffect(() => {
    let alive = true
    api
      .stations()
      .then((l) => {
        if (alive) setStations(l.map((s) => s.id).sort((a, b) => a - b))
      })
      .catch(() => {
        if (alive) setStations([])
      })
    return () => {
      alive = false
    }
  }, [])

  const live = value.from === null && value.to === null
  const winId = live ? '10m' : 'custom'
  const cur = swimRange(value, Date.now())

  function pickWindow(v: string) {
    if (v === '10m') onChange({ ...value, from: null, to: null })
    else if (v === '1h') {
      const now = Date.now()
      onChange({ ...value, from: now - 3_600_000, to: now })
    } else if (v === 'at') {
      setAt(msToLocalInput(live ? Date.now() - SWIM_HALF_MS : (cur.from + cur.to) / 2))
      setOpen(true)
    }
  }

  function zoomOut() {
    const mid = (cur.from + cur.to) / 2
    const span = Math.min(MAX_WINDOW_MS, (cur.to - cur.from) * 3)
    onChange({ ...value, from: Math.round(mid - span / 2), to: Math.round(mid + span / 2) })
  }

  const atMs = localInputToMs(at)
  const halfMin = Number(half)
  const why =
    atMs === null
      ? '시각을 적으세요'
      : !Number.isFinite(halfMin) || halfMin <= 0 || halfMin > 720
        ? '± 분은 0 보다 크고 720 이하입니다'
        : undefined

  // 이벤트·슬롯으로 들어와 아직 스테이션을 모르면 그 값을 목록에 잠깐 보인다
  const stationValue = value.station !== null ? String(value.station) : ''
  const known = stations ?? []

  return (
    <>
      <Select
        dense
        aria-label="Station"
        title="Station Id (레지스트리)"
        value={stationValue}
        onValueChange={(v) =>
          onChange({ ...value, station: v ? Number(v) : null, slot: null, event: null })
        }
        data-testid="evt-swim-station"
      >
        <option value="">
          {value.slot !== null
            ? `slot ${value.slot}`
            : value.event !== null
              ? `event ${value.event}`
              : 'Station'}
        </option>
        {value.station !== null && !known.includes(value.station) ? (
          <option value={value.station}>{value.station}</option>
        ) : null}
        {known.map((id) => (
          <option key={id} value={id}>
            {id}
          </option>
        ))}
      </Select>
      <Select
        dense
        aria-label="창"
        value={winId}
        onValueChange={pickWindow}
        data-testid="evt-swim-window"
      >
        <option value="10m">최근 10분</option>
        <option value="1h">최근 1시간</option>
        {live ? null : (
          <option value="custom">{`${short(cur.from)} ~ ${short(cur.to).slice(6)}`}</option>
        )}
        <option value="at">시각 ± 분…</option>
      </Select>
      <Button
        size="icon-sm"
        intent="ghost"
        icon={<ZoomOut size={14} />}
        aria-label="축소"
        title="창을 세 배로 넓힌다 (끌어서 고르면 확대)"
        onClick={zoomOut}
        data-testid="evt-swim-zoomout"
      />
      <FormDialog
        open={open}
        onOpenChange={setOpen}
        title="시각 ± 분"
        size="sm"
        submitLabel="적용"
        disabledReason={why}
        onSubmit={() => {
          if (atMs === null) return
          onChange({ ...value, ...aroundRange(atMs, halfMin * 60_000) })
          setOpen(false)
        }}
        testid="evt-swim-at"
      >
        <Input type="datetime-local" step={1} label="at" value={at} onValueChange={setAt} />
        <Input label="± (min)" inputMode="numeric" value={half} onValueChange={setHalf} />
      </FormDialog>
    </>
  )
}
