import { useMemo, useRef, useState, type FormEvent } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { ScanText } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import type { Picked } from '@/components/Picker'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { DatePicker } from '@/components/ui/date-picker'
import { DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { api } from '@/lib/api'
import { fillFromSlip, useModule } from '@/lib/imports'
import { formatDay, today } from '@/lib/dates'
import { negate, parseAmount, sign, sum } from '@/lib/decimal'
import { keys, openAccounts, useAccounts, useBooksChanged } from '@/lib/ledger'
import { problem as problemOf } from '@/lib/problem'
import { useToastManager } from '@/components/ui/toast'
import { useRecordedToast, useSecondPress } from '@/lib/recording'
import { formatMoney, formatRate } from '@/lib/money'
import type { ConversionSummary, PlaceBody, SlipReading } from '@/lib/types'
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
 *
 * The slip itself can fill the amounts in: a photo of it is read on the
 * server, the amounts and the rate it shows are put in the fields for the
 * person to check, and the photo is kept with the exchange once it is saved.
 */
export function ExchangeForm({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const recorded = useRecordedToast()
  const secondPress = useSecondPress()
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
  const readsSlips = useModule('imports') === true
  const toasts = useToastManager()
  const camera = useRef<HTMLInputElement>(null)
  const [slip, setSlip] = useState<{ photo: File; reading: SlipReading | null } | null>(null)

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

  const read = useMutation({
    mutationFn: (photo: File) => api.readSlip(photo),
    onSuccess: (reading, photo) => {
      setSlip({ photo, reading })
      if (from && to) {
        const fill = fillFromSlip(reading, from.currency, to.currency)
        if (fill.given !== undefined) setGiven(fill.given)
        if (fill.got !== undefined) setGot(fill.got)
      }
    },
    // A slip that cannot be read is still worth keeping: the person types
    // the amounts, and the photo goes with the exchange all the same.
    onError: (error: Error, photo) => {
      setSlip({ photo, reading: null })
      setProblem(problemOf(error, t))
    },
  })

  const record = useMutation({
    mutationFn: async (again: boolean) => {
      const givenAmount = parseAmount(given, locale, from?.currency)
      const gotAmount = parseAmount(got, locale, to?.currency)
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
        ...(again ? { allow_duplicate: true } : {}),
      })
    },
    onSuccess: (done) => {
      void changed()
      recorded(done.entry, t('exchange.recorded'), done.conversion ? describe(done.conversion, locale, t) : undefined)
      if (slip) {
        // Kept with the entry it proves, like a receipt's photo. The exchange
        // is recorded either way; a photo that could not be kept is said.
        api
          .attach(done.entry.id, slip.photo)
          .then(() => void changed())
          .catch((error: Error) => toasts.add({ type: 'error', title: t('exchange.slipAttachFailed', { problem: problemOf(error, t) }) }))
      }
      onDone()
    },
    onError: (error: Error) => {
      void changed()
      // Said to be one press, the entry is recorded already: the form is done.
      if (!secondPress.ask(error, () => record.mutate(true), onDone)) setProblem(problemOf(error, t))
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    const amounts = [parseAmount(given, locale, from?.currency), parseAmount(got, locale, to?.currency)]
    if (amounts.some((value) => value === null || sign(value) <= 0)) {
      setProblem(t('form.positiveAmounts'))
      return
    }
    if (from && to && from.currency === to.currency) {
      setProblem(t('exchange.sameCurrency'))
      return
    }
    record.mutate(false)
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
          {readsSlips ? (
            <div className="flex flex-col gap-1">
              <Button size="sm" className="self-start" onClick={() => camera.current?.click()} disabled={read.isPending || pair === null}>
                <ScanText aria-hidden />
                {read.isPending ? t('exchange.readingSlip') : t('exchange.readSlip')}
              </Button>
              <input
                ref={camera}
                type="file"
                accept="image/jpeg,image/png,image/webp"
                capture="environment"
                hidden
                data-testid="slip-photo"
                onChange={(event) => {
                  const photo = event.target.files?.[0]
                  event.target.value = ''
                  if (photo) {
                    setProblem(null)
                    read.mutate(photo)
                  }
                }}
              />
              {slip ? <SlipSaid reading={slip.reading} /> : null}
            </div>
          ) : null}
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
      {secondPress.dialog}
    </DialogPopup>
  )
}

/** What a slip was read as, for checking against the fields it filled. */
function SlipSaid({ reading }: { reading: SlipReading | null }) {
  const { t } = useTranslation()
  const locale = useLocale()
  if (!reading || (reading.amounts.length === 0 && !reading.rate)) {
    return <p className="text-sm text-dim">{reading ? t('exchange.slipNothing') : t('exchange.slipKept')}</p>
  }
  const parts = reading.amounts.map((money) => formatMoney(money.amount, money.currency, locale))
  if (reading.rate?.base && reading.rate.quote) {
    parts.push(t('exchange.slipRate', { base: reading.rate.base, rate: formatRate(reading.rate.rate, locale), quote: reading.rate.quote }))
  } else if (reading.rate) {
    parts.push(formatRate(reading.rate.rate, locale))
  }
  return (
    <div className="flex flex-col gap-1 text-sm text-dim">
      <p>{t('exchange.slipRead', { what: parts.join(', ') })}</p>
      <details>
        <summary className="cursor-pointer text-xs">{t('exchange.slipText')}</summary>
        <pre className="mt-1 max-h-40 overflow-auto font-mono text-xs whitespace-pre-wrap">{reading.text}</pre>
      </details>
    </div>
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
