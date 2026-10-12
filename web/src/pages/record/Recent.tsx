import { useMemo } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Repeat } from 'lucide-react'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { RowButton } from '@/components/ui/list-row'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { addDays } from '@/components/ui/calendar-math'
import { formatDay, today } from '@/lib/dates'
import { categoriesOf, isExchange, movements } from '@/lib/entries'
import { categoryLabels, keys, useAccounts, useBooksChanged, useCategories } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import { useRecordedToast, useSecondPress } from '@/lib/recording'
import type { Entry, IsoDate } from '@/lib/types'
import { describe } from '@/pages/entries/ExchangeForm'
import { title } from '@/pages/entries/EntryDetail'

/** How many of a day's entries are listed; a day of more is the Entries screen's. */
const DAY_LIMIT = 30

interface RecentProps {
  /** The day a repeat is recorded on. */
  day: IsoDate
  onOpen: (id: string) => void
}

/**
 * What was recorded today, and on the last day before it - usually yesterday
 * - each with a button that records it again.
 *
 * Today's list is the receipt for what the screen just did: a line that
 * meant something else shows up here, a press away from its card. The day
 * before is the day most worth repeating: the same bus, the same lunch.
 */
export function Recent({ day, onOpen }: RecentProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const now = today()
  const todays = useQuery({
    queryKey: [...keys.all, 'recent', 'today', now],
    queryFn: () => api.entries({ from: now, to: now }, DAY_LIMIT, 0),
  })
  // The newest day before today that has anything on it: after a weekend
  // without the phone, "yesterday" is Friday.
  const before = useQuery({
    queryKey: [...keys.all, 'recent', 'before', now],
    queryFn: async () => {
      const rows = await api.entries({ to: addDays(now, -1) }, DAY_LIMIT, 0)
      const last = rows[0]?.occurred_on
      return { day: last, rows: rows.filter((entry) => entry.occurred_on === last) }
    },
  })
  const yesterday = addDays(now, -1)

  return (
    <>
      <DayList heading={t('record.today')} entries={todays.data} empty={t('record.nothingToday')} day={day} onOpen={onOpen} />
      {before.data?.day ? (
        <DayList
          heading={before.data.day === yesterday ? t('record.yesterday') : formatDay(before.data.day, locale)}
          entries={before.data.rows}
          day={day}
          onOpen={onOpen}
        />
      ) : null}
    </>
  )
}

interface DayListProps {
  heading: string
  entries: Entry[] | undefined
  empty?: string
  day: IsoDate
  onOpen: (id: string) => void
}

function DayList({ heading, entries, empty, day, onOpen }: DayListProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const categories = useCategories()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const names = useMemo(() => new Map((accounts.data ?? []).map((account) => [account.id, account.name])), [accounts.data])
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const recorded = useRecordedToast()
  const secondPress = useSecondPress()

  const repeat = useMutation({
    mutationFn: ({ entry, again }: { entry: Entry; again: boolean }) => api.repeat(entry.id, day, again),
    onSuccess: (done) => {
      void changed()
      recorded(
        done.entry,
        t('record.repeated', { name: title(done.entry, t('entry.untitled')) }),
        done.conversion ? describe(done.conversion, locale, t) : undefined,
      )
    },
    onError: (error: Error, sent) => {
      if (!secondPress.ask(error, () => repeat.mutate({ ...sent, again: true }))) {
        toasts.add({ type: 'error', title: problem(error, t) })
      }
    },
  })

  if (entries === undefined) return null
  return (
    <section aria-label={heading} className="flex flex-col gap-1">
      <h2 className="caption">{heading}</h2>
      {entries.length === 0 ? (
        <p className="text-sm text-dim">{empty}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-line">
          {entries.map((entry) => {
            const name = title(entry, t('entry.untitled'))
            const moved = movements(entry)
            const where = [
              categoriesOf(entry)
                .map((id) => labels.get(id))
                .filter(Boolean)
                .join(', '),
              moved.map((movement) => names.get(movement.accountId)).join(' → '),
            ].filter(Boolean)
            return (
              <li key={entry.id} className="flex items-center gap-1 py-1">
                {/* The entry opens from the row, a real button; the repeat
                    beside it is a button of its own, not a target inside a
                    target a tap meant for the card could hit. */}
                <RowButton
                  className="flex-1 px-1"
                  onClick={() => onOpen(entry.id)}
                  description={where.join(' · ')}
                  end={
                    <span className="flex flex-col items-end">
                      {moved.map((movement) => (
                        <Money
                          key={movement.accountId}
                          amount={movement.amount}
                          currency={movement.currency}
                          signDisplay="exceptZero"
                          className="text-sm text-text"
                        />
                      ))}
                    </span>
                  }
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <span className="truncate">{name}</span>
                    {entry.status === 'pending' ? <Badge variant="warn">{t('entry.heldBadge')}</Badge> : null}
                  </span>
                </RowButton>
                {isExchange(entry) ? (
                  // Keeps the amounts in one column with the rows that have
                  // a repeat button.
                  <span className="size-8 shrink-0" />
                ) : (
                  <Button
                    variant="icon"
                    size="icon-md"
                    aria-label={t('record.repeat', { name })}
                    disabled={repeat.isPending}
                    onClick={() => repeat.mutate({ entry, again: false })}
                  >
                    <Repeat aria-hidden />
                  </Button>
                )}
              </li>
            )
          })}
        </ul>
      )}
      {secondPress.dialog}
    </section>
  )
}
