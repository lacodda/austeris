import { useMemo, useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Plus } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  ConfirmDialog,
  ConfirmDialogActions,
  ConfirmDialogClose,
  ConfirmDialogDescription,
  ConfirmDialogHeader,
  ConfirmDialogPopup,
  ConfirmDialogTitle,
  ConfirmDialogTrigger,
} from '@/components/ui/confirm-dialog'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatMoment } from '@/lib/dates'
import { parseAmount } from '@/lib/decimal'
import { useAccounts } from '@/lib/ledger'
import { draftOf, holdingAccounts, localMoment, tradeBody, tradeMoney, useInstruments, useTradeHistory, useTradesChanged } from '@/lib/portfolio'
import { problem } from '@/lib/problem'
import { TRADE_KINDS, type Trade, type TradeKind } from '@/lib/types'

/** The button that records a trade, and the form it opens. */
export function RecordTrade({ variant = 'primary' }: { variant?: 'primary' | 'ghost' }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <Button variant={variant} size="sm" onClick={() => setOpen(true)}>
        <Plus aria-hidden />
        {t('trade.record')}
      </Button>
      {open ? <TradeDialog open={open} onOpenChange={setOpen} /> : null}
    </>
  )
}

/**
 * A trade: recorded, or changed and deleted.
 *
 * What the account is paid in - its currency, which is the price's and the
 * fee's - comes from the account chosen; a transfer's fee is in units, as the
 * network takes it. What its money will move the account by is shown before
 * it is saved, worked out the way the server books it.
 */
