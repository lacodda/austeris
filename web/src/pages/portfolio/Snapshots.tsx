import { useState, type FormEvent } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { ArrowLeft, Camera } from 'lucide-react'
import { Money } from '@/components/Money'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
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
import { DatePicker } from '@/components/ui/date-picker'
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
import { formatDay, formatMoment, today } from '@/lib/dates'
import { negate, sign, sum } from '@/lib/decimal'
import { useAccounts } from '@/lib/ledger'
import { formatQuantity, keys, useComparison, useSnapshots, useValueCurrency } from '@/lib/portfolio'
import { problem } from '@/lib/problem'
import type { Difference } from '@/lib/types'

/** Every snapshot, the latest day first: what the portfolio was worth then. */
export function Snapshots() {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const [currency] = useValueCurrency(accounts.data)
  const snapshots = useSnapshots()

  return (
    <div className="flex flex-col gap-4">
      <div className="flex justify-end">{currency ? <TakeSnapshot currency={currency} /> : null}</div>
      <QueryState
        pending={snapshots.isPending}
        error={snapshots.error}
        empty={(snapshots.data ?? []).length === 0}
        emptyState={<EmptyState title={t('snapshot.emptyTitle')} body={t('snapshot.emptyBody')} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void snapshots.refetch()}
        retryLabel={t('common.retry')}
      >
        <Panel>
          <TableScroll>
            <Table>
              <TableHead>
                <TableRow>
                  <TableHeader>{t('snapshot.day')}</TableHeader>
                  <TableHeader numeric>{t('snapshot.worth')}</TableHeader>
                </TableRow>
              </TableHead>
              <TableBody>
                {(snapshots.data ?? []).map((snapshot) => (
                  <TableRow key={snapshot.id}>
                    <TableCell>
                      <Link to={`/portfolio/snapshots/${snapshot.id}`} className="font-medium text-text hover:text-accent">
                        {formatDay(snapshot.on, locale)}
                      </Link>
                      {snapshot.note ? <div className="text-xs text-dim">{snapshot.note}</div> : null}
                    </TableCell>
                    <TableCell numeric>
                      <Money amount={snapshot.total} currency={snapshot.currency} />
                      {snapshot.incomplete ? <div className="text-xs text-warn">{t('snapshot.incomplete')}</div> : null}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableScroll>
        </Panel>
      </QueryState>
    </div>
  )
}

/** Taking a snapshot: of today, or of a day past, in a currency, with a word
 * on it. */
export function TakeSnapshot({ currency }: { currency: string }) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const queries = useQueryClient()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [day, setDay] = useState(today())
  const [note, setNote] = useState('')
  const [failure, setFailure] = useState<string | null>(null)

  const take = useMutation({
    mutationFn: () => api.takeSnapshot(currency, note.trim(), day === today() ? undefined : day),
    onSuccess: (snapshot) => {
      void queries.invalidateQueries({ queryKey: keys.snapshots })
      toasts.add({ type: 'success', title: t('snapshot.taken') })
      setOpen(false)
      setNote('')
      void navigate(`/portfolio/snapshots/${snapshot.id}`)
    },
    onError: (error: Error) => setFailure(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setFailure(null)
    take.mutate()
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Button size="sm" onClick={() => setOpen(true)}>
        <Camera aria-hidden />
        {t('snapshot.take')}
      </Button>
      <DialogPopup>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{t('snapshot.takeTitle')}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <p className="text-sm text-dim">{t('snapshot.takeHelp', { currency })}</p>
            <Field label={t('snapshot.day')} required>
              <DatePicker
                value={day}
                onValueChange={setDay}
                max={today()}
                placeholder={t('snapshot.day')}
                previousMonthLabel={t('calendar.previous')}
                nextMonthLabel={t('calendar.next')}
              />
            </Field>
            <Field label={t('snapshot.note')}>
              <Input value={note} onChange={(event) => setNote(event.target.value)} placeholder={t('snapshot.notePlaceholder')} />
            </Field>
            {failure ? (
              <Alert tone="bad" role="alert">
                {failure}
              </Alert>
            ) : null}
          </DialogBody>
          <DialogActions>
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={take.isPending}>
              {t('snapshot.take')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}

/** What a line's value moved by, when both ends are known - a line that was
 * not there, or is gone, being worth nothing at that end. */
function change(then: string | null | undefined, now: string | null | undefined): string | null {
  return then && now ? sum([now, negate(then)]) : null
}

/** A line's value at one end: nothing held is worth nothing, and held without
 * a price is not known. */
function worth(quantity: string, value: string | null | undefined): string | null {
  return sign(quantity) === 0 ? '0' : (value ?? null)
}

/**
 * A snapshot beside now - or beside another snapshot: every line of either,
 * how many were held then and now, and what they were worth.
 */
export function SnapshotView() {
  const { t } = useTranslation()
  const locale = useLocale()
  const { id = '' } = useParams()
  const navigate = useNavigate()
  const toasts = useToastManager()
  const queries = useQueryClient()
  const [against, setAgainst] = useState('')
  const snapshots = useSnapshots()
  const comparison = useComparison(id, against || undefined)
  const others = (snapshots.data ?? []).filter((snapshot) => snapshot.id !== id)
  const NOW = ''
  const choices: Record<string, string> = { [NOW]: t('snapshot.now') }
  for (const other of others) choices[other.id] = formatDay(other.on, locale) + (other.note ? ` · ${other.note}` : '')

  const remove = useMutation({
    mutationFn: () => api.deleteSnapshot(id),
    onSuccess: () => {
      void queries.invalidateQueries({ queryKey: keys.snapshots })
      toasts.add({ type: 'success', title: t('snapshot.deleted') })
      void navigate('/portfolio/snapshots')
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })

  const data = comparison.data
  const currency = data?.snapshot.currency ?? ''
  const totalChange = data ? change(data.snapshot.total, data.against.total) : null

  return (
    <div className="flex flex-col gap-4">
      <Link to="/portfolio/snapshots" className="inline-flex items-center gap-1 text-sm text-dim hover:text-accent">
        <ArrowLeft aria-hidden className="size-4" />
        {t('snapshot.all')}
      </Link>
      <QueryState
        pending={comparison.isPending}
        error={comparison.error}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void comparison.refetch()}
        retryLabel={t('common.retry')}
      >
        {data ? (
          <>
            <Panel className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-4">
              <div className="flex flex-col gap-1">
                <span className="caption">{t('snapshot.of', { day: formatDay(data.snapshot.on, locale) })}</span>
                <Money amount={data.snapshot.total} currency={currency} className="text-2xl font-semibold text-text" />
                {data.snapshot.note ? <span className="text-sm text-dim">{data.snapshot.note}</span> : null}
                <span className="text-xs text-faint">{t('snapshot.takenAt', { moment: formatMoment(data.snapshot.taken_at, locale) })}</span>
              </div>
              <div className="flex flex-col items-end gap-1">
                <span className="caption">{against ? choices[against] : t('snapshot.now')}</span>
                <Money amount={data.against.total} currency={currency} className="text-lg font-medium text-text" />
                {totalChange ? <Change amount={totalChange} currency={currency} /> : null}
              </div>
            </Panel>
            <div className="flex flex-wrap items-end justify-between gap-2">
              <Field label={t('snapshot.compareWith')} className="w-64">
                <Select items={choices} value={against} onValueChange={(value) => setAgainst(value as string)}>
                  <SelectTrigger size="sm">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectPopup>
                    {Object.entries(choices).map(([value, label]) => (
                      <SelectItem key={value} value={value}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectPopup>
                </Select>
              </Field>
              <ConfirmDialog>
                <ConfirmDialogTrigger render={<Button variant="danger" size="sm" />}>{t('snapshot.delete')}</ConfirmDialogTrigger>
                <ConfirmDialogPopup>
                  <ConfirmDialogHeader>
                    <ConfirmDialogTitle>{t('snapshot.deleteTitle')}</ConfirmDialogTitle>
                    <ConfirmDialogDescription>{t('snapshot.deleteBody')}</ConfirmDialogDescription>
                  </ConfirmDialogHeader>
                  <ConfirmDialogActions>
                    <Button render={<ConfirmDialogClose />}>{t('common.cancel')}</Button>
                    <Button variant="danger" onClick={() => remove.mutate()} disabled={remove.isPending}>
                      {t('snapshot.delete')}
                    </Button>
                  </ConfirmDialogActions>
                </ConfirmDialogPopup>
              </ConfirmDialog>
            </div>
            {data.snapshot.unvalued.length > 0 || data.against.unvalued.length > 0 ? (
              <Alert tone="warn">{t('snapshot.unvalued', { symbols: [...new Set([...data.snapshot.unvalued, ...data.against.unvalued])].join(', ') })}</Alert>
            ) : null}
            <Panel>
              <TableScroll>
                <Table>
                  <TableHead>
                    <TableRow>
                      <TableHeader>{t('portfolio.what')}</TableHeader>
                      <TableHeader numeric>{t('snapshot.then')}</TableHeader>
                      <TableHeader numeric>{against ? t('snapshot.thenOther') : t('snapshot.now')}</TableHeader>
                      <TableHeader numeric className="hidden sm:table-cell">
                        {t('snapshot.change')}
                      </TableHeader>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {data.differences.map((line) => (
                      <DifferenceRow key={`${line.account_id}:${line.kind}:${line.instrument_id ?? line.symbol}`} line={line} currency={currency} locale={locale} />
                    ))}
                  </TableBody>
                </Table>
              </TableScroll>
            </Panel>
          </>
        ) : null}
      </QueryState>
    </div>
  )
}

function DifferenceRow({ line, currency, locale }: { line: Difference; currency: string; locale: string }) {
  const { t } = useTranslation()
  const cash = line.kind === 'cash'
  const amount = (quantity: string) => (cash ? <Money amount={quantity} currency={line.symbol} /> : formatQuantity(quantity, locale))
  const moved = change(worth(line.quantity_then, line.value_then), worth(line.quantity_now, line.value_now))
  // Money in the currency it is valued in is its own value.
  const valued = !(cash && line.symbol === currency)
  return (
    <TableRow>
      <TableCell>
        <div className="flex items-center gap-2">
          <span className="font-mono font-medium text-text">{line.symbol}</span>
          {cash ? <Badge variant="soft">{t('portfolio.money')}</Badge> : null}
        </div>
        <div className="text-xs text-dim">{line.account_name}</div>
      </TableCell>
      <TableCell numeric>
        {amount(line.quantity_then)}
        {valued && line.value_then ? (
          <div className="text-xs text-dim">
            <Money amount={line.value_then} currency={currency} />
          </div>
        ) : null}
      </TableCell>
      <TableCell numeric>
        {amount(line.quantity_now)}
        {valued && line.value_now ? (
          <div className="text-xs text-dim">
            <Money amount={line.value_now} currency={currency} />
          </div>
        ) : null}
      </TableCell>
      <TableCell numeric className="hidden sm:table-cell">
        {moved ? <Change amount={moved} currency={currency} /> : null}
      </TableCell>
    </TableRow>
  )
}

/** A change in value: green up, red down, nothing for none. */
function Change({ amount, currency }: { amount: string; currency: string }) {
  const direction = sign(amount)
  return (
    <span className={direction > 0 ? 'text-good' : direction < 0 ? 'text-bad' : 'text-dim'}>
      <Money amount={amount} currency={currency} signDisplay="exceptZero" />
    </span>
  )
}
