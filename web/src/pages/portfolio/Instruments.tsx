import { useMemo, useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Plus } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { EmptyState } from '@/components/ui/empty-state'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatMoment } from '@/lib/dates'
import { parseAmount } from '@/lib/decimal'
import { useAccounts } from '@/lib/ledger'
import { localMoment, useInstruments, useMarketChanged, usePrices, useValueCurrency, withOffset } from '@/lib/portfolio'
import { problem } from '@/lib/problem'
import { INSTRUMENT_KINDS, type Instrument, type InstrumentKind } from '@/lib/types'

/**
 * What can be priced, and what it was last worth.
 *
 * Prices arrive from the sources the installation has keys for; one can also
 * be said by hand - a coin's, when no source is switched on, or a flat's -
 * and the newest word is the latest whoever said it.
 */
export function Instruments() {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const instruments = useInstruments()
  const [currency, setCurrency, currencies] = useValueCurrency(accounts.data)
  const prices = usePrices(currency)
  const [pricing, setPricing] = useState<Instrument | null>(null)
  const latest = useMemo(() => new Map((prices.data ?? []).map((price) => [price.instrument_id, price])), [prices.data])
  const shown = (instruments.data ?? []).filter((instrument) => instrument.kind !== 'fx')
  const choices = Object.fromEntries(currencies.map((code) => [code, code]))

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-end gap-2">
        {currency ? (
          <Select items={choices} value={currency} onValueChange={(code) => setCurrency(code as string)}>
            <SelectTrigger aria-label={t('portfolio.valueIn')} className="w-28" size="sm">
              <SelectValue />
            </SelectTrigger>
            <SelectPopup>
              {currencies.map((code) => (
                <SelectItem key={code} value={code}>
                  {code}
                </SelectItem>
              ))}
            </SelectPopup>
          </Select>
        ) : null}
        <NewInstrument />
      </div>
      <QueryState
        pending={instruments.isPending}
        error={instruments.error ?? prices.error}
        empty={shown.length === 0}
        emptyState={<EmptyState title={t('instrument.emptyTitle')} body={t('instrument.emptyBody')} action={<NewInstrument />} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void (instruments.error ? instruments.refetch() : prices.refetch())}
        retryLabel={t('common.retry')}
      >
        <Panel>
          <TableScroll>
            <Table>
              <TableHead>
                <TableRow>
                  <TableHeader>{t('instrument.instrument')}</TableHeader>
                  <TableHeader numeric>{t('portfolio.price', { currency: currency ?? '' })}</TableHeader>
                  <TableHeader>
                    <span className="sr-only">{t('common.actions')}</span>
                  </TableHeader>
                </TableRow>
              </TableHead>
              <TableBody>
                {shown.map((instrument) => {
                  const price = latest.get(instrument.id)
                  return (
                    <TableRow key={instrument.id}>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <span className="font-mono font-medium text-text">{instrument.symbol}</span>
                          <Badge variant="outline">{t(`instrument.kinds.${instrument.kind}`)}</Badge>
                        </div>
                        <div className="text-xs text-dim">{instrument.name}</div>
                      </TableCell>
                      <TableCell numeric>
                        {price ? (
                          <>
                            <Money amount={price.price} currency={price.quote_currency} />
                            <div className={price.stale ? 'text-xs text-warn' : 'text-xs text-dim'}>
                              {t(price.stale ? 'instrument.staleAsOf' : 'instrument.asOf', { moment: formatMoment(price.observed_at, locale) })}
                              {price.source === 'manual' ? ` · ${t('instrument.byHand')}` : null}
                            </div>
                          </>
                        ) : (
                          <span className="text-dim">{t('portfolio.noPrice')}</span>
                        )}
                      </TableCell>
                      <TableCell className="w-10 text-right">
                        <Button size="sm" variant="ghost" onClick={() => setPricing(instrument)}>
                          {t('instrument.price')}
                        </Button>
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          </TableScroll>
        </Panel>
      </QueryState>
      {pricing && currency ? (
        <PriceDialog instrument={pricing} currency={currency} onOpenChange={(still) => (still ? undefined : setPricing(null))} />
      ) : null}
    </div>
  )
}

/** Adding something to price: its kind, its ticker, and what it is called. */
function NewInstrument() {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useMarketChanged()
  const [open, setOpen] = useState(false)
  const [kind, setKind] = useState<InstrumentKind>('crypto')
  const [symbol, setSymbol] = useState('')
  const [name, setName] = useState('')
  const [failure, setFailure] = useState<string | null>(null)

  const create = useMutation({
    mutationFn: () => api.createInstrument({ kind, symbol: symbol.trim().toUpperCase(), name: name.trim() || symbol.trim().toUpperCase() }),
    onSuccess: (instrument) => {
      void changed()
      toasts.add({ type: 'success', title: t('instrument.created', { symbol: instrument.symbol }) })
      setOpen(false)
      setSymbol('')
      setName('')
    },
    onError: (error: Error) => setFailure(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setFailure(null)
    create.mutate()
  }

  const kinds = Object.fromEntries(INSTRUMENT_KINDS.map((each) => [each, t(`instrument.kinds.${each}`)]))

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Button size="sm" onClick={() => setOpen(true)}>
        <Plus aria-hidden />
        {t('instrument.new')}
      </Button>
      <DialogPopup>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{t('instrument.newTitle')}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <Field label={t('instrument.kind')}>
              <Select items={kinds} value={kind} onValueChange={(value) => setKind(value as InstrumentKind)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectPopup>
                  {INSTRUMENT_KINDS.map((each) => (
                    <SelectItem key={each} value={each}>
                      {kinds[each]}
                    </SelectItem>
                  ))}
                </SelectPopup>
              </Select>
            </Field>
            <Field label={t('instrument.symbol')} help={t('instrument.symbolHelp')} required>
              <Input
                required
                value={symbol}
                onChange={(event) => setSymbol(event.target.value.toUpperCase())}
                maxLength={12}
                autoCapitalize="characters"
                autoComplete="off"
                className="font-mono uppercase"
                autoFocus
              />
            </Field>
            <Field label={t('instrument.name')}>
              <Input value={name} onChange={(event) => setName(event.target.value)} />
            </Field>
            {failure ? (
              <Alert tone="bad" role="alert">
                {failure}
              </Alert>
            ) : null}
          </DialogBody>
          <DialogActions>
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={create.isPending}>
              {t('instrument.create')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}

/** A price said by hand: what a unit is worth, in a currency, at a moment. */
function PriceDialog({ instrument, currency, onOpenChange }: { instrument: Instrument; currency: string; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useMarketChanged()
  const [price, setPrice] = useState('')
  const [quote, setQuote] = useState(currency)
  const [at, setAt] = useState(localMoment(new Date()))
  const [failure, setFailure] = useState<string | null>(null)

  const record = useMutation({
    mutationFn: (amount: string) => api.recordPrice(instrument.id, { price: amount, currency: quote.trim().toUpperCase(), observed_at: withOffset(at) }),
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('instrument.priced', { symbol: instrument.symbol }) })
      onOpenChange(false)
    },
    onError: (error: Error) => setFailure(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setFailure(null)
    const amount = parseAmount(price, locale)
    if (amount === null) {
      setFailure(t('form.notAnAmount'))
      return
    }
    record.mutate(amount)
  }

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogPopup>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{t('instrument.priceTitle', { symbol: instrument.symbol })}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <p className="text-sm text-dim">{t('instrument.priceHelp')}</p>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label={t('instrument.unitPrice')} required>
                <AmountInput value={price} onChange={(event) => setPrice(event.target.value)} autoFocus />
              </Field>
              <Field label={t('instrument.currency')} required>
                <Input
                  required
                  value={quote}
                  onChange={(event) => setQuote(event.target.value.toUpperCase())}
                  maxLength={5}
                  autoCapitalize="characters"
                  autoComplete="off"
                  className="font-mono uppercase"
                />
              </Field>
            </div>
            <Field label={t('instrument.when')} required>
              <Input type="datetime-local" required value={at} onChange={(event) => setAt(event.target.value)} />
            </Field>
            {failure ? (
              <Alert tone="bad" role="alert">
                {failure}
              </Alert>
            ) : null}
          </DialogBody>
          <DialogActions>
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={record.isPending}>
              {t('instrument.recordPrice')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}