export function TradeDialog({ trade, open, onOpenChange }: { trade?: Trade; open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useTradesChanged()
  const accounts = useAccounts()
  const instruments = useInstruments()
  const start = useMemo(() => (trade ? draftOf(trade) : undefined), [trade])
  const [kind, setKind] = useState<TradeKind>(start?.kind ?? 'buy')
  const [accountId, setAccountId] = useState(start?.accountId ?? '')
  const [toAccountId, setToAccountId] = useState(start?.toAccountId ?? '')
  const [instrumentId, setInstrumentId] = useState(start?.instrumentId ?? '')
  const [quantity, setQuantity] = useState(start?.quantity ?? '')
  const [price, setPrice] = useState(start?.price ?? '')
  const [fee, setFee] = useState(start?.fee && start.fee !== '0' ? start.fee : '')
  const [at, setAt] = useState(start?.at ?? localMoment(new Date()))
  const [note, setNote] = useState(start?.note ?? '')
  const [failure, setFailure] = useState<string | null>(null)

  const places = holdingAccounts(accounts.data, start?.accountId)
  const account = places.find((each) => each.id === accountId)
  const tradable = (instruments.data ?? []).filter((instrument) => instrument.kind !== 'fx')
  const instrument = tradable.find((each) => each.id === instrumentId)
  const transfer = kind === 'transfer'

  const units = parseAmount(quantity, locale)
  const each = transfer ? undefined : parseAmount(price, locale)
  const cost = fee.trim() === '' ? '0' : parseAmount(fee, locale, transfer ? undefined : account?.currency)
  const money = !transfer && account && units && each && cost ? tradeMoney(kind, units, each, cost, account.currency) : null

  const save = useMutation({
    mutationFn: () => {
      const body = tradeBody({
        kind,
        accountId,
        toAccountId: toAccountId || undefined,
        instrumentId,
        quantity: units ?? '',
        price: each ?? undefined,
        fee: cost ?? undefined,
        at,
        note,
      })
      return trade ? api.replaceTrade(trade.id, body) : api.createTrade(body)
    },
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t(trade ? 'trade.saved' : 'trade.recorded') })
      onOpenChange(false)
    },
    onError: (error: Error) => setFailure(problem(error, t)),
  })

  const remove = useMutation({
    mutationFn: () => api.deleteTrade(trade?.id ?? ''),
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('trade.deleted') })
      onOpenChange(false)
    },
    onError: (error: Error) => setFailure(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setFailure(null)
    if (!account || !instrument || (transfer && !toAccountId)) {
      setFailure(t('trade.incomplete'))
      return
    }
    if (units === null || (!transfer && each === null) || cost === null) {
      setFailure(t('form.notAnAmount'))
      return
    }
    save.mutate()
  }

  const accountItems = Object.fromEntries(places.map((each) => [each.id, `${each.name} · ${each.currency}`]))
  const destinations = places.filter((each) => each.id !== accountId)
  const destinationItems = Object.fromEntries(destinations.map((each) => [each.id, each.name]))
  const instrumentItems = Object.fromEntries(tradable.map((each) => [each.id, `${each.symbol} · ${each.name}`]))

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{t(trade ? 'trade.editTitle' : 'trade.newTitle')}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <SegmentedControl aria-label={t('trade.kind')} value={kind} onValueChange={(value) => setKind(value as TradeKind)}>
              {TRADE_KINDS.map((each) => (
                <Segment key={each} value={each}>
                  {t(`trade.kinds.${each}`)}
                </Segment>
              ))}
            </SegmentedControl>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label={transfer ? t('trade.from') : t('trade.account')} help={places.length === 0 ? t('trade.noAccounts') : undefined} required>
                <Select items={accountItems} value={accountId || null} onValueChange={(id) => setAccountId(id as string)}>
                  <SelectTrigger>
                    <SelectValue placeholder={t('entry.chooseAccount')} />
                  </SelectTrigger>
                  <SelectPopup>
                    {places.map((each) => (
                      <SelectItem key={each.id} value={each.id}>
                        {each.name} <span className="text-dim">· {each.currency}</span>
                      </SelectItem>
                    ))}
                  </SelectPopup>
                </Select>
              </Field>
              {transfer ? (
                <Field label={t('trade.to')} required>
                  <Select items={destinationItems} value={toAccountId || null} onValueChange={(id) => setToAccountId(id as string)}>
                    <SelectTrigger>
                      <SelectValue placeholder={t('entry.chooseAccount')} />
                    </SelectTrigger>
                    <SelectPopup>
                      {destinations.map((each) => (
                        <SelectItem key={each.id} value={each.id}>
                          {each.name}
                        </SelectItem>
                      ))}
                    </SelectPopup>
                  </Select>
                </Field>
              ) : null}
              <Field label={t('trade.instrument')} help={tradable.length === 0 ? t('trade.noInstruments') : undefined} required>
                <Select items={instrumentItems} value={instrumentId || null} onValueChange={(id) => setInstrumentId(id as string)}>
                  <SelectTrigger>
                    <SelectValue placeholder={t('trade.chooseInstrument')} />
                  </SelectTrigger>
                  <SelectPopup>
                    {tradable.map((each) => (
                      <SelectItem key={each.id} value={each.id}>
                        <span className="font-mono">{each.symbol}</span> <span className="text-dim">· {each.name}</span>
                      </SelectItem>
                    ))}
                  </SelectPopup>
                </Select>
              </Field>
              <Field label={t('trade.quantity')} help={instrument?.symbol} required>
                <AmountInput value={quantity} onChange={(event) => setQuantity(event.target.value)} />
              </Field>
              {transfer ? null : (
                <Field label={t('trade.price')} help={account ? t('trade.perUnitIn', { currency: account.currency }) : undefined} required>
                  <AmountInput value={price} onChange={(event) => setPrice(event.target.value)} />
                </Field>
              )}
              <Field
                label={t('trade.fee')}
                help={transfer ? t('trade.networkFee', { symbol: instrument?.symbol ?? '' }) : account ? account.currency : undefined}
              >
                <AmountInput value={fee} onChange={(event) => setFee(event.target.value)} placeholder="0" />
              </Field>
              <Field label={t('trade.when')} required>
                <Input type="datetime-local" required value={at} onChange={(event) => setAt(event.target.value)} />
              </Field>
            </div>
            <Field label={t('trade.note')}>
              <Input value={note} onChange={(event) => setNote(event.target.value)} />
            </Field>
            {money && account ? (
              <p className="text-sm text-dim">
                {t('trade.moves', { account: account.name })} <Money amount={money} currency={account.currency} signDisplay="exceptZero" />
              </p>
            ) : transfer ? (
              <p className="text-sm text-dim">{t('trade.noMoney')}</p>
            ) : null}
            {failure ? (
              <Alert tone="bad" role="alert">
                {failure}
              </Alert>
            ) : null}
            {trade ? <TradeHistory id={trade.id} /> : null}
          </DialogBody>
          <DialogActions
            start={
              trade ? (
                <ConfirmDialog>
                  <ConfirmDialogTrigger render={<Button variant="danger" />}>{t('trade.delete')}</ConfirmDialogTrigger>
                  <ConfirmDialogPopup>
                    <ConfirmDialogHeader>
                      <ConfirmDialogTitle>{t('trade.deleteTitle')}</ConfirmDialogTitle>
                      <ConfirmDialogDescription>{t('trade.deleteBody')}</ConfirmDialogDescription>
                    </ConfirmDialogHeader>
                    <ConfirmDialogActions>
                      <Button render={<ConfirmDialogClose />}>{t('common.cancel')}</Button>
                      <Button variant="danger" onClick={() => remove.mutate()} disabled={remove.isPending}>
                        {t('trade.delete')}
                      </Button>
                    </ConfirmDialogActions>
                  </ConfirmDialogPopup>
                </ConfirmDialog>
              ) : null
            }
          >
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={save.isPending}>
              {t(trade ? 'trade.save' : 'trade.record')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}

/** Who recorded and changed a trade, and when. */
function TradeHistory({ id }: { id: string }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const history = useTradeHistory(id)
  if (!history.data || history.data.length === 0) return null
  return (
    <section className="flex flex-col gap-1" aria-label={t('trade.history')}>
      <h3 className="caption">{t('trade.history')}</h3>
      <ul className="flex flex-col gap-0.5 text-xs text-dim">
        {history.data.map((revision) => (
          <li key={revision.id}>
            {t(`trade.actions.${revision.action}`)} · {formatMoment(revision.at, locale)}
          </li>
        ))}
      </ul>
    </section>
  )
}
