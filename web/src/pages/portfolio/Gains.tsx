import { useMemo, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { EmptyState } from '@/components/ui/empty-state'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatListDay } from '@/lib/dates'
import { sign } from '@/lib/decimal'
import { useAccounts } from '@/lib/ledger'
import { formatQuantity, useCostMethodChanged, useGains, useLots, usePortfolioAccounts, useTrades, useValueCurrency } from '@/lib/portfolio'
import { problem } from '@/lib/problem'
import { COST_METHODS, type CostMethod, type Decimal, type PortfolioAccount, type PositionLots } from '@/lib/types'

/**
 * What sales gained, and what is held is made of.
 *
 * A year at a time, as a tax return counts it: each sale's proceeds beside
 * what the lots it took cost, in the currency the portfolio is read in. Then
 * how each account picks those lots - which is the person's to say, and
 * counts every gain on the account again - and the lots still held.
 */
export function Gains() {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const trades = useTrades()
  const [currency, setCurrency, currencies] = useValueCurrency(accounts.data)
  const thisYear = new Date().getFullYear()
  const [year, setYear] = useState(thisYear)
  const gains = useGains(`${year}-01-01`, `${year}-12-31`, currency)

  // Every year a trade happened in, and this one.
  const years = useMemo(() => {
    const seen = new Set<number>([thisYear])
    for (const trade of trades.data ?? []) seen.add(Number(trade.occurred_on.slice(0, 4)))
    return [...seen].sort((a, b) => b - a)
  }, [trades.data, thisYear])
  const yearItems = Object.fromEntries(years.map((each) => [String(each), String(each)]))
  const currencyItems = Object.fromEntries(currencies.map((code) => [code, code]))

  return (
    <div className="flex flex-col gap-6">
      <QueryState
        pending={accounts.isPending || gains.isPending}
        error={accounts.error ?? gains.error}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void gains.refetch()}
        retryLabel={t('common.retry')}
      >
        {gains.data ? (
          <section className="flex flex-col gap-3" aria-label={t('gains.sales')}>
            <Panel className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-4">
              <dl className="flex flex-wrap gap-x-8 gap-y-2">
                <div className="flex flex-col gap-1">
                  <dt className="caption">{t('gains.gain')}</dt>
                  <dd>
                    <Signed amount={gains.data.gain} currency={gains.data.currency} className="text-2xl font-semibold" />
                  </dd>
                </div>
                <div className="flex flex-col gap-1">
                  <dt className="caption">{t('gains.proceeds')}</dt>
                  <dd>
                    <Money amount={gains.data.proceeds} currency={gains.data.currency} className="text-lg text-text" />
                  </dd>
                </div>
                <div className="flex flex-col gap-1">
                  <dt className="caption">{t('gains.cost')}</dt>
                  <dd>
                    <Money amount={gains.data.cost} currency={gains.data.currency} className="text-lg text-text" />
                  </dd>
                </div>
              </dl>
              <div className="flex flex-wrap items-center gap-2">
                <Select items={yearItems} value={String(year)} onValueChange={(each) => setYear(Number(each))}>
                  <SelectTrigger aria-label={t('gains.year')} className="w-24" size="sm">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectPopup>
                    {years.map((each) => (
                      <SelectItem key={each} value={String(each)}>
                        {each}
                      </SelectItem>
                    ))}
                  </SelectPopup>
                </Select>
                <Select items={currencyItems} value={currency} onValueChange={(code) => setCurrency(code as string)}>
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
              </div>
              {gains.data.unvalued.length > 0 ? (
                <p className="w-full text-sm text-warn">
                  {t('gains.unvalued', { currency: gains.data.currency, symbols: gains.data.unvalued.join(', ') })}
                </p>
              ) : null}
            </Panel>
            {gains.data.sales.length === 0 ? (
              <EmptyState title={t('gains.noSalesTitle', { year })} body={t('gains.noSalesBody')} />
            ) : (
              <Panel>
                <TableScroll>
                  <Table>
                    <TableHead>
                      <TableRow>
                        <TableHeader>{t('trade.day')}</TableHeader>
                        <TableHeader>{t('trade.what')}</TableHeader>
                        <TableHeader numeric className="hidden sm:table-cell">
                          {t('trade.quantity')}
                        </TableHeader>
                        <TableHeader numeric className="hidden md:table-cell">
                          {t('gains.proceeds')}
                        </TableHeader>
                        <TableHeader numeric className="hidden md:table-cell">
                          {t('gains.cost')}
                        </TableHeader>
                        <TableHeader numeric>{t('gains.gain')}</TableHeader>
                      </TableRow>
                    </TableHead>
                    <TableBody>
                      {gains.data.sales.map((sale) => (
                        <TableRow key={sale.trade_id}>
                          <TableCell className="whitespace-nowrap text-dim">{formatListDay(sale.occurred_on, locale)}</TableCell>
                          <TableCell>
                            <span className="font-mono font-medium text-text">{sale.symbol}</span>
                            <div className="text-xs text-dim">
                              {sale.account_name} · {t(`gains.method.${sale.method}`)}
                            </div>
                          </TableCell>
                          <TableCell numeric className="hidden sm:table-cell">
                            <Quantity amount={sale.quantity} />
                          </TableCell>
                          <TableCell numeric className="hidden md:table-cell">
                            {sale.proceeds ? <Money amount={sale.proceeds} currency={gains.data.currency} /> : null}
                          </TableCell>
                          <TableCell numeric className="hidden md:table-cell">
                            {sale.cost ? <Money amount={sale.cost} currency={gains.data.currency} /> : null}
                          </TableCell>
                          <TableCell numeric>
                            {sale.gain ? (
                              <Signed amount={sale.gain} currency={gains.data.currency} />
                            ) : (
                              <span className="text-warn">{t('gains.noRate')}</span>
                            )}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </TableScroll>
              </Panel>
            )}
            <p className="text-xs text-faint">{t('gains.feesIncluded')}</p>
          </section>
        ) : null}
      </QueryState>
      <Methods />
      <Lots />
    </div>
  )
}

/** A number of units, every digit it has, in the interface's language. */
function Quantity({ amount }: { amount: Decimal }) {
  const locale = useLocale()
  return <span className="tabular-nums">{formatQuantity(amount, locale)}</span>
}

/** A gain or a loss: its sign written, and coloured - never colour alone. */
export function Signed({ amount, currency, className }: { amount: Decimal; currency: string; className?: string }) {
  const tone = sign(amount) > 0 ? 'text-good' : sign(amount) < 0 ? 'text-bad' : 'text-text'
  return <Money amount={amount} currency={currency} signDisplay="exceptZero" className={`${tone} ${className ?? ''}`} />
}

/** How each broker's and wallet's account picks the lots its sales take. */
function Methods() {
  const { t } = useTranslation()
  const accounts = usePortfolioAccounts()
  const open = (accounts.data ?? []).filter((account) => !account.closed)
  if (open.length === 0) return null
  return (
    <section className="flex flex-col gap-2" aria-label={t('gains.methods')}>
      <h2 className="font-medium text-text">{t('gains.methods')}</h2>
      <p className="text-sm text-dim">{t('gains.methodsHelp')}</p>
      <Panel className="divide-y divide-line">
        {open.map((account) => (
          <MethodRow key={account.account_id} account={account} />
        ))}
      </Panel>
    </section>
  )
}

function MethodRow({ account }: { account: PortfolioAccount }) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useCostMethodChanged()
  const items = Object.fromEntries(COST_METHODS.map((method) => [method, t(`gains.method.${method}`)]))
  const save = useMutation({
    mutationFn: (method: CostMethod) => api.setCostMethod(account.account_id, method),
    onSuccess: (saved) => {
      void changed()
      toasts.add({ type: 'success', title: t('gains.methodSaved', { account: saved.name, method: t(`gains.method.${saved.cost_method}`) }) })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
      <div>
        <div className="text-text">{account.name}</div>
        <div className="text-xs text-dim">{account.currency}</div>
      </div>
      <Select items={items} value={account.cost_method} onValueChange={(method) => save.mutate(method as CostMethod)} disabled={save.isPending}>
        <SelectTrigger aria-label={t('gains.methodOf', { account: account.name })} className="w-56" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectPopup>
          {COST_METHODS.map((method) => (
            <SelectItem key={method} value={method}>
              {t(`gains.method.${method}`)}
            </SelectItem>
          ))}
        </SelectPopup>
      </Select>
    </div>
  )
}

/** The lots still held: each position, and what each part of it cost. */
function Lots() {
  const { t } = useTranslation()
  const lots = useLots()
  const accounts = usePortfolioAccounts()
  const names = useMemo(() => new Map((accounts.data ?? []).map((account) => [account.account_id, account.name])), [accounts.data])
  if (!lots.data || lots.data.length === 0) return null
  return (
    <section className="flex flex-col gap-2" aria-label={t('gains.lots')}>
      <h2 className="font-medium text-text">{t('gains.lots')}</h2>
      {lots.data.map((position) => (
        <PositionPanel key={`${position.account_id}:${position.instrument_id}`} position={position} account={names.get(position.account_id) ?? ''} />
      ))}
    </section>
  )
}

function PositionPanel({ position, account }: { position: PositionLots; account: string }) {
  const { t } = useTranslation()
  const locale = useLocale()
  return (
    <Panel>
      <div className="flex flex-wrap items-baseline justify-between gap-2 px-4 pt-3">
        <div>
          <span className="font-mono font-medium text-text">{position.symbol}</span> <span className="text-sm text-dim">· {account}</span>
        </div>
        <span className="text-sm text-dim">
          <Quantity amount={position.quantity} /> · {t(`gains.method.${position.method}`)}
        </span>
      </div>
      <TableScroll>
        <Table>
          <TableHead>
            <TableRow>
              <TableHeader>{t('gains.acquired')}</TableHeader>
              <TableHeader numeric>{t('trade.quantity')}</TableHeader>
              <TableHeader numeric className="hidden sm:table-cell">
                {t('gains.unitCost')}
              </TableHeader>
              <TableHeader numeric>{t('gains.cost')}</TableHeader>
            </TableRow>
          </TableHead>
          <TableBody>
            {position.lots.map((lot) => (
              <TableRow key={`${lot.trade_id}:${lot.acquired_at}`}>
                <TableCell className="whitespace-nowrap text-dim">{formatListDay(lot.acquired_on, locale)}</TableCell>
                <TableCell numeric>
                  <Quantity amount={lot.quantity} />
                </TableCell>
                <TableCell numeric className="hidden sm:table-cell">
                  <Money amount={lot.unit_cost} currency={lot.currency} />
                </TableCell>
                <TableCell numeric>
                  <Money amount={lot.cost} currency={lot.currency} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableScroll>
    </Panel>
  )
}
