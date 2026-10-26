import { NavLink, Navigate, Route, Routes } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { EmptyState } from '@/components/ui/empty-state'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { formatMoment } from '@/lib/dates'
import { formatRate } from '@/lib/money'
import { useAccounts } from '@/lib/ledger'
import { formatQuantity, holdingAccounts, useValue, useValueCurrency } from '@/lib/portfolio'
import type { AccountValue, Holding } from '@/lib/types'
import { Instruments } from '@/pages/portfolio/Instruments'
import { SnapshotView, Snapshots, TakeSnapshot } from '@/pages/portfolio/Snapshots'
import { RecordTrade } from '@/pages/portfolio/TradeDialog'
import { Trades } from '@/pages/portfolio/Trades'

/** The portfolio's parts, in the order its tabs list them. */
const PARTS = [
  { id: 'holdings', path: '/portfolio' },
  { id: 'trades', path: '/portfolio/trades' },
  { id: 'instruments', path: '/portfolio/instruments' },
  { id: 'snapshots', path: '/portfolio/snapshots' },
] as const

/**
 * What a person holds at brokers and on exchanges, and what it is worth.
 *
 * The holdings first - the question the screen is opened with - then the
 * trades they add up from, the instruments and their prices, and snapshots
 * to compare with.
 */
export function Portfolio() {
  const { t } = useTranslation()
  return (
    <Container>
      <PageHeader title={t('portfolio.title')} description={t('portfolio.description')} />
      <nav aria-label={t('portfolio.parts')} className="mb-5 flex gap-1 overflow-x-auto border-b border-line">
        {PARTS.map((part) => (
          <NavLink
            key={part.id}
            to={part.path}
            end
            className={({ isActive }) =>
              `-mb-px border-b-2 px-3 py-2 text-sm whitespace-nowrap ${
                isActive ? 'border-accent font-semibold text-text' : 'border-transparent text-dim hover:text-text'
              }`
            }
          >
            {t(`portfolio.part.${part.id}`)}
          </NavLink>
        ))}
      </nav>
      <Routes>
        <Route index element={<Holdings />} />
        <Route path="trades" element={<Trades />} />
        <Route path="instruments" element={<Instruments />} />
        <Route path="snapshots" element={<Snapshots />} />
        <Route path="snapshots/:id" element={<SnapshotView />} />
        <Route path="*" element={<Navigate to="/portfolio" replace />} />
      </Routes>
    </Container>
  )
}

/** Each broker's and wallet's account: the money on it and every position,
 * what each is worth, and what it all adds up to. */
function Holdings() {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const [currency, setCurrency, currencies] = useValueCurrency(accounts.data)
  const value = useValue(currency)
  const places = holdingAccounts(accounts.data)
  const choices = Object.fromEntries(currencies.map((code) => [code, code]))

  return (
    <QueryState
      pending={accounts.isPending || (places.length > 0 && value.isPending)}
      error={accounts.error ?? value.error}
      empty={places.length === 0 && (value.data?.accounts.length ?? 0) === 0}
      emptyState={<EmptyState title={t('portfolio.emptyTitle')} body={t('portfolio.emptyBody')} />}
      errorLabels={{ title: t('common.loadFailed') }}
      onRetry={() => void (accounts.error ? accounts.refetch() : value.refetch())}
      retryLabel={t('common.retry')}
    >
      {value.data && currency ? (
        <div className="flex flex-col gap-4">
          <Panel className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-4">
            <div className="flex flex-col gap-1">
              <span className="caption">{t('portfolio.worth')}</span>
              <Money amount={value.data.total} currency={value.data.currency} className="text-2xl font-semibold text-text" />
              {value.data.unvalued.length > 0 ? (
                <span className="text-sm text-warn">{t('portfolio.unvalued', { symbols: value.data.unvalued.join(', ') })}</span>
              ) : null}
              {value.data.stale ? <span className="text-sm text-dim">{t('portfolio.stale')}</span> : null}
            </div>
            <div className="flex flex-wrap items-center gap-2">
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
              <TakeSnapshot currency={currency} />
              <RecordTrade />
            </div>
          </Panel>
          {value.data.accounts.map((account) => (
            <AccountPanel key={account.account_id} account={account} currency={value.data.currency} locale={locale} />
          ))}
          <p className="text-xs text-faint">{t('portfolio.asOf', { moment: formatMoment(value.data.at, locale) })}</p>
        </div>
      ) : null}
    </QueryState>
  )
}

