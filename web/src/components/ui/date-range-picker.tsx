import { useId, useMemo, useState } from 'react'
import { cn, useLocale } from 'dowel-ui'
import { Button } from './button'
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
 *
 * Given `clearLabel`, the popup offers to take the range back - half made or
 * whole - under the month, the way DatePicker does, and `onValueChange` is
 * told the empty range.
 */

export interface DateRange {
  /** The first day, inclusive. */
  start?: IsoDate
  /** The last day, inclusive. Absent while the range is half made. */
  end?: IsoDate
}

export interface DateRangePickerProps {
  /** The chosen range. Either end may be absent: a start with no end is the range while it is half made. */
  value?: DateRange
  /** Called on both clicks, so a product sees the half-made range as well as the whole one. */
  onValueChange?: (value: DateRange) => void
  /** The words of the button that takes the range back: "Clear". Leave it out
   * and a chosen range can only be replaced, not emptied. */
  clearLabel?: string
  /** The earliest day that can be chosen, as `YYYY-MM-DD`. */
  min?: IsoDate
  /** The latest day that can be chosen, as `YYYY-MM-DD`. */
  max?: IsoDate
  /** What the trigger says when nothing is chosen. */
  placeholder: string
  /** Names the two month-paging buttons inside the calendar. */
  previousMonthLabel: string
  /** Names the button that pages to the next month, for the same reason as `previousMonthLabel`. */
  nextMonthLabel: string
  /** The language the dates and the calendar are written in. Defaults to the locale from `useLocale`. */
  locale?: string
  /** Stops the trigger opening the calendar. */
  disabled?: boolean
  /** Put on the trigger, so a `Field` label can point at it. */
  id?: string
  /** Names the trigger when there is no visible label. The trigger's name is this and then the date it shows. */
  'aria-label'?: string
  /** Points the trigger at its visible label. The date it shows is added to the name. */
  'aria-labelledby'?: string
  /** Points the trigger at a hint or an error message. */
  'aria-describedby'?: string
  /** Marks the trigger as holding a value that is not accepted. */
  'aria-invalid'?: boolean | 'true' | 'false'
  className?: string
}

export function DateRangePicker({
  value,
  onValueChange,
  clearLabel,
  min,
  max,
  placeholder,
  previousMonthLabel,
  nextMonthLabel,
  locale,
  disabled = false,
  className,
  'aria-label': ariaLabel,
  id,
  'aria-labelledby': labelledBy,
  'aria-describedby': describedBy,
  'aria-invalid': invalid,
}: DateRangePickerProps) {
  const [open, setOpen] = useState(false)
  const language = useLocale(locale)
  // Named by its label and then by the date it shows. A label alone would be
  // announced without the value on the button, and a name that does not hold
  // the words on the button fails WCAG 2.5.3: someone saying what they see to
  // voice control says words the name does not have. With an `aria-label` the
  // trigger names itself - an element listed in its own `aria-labelledby` is
  // read by its `aria-label`.
  const ownId = useId()
  const triggerId = id ?? ownId
  const shownId = useId()
  const naming = labelledBy ?? (ariaLabel === undefined ? undefined : triggerId)

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
        id={triggerId}
        aria-labelledby={naming === undefined ? undefined : `${naming} ${shownId}`}
        aria-describedby={describedBy}
        aria-invalid={invalid}
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
        {/* The empty face in `dim`: `faint` at this size measured 3.2:1. */}
        <span id={shownId} data-placeholder={shown === undefined ? '' : undefined} className="truncate data-[placeholder]:text-dim">
          {shown ?? placeholder}
        </span>
      </PopoverTrigger>

      {/* The calendar takes the focus to the day under the cursor itself, so
        * the popup moves it nowhere first. And the popup is a dialog, so it is
        * named - for the field. */}
      <PopoverPopup
        arrow={false}
        className="w-auto p-3"
        initialFocus={false}
        aria-label={labelledBy === undefined ? (ariaLabel ?? placeholder) : undefined}
        aria-labelledby={labelledBy}
      >
        <Calendar
          autoFocus
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
        {clearLabel !== undefined && range.start !== undefined && (
          <div className="mt-2 flex justify-end border-t border-line pt-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                onValueChange?.({})
                setOpen(false)
              }}
            >
              {clearLabel}
            </Button>
          </div>
        )}
      </PopoverPopup>
    </Popover>
  )
}
