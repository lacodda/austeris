import { useMemo, useState } from 'react'
import { cn, useLocale } from 'dowel-ui'
import { fieldClasses } from './input'
import { Popover, PopoverPopup, PopoverTrigger } from './popover'
import { Calendar } from './calendar'
import { isIsoDate, type IsoDate } from './calendar-math'

/*
 * DateRangePicker - two days, chosen in two clicks.
 *
 * The interesting part is the state between them. After the first click there
 * is a start and no end, and that is not an incomplete range to be hidden or
 * a range of one day - it is the normal middle of the interaction, and the
 * calendar has to show it: the first day marked, the days under the pointer
 * shading as the reader moves, the popup staying open. Products that skip it
 * end up with a picker that seems to do nothing until the second click.
 *
 * So the value is a pair where either end may be absent, and the component is
 * explicit about which half it is waiting for. `onValueChange` fires on both
 * clicks - a product watching it sees the half-made range, which is what lets
 * it show "from 2 September" while the reader is still deciding.
 *
 * The second click can land before the first. Clicking the 20th and then the
 * 10th means the 10th to the 20th, because that is plainly what was meant;
 * refusing it would be correct and unhelpful.
 */

export interface DateRange {
  /** The first day, inclusive. */
  start?: IsoDate
  /** The last day, inclusive. Absent while the range is half made. */
  end?: IsoDate
}

export interface DateRangePickerProps {
  value?: DateRange
  onValueChange?: (value: DateRange) => void
  min?: IsoDate
  max?: IsoDate
  /** What the trigger says when nothing is chosen. */
  placeholder: string
  /** Names the two month-paging buttons inside the calendar. */
  previousMonthLabel: string
  nextMonthLabel: string
  locale?: string
  disabled?: boolean
  'aria-label'?: string
  className?: string
}

export function DateRangePicker({
  value,
  onValueChange,
  min,
  max,
  placeholder,
  previousMonthLabel,
  nextMonthLabel,
  locale,
  disabled = false,
  className,
  'aria-label': ariaLabel,
}: DateRangePickerProps) {
  const [open, setOpen] = useState(false)
  const language = useLocale(locale)

  const range = value ?? {}
  const waitingForEnd = range.start !== undefined && range.end === undefined

  const shown = useMemo(() => {
    const write = (date: IsoDate) => {
      const [year, month, day] = date.split('-').map(Number) as [number, number, number]
      return new Intl.DateTimeFormat(language, { dateStyle: 'medium' }).format(
        new Date(year, month - 1, day),
      )
    }
    if (range.start === undefined || !isIsoDate(range.start)) return undefined
    if (range.end === undefined) return write(range.start)
    // An en dash rather than a hyphen: this is a span, and the two read
    // differently at a glance in a row of dates.
    return `${write(range.start)} – ${write(range.end)}`
  }, [range.start, range.end, language])

  const choose = (date: IsoDate) => {
    // A fresh click starts a new range whenever there is nothing waiting -
    // including right after a completed one, which is what a reader means by
    // clicking again.
    if (!waitingForEnd) {
      onValueChange?.({ start: date })
      return
    }

    const start = range.start!
    // Backwards is fine: the reader plainly meant the span between them.
    const next: DateRange = date < start ? { start: date, end: start } : { start, end: date }
    onValueChange?.(next)
    setOpen(false)
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={disabled}
        aria-label={ariaLabel}
        className={cn(
          fieldClasses,
          'flex h-control cursor-pointer items-center gap-2 text-left',
          'disabled:cursor-not-allowed',
          className,
        )}
      >
        <svg viewBox="0 0 16 16" className="size-4 shrink-0 text-faint" fill="none" aria-hidden>
          <rect x="2" y="3" width="12" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
          <path d="M2 6.5h12M5.5 2v2M10.5 2v2" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
        </svg>
        <span className={cn('truncate', shown === undefined && 'text-faint')}>
          {shown ?? placeholder}
        </span>
      </PopoverTrigger>

      <PopoverPopup arrow={false} className="w-auto p-3">
        <Calendar
          value={range.start}
          rangeEnd={range.end}
          min={min}
          max={max}
          locale={language}
          aria-label={ariaLabel ?? placeholder}
          previousMonthLabel={previousMonthLabel}
          nextMonthLabel={nextMonthLabel}
          onValueChange={choose}
        />
      </PopoverPopup>
    </Popover>
  )
}