function AccountPanel({ account, currency, locale }: { account: AccountValue; currency: string; locale: string }) {
  const { t } = useTranslation()
  return (
    <section className="flex flex-col gap-2" aria-label={account.name}>
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="font-medium text-text">{account.name}</h2>
        <Money amount={account.total} currency={currency} className="font-medium" />
      </div>
      <Panel>
        <TableScroll>
          {/* Fixed columns, so the accounts' tables line up under each
              other and read as one. */}
          <Table className="table-fixed">
            <TableHead>
              <TableRow>
                <TableHeader>{t('portfolio.what')}</TableHeader>
                <TableHeader numeric className="w-[30%] sm:w-[22%]">
                  {t('portfolio.quantity')}
                </TableHeader>
                <TableHeader numeric className="hidden w-[22%] sm:table-cell">
                  {t('portfolio.price', { currency })}
                </TableHeader>
                <TableHeader numeric className="w-[30%] sm:w-[22%]">
                  {t('portfolio.value', { currency })}
                </TableHeader>
              </TableRow>
            </TableHead>
            <TableBody>
              {account.holdings.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={4} className="text-dim">
                    {t('portfolio.nothingHeld')}
                  </TableCell>
                </TableRow>
              ) : (
                account.holdings.map((holding) => (
                  <HoldingRow key={`${holding.kind}:${holding.instrument_id ?? holding.symbol}`} holding={holding} currency={currency} locale={locale} />
                ))
              )}
            </TableBody>
          </Table>
        </TableScroll>
      </Panel>
    </section>
  )
}

function HoldingRow({ holding, currency, locale }: { holding: Holding; currency: string; locale: string }) {
  const { t } = useTranslation()
  const cash = holding.kind === 'cash'
  const staleNote = holding.stale && holding.priced_at ? t('portfolio.stalePrice', { moment: formatMoment(holding.priced_at, locale) }) : undefined
  return (
    <TableRow>
      <TableCell>
        <div className="flex items-center gap-2">
          <span className="font-mono font-medium text-text">{holding.symbol}</span>
          {cash ? <Badge variant="soft">{t('portfolio.money')}</Badge> : null}
        </div>
        {holding.name ? <div className="text-xs text-dim">{holding.name}</div> : null}
      </TableCell>
      <TableCell numeric>{cash ? <Money amount={holding.quantity} currency={holding.symbol} /> : formatQuantity(holding.quantity, locale)}</TableCell>
      <TableCell numeric className="hidden text-dim sm:table-cell">
        {/* Money in the currency it is valued in has no price, and money in
            another one has a rate - written as one, to the digits that
            tell rates apart. */}
        {holding.price && !(cash && holding.symbol === currency) ? (
          <span title={staleNote}>
            {cash ? formatRate(holding.price, locale) : <Money amount={holding.price} currency={currency} />}
            {holding.stale ? <span className="text-warn"> *</span> : null}
          </span>
        ) : null}
      </TableCell>
      <TableCell numeric>
        {holding.value ? (
          <span title={staleNote}>
            <Money amount={holding.value} currency={currency} />
            {holding.stale ? <span className="text-warn sm:hidden"> *</span> : null}
          </span>
        ) : (
          <span className="text-warn">{t('portfolio.noPrice')}</span>
        )}
      </TableCell>
    </TableRow>
  )
}
