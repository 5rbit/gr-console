// 일일 보고서 — 날짜 하나를 골라 xlsx 로 받는다(Summary · Alarms · Steps · Events · Tasks).
import { useEffect, useState } from 'react'
import { evtApi } from '../../lib/evtlog/api'
import { localDate } from '../../lib/evtlog/evtRowsModel'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'

export function ReportDialog({
  open,
  onOpenChange,
  onDownload,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onDownload: (url: string) => void
}) {
  const [date, setDate] = useState('')

  useEffect(() => {
    if (open) setDate(localDate(Date.now()))
  }, [open])

  const why = /^\d{4}-\d{2}-\d{2}$/.test(date) ? undefined : '날짜를 고르세요'

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title="일일 보고서"
      size="sm"
      submitLabel="내려받기"
      disabledReason={why}
      onSubmit={() => {
        onDownload(evtApi.reportUrl(date))
        onOpenChange(false)
      }}
      testid="evt-report"
    >
      <Input
        type="date"
        label="date"
        value={date}
        max={localDate(Date.now())}
        onValueChange={setDate}
        hint="그날 00:00 ~ 24:00 (로컬)"
      />
    </FormDialog>
  )
}
