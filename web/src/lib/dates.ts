import type { IsoDate } from '@/lib/types'

/*
 * Days, as the ledger keeps them: `YYYY-MM-DD`, with no time and no zone.
 *
 * A day is read and written in UTC on purpose. `new Date('2026-10-08')` is
 * midnight UTC, and formatting it in a zone west of Greenwich - Paraguay's -
 * prints the seventh. Pinning both ends to UTC keeps the day the day it was
 * recorded on, wherever the browser is.
 */

/** Today, in the browser's own calendar - the day a person means by "today". */
export function today(): IsoDate {
  const now = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
}

/**
 * A day in a list: the year only when it is not this one. A column of
 * `8 Oct 2026` repeats the same year forty times and pushes the day itself
 * into the shadow of it.
 */
export function formatListDay(day: IsoDate, locale: string, now: IsoDate = today()): string {
  return formatDay(day, locale, day.slice(0, 4) === now.slice(0, 4) ? { year: undefined } : {})
}

/** A day in the interface's language: `Oct 8, 2026` in English, the day first in Russian. */
export function formatDay(day: IsoDate, locale: string, options: Intl.DateTimeFormatOptions = {}): string {
  const date = new Date(`${day}T00:00:00Z`)
  if (Number.isNaN(date.getTime())) return day
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', year: 'numeric', ...options, timeZone: 'UTC' }).format(date)
}
