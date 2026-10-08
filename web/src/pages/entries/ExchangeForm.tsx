import { useMemo, useState, type FormEvent } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { AmountInput } from '@/components/AmountInput'
import type { Picked } from '@/components/Picker'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { DatePicker } from '@/components/ui/date-picker'
import { DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatDay, today } from '@/lib/dates'
import { negate, parseAmount, sign, sum } from '@/lib/decimal'
import { keys, openAccounts, useAccounts, useBooksChanged } from '@/lib/ledger'
import { formatMoney, formatRate } from '@/lib/money'
import type { ConversionSummary, PlaceBody } from '@/lib/types'
import { AccountSelect, CounterpartyField, PlaceField } from '@/pages/entries/fields'
import { resolver } from '@/pages/entries/resolve'

/**
 * Money changed from one currency into another: what left one account and
 * what arrived in the other, both as the receipt says them.
 *
 * The rate is not typed - the two amounts are the deal, and the rate they
 * imply is worked out by the ledger. What the form does show is the day's
 * official rate, so the person can see before saving whether the exchange
 * office's board was far from it; after saving, the ledger says what the
 * difference cost.
 */
export function ExchangeForm({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const accounts = useAccounts()
  const usable = useMemo(() => openAccounts(accounts.data), [accounts.data])

  const [fromId, setFromId] = useState<string | undefined>()
  const [toId, setToId] = useState<string | undefined>()
  const [given, setGiven] = useState('')
  const [got, setGot] = useState('')
  const [date, setDate] = useState(today())
  const [counterparty, setCounterparty] = useState<Picked | null>(null)
  const [place, setPlace] = useState<PlaceBody | null>(null)
  const [problem, setProblem] = useState<string | null>(null)

  const from = usable.find((account) => account.id === fromId)
  const to = usable.find((account) => account.id === toId)
  const pair = from && to && from.currency !== to.currency ? ([from.currency, to.currency] as const) : null

  // Read the way an exchange office's board reads - one of the dearer
  // currency in the cheaper - so a rate below one is asked again the other
  // way round: `1 USD = 5,900 PYG`, not `1 PYG = 0.00017 USD`.
  const reference = useQuery({
    queryKey: pair ? keys.rate(pair[0], pair[1], date) : ['ledger', 'rate', null],
    queryFn: async () => {
      const [given, got] = pair!
      const rate = await api.rateAt(got, given, date)
      return sign(sum([rate.rate, '-1'])) >= 0 ? rate : api.rateAt(given, got, date)
    },
    enabled: pair !== null,
  })

  const record = useMutation({
    mutationFn: async () => {
      const givenAmount = parseAmount(given, locale)
      const gotAmount = parseAmount(got, locale)
      if (!from || !to || !givenAmount || !gotAmount) throw new Error(t('form.incomplete'))
      const counterpartyId = await resolver().counterparty(counterparty)
      return api.exchange({
        from_account: from.id,
        given: givenAmount,
        to_account: to.id,
        got: gotAmount,
        occurred_on: date,
        counterparty_id: counterpartyId,
        place: place && place.country ? place : null,
      })
    },
    onSuccess: (recorded) => {
      void changed()
      toasts.add({
        type: 'success',
        title: t('exchange.recorded'),
        description: recorded.conversion ? describe(recorded.conversion, locale, t) : undefined,
      })
      onDone()
    },
    onError: (error: Error) => {
      void changed()
      setProblem(error.message)
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    const amounts = [parseAmount(given, locale), parseAmount(got, locale)]
    if (amounts.some((value) => value === null || sign(value) <= 0)) {
      setProblem(t('form.positiveAmounts'))
      return
    }
    if (from && to && from.currency === to.currency) {
      setProblem(t('exchange.sameCurrency'))
      return
    }
    record.mutate()
  }

  return (
    <DialogPopup size="lg">
      <form onSubmit={submit} className="contents">
        <DialogHeader>
          <DialogTitle>{t('exchange.title')}</DialogTitle>
        </DialogHeader>
        <DialogBody className="flex flex-col gap-4">
          <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem]">
            <AccountSelect accounts={usable} value={fromId} onValueChange={setFromId} label={t('exchange.from')} exclude={toId} />
            <Field label={t('exchange.given')} required help={from?.currency}>
              <AmountInput value={given} onChange={(event) => setGiven(event.target.value)} />
            </Field>
            <AccountSelect accounts={usable} value={toId} onValueChange={setToId} label={t('exchange.to')} exclude={fromId} />
            <Field label={t('exchange.got')} required help={to?.currency}>
              <AmountInput value={got} onChange={(event) => setGot(event.target.value)} />
            </Field>
          </div>
          {pair && reference.data ? (
            <p className="text-sm text-dim">
              {t('exchange.reference', {
                base: reference.data.base_currency,
                rate: formatRate(reference.data.rate, locale),
                quote: reference.data.quote_currency,
                source: reference.data.source,
                date: formatDay(reference.data.on_date, locale),
              })}
              {reference.data.stale ? <span className="text-warn"> {t('exchange.staleReference')}</span> : null}
            </p>
          ) : null}
          <Field label={t('entry.date')} required>
            <DatePicker
              value={date}
              onValueChange={setDate}
              placeholder={t('entry.date')}
              previousMonthLabel={t('calendar.previous')}
              nextMonthLabel={t('calendar.next')}
            />
          </Field>
          <CounterpartyField value={counterparty} onValueChange={setCounterparty} />
          <PlaceField value={place} onValueChange={setPlace} />
          {problem ? (
            <Alert tone="bad" role="alert">
              {problem}
            </Alert>
          ) : null}
        </DialogBody>
        <DialogActions>
          <Button render={<DialogClose />}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" disabled={record.isPending}>
            {t('exchange.record')}
          </Button>
        </DialogActions>
      </form>
    </DialogPopup>
  )
}

/** What an exchange came to, in one line: the rate it was done at, the day's
 * rate it is measured against, and what the difference cost. */
export function describe(summary: ConversionSummary, locale: string, t: (key: string, options?: Record<string, unknown>) => string): string {
  const parts = [
    t('exchange.deal', { base: summary.deal.base, rate: formatRate(summary.deal.rate, locale), quote: summary.deal.quote }),
  ]
  if (summary.reference) {
    parts.push(t('exchange.against', { rate: formatRate(summary.reference.rate, locale) }))
  }
  if (summary.fee && sign(summary.fee.amount) < 0) {
    // A deal better than the day's rate: said as what it saved, not as a
    // fee of minus something.
    parts.push(t('exchange.saved', { amount: formatMoney(negate(summary.fee.amount), summary.fee.currency, locale) }))
  } else if (summary.fee) {
    parts.push(t('exchange.fee', { fee: formatMoney(summary.fee.amount, summary.fee.currency, locale) }))
  } else if (summary.no_fee === 'no_reference') {
    parts.push(t('exchange.noReference'))
  } else if (summary.no_fee === 'stale_reference') {
    parts.push(t('exchange.staleNoFee'))
  }
  return parts.join(' · ')
}
