import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { formatListDay } from '@/lib/dates'
import { useAccounts } from '@/lib/ledger'
import { formatQuantity, useInstruments, useTrades } from '@/lib/portfolio'
import type { Trade } from '@/lib/types'
import { RecordTrade, TradeDialog } from '@/pages/portfolio/TradeDialog'

/** Every trade, the latest first: what, where, how many, at what price, and
 * what it moved the account's money by. Opened, one is changed or deleted. */
export function Trades() {
  const { t } = useTranslation()
  const locale = useLocale()
  const trades = useTrades()
  const accounts = useAccounts()
  const instruments = useInstruments()
  const [open, setOpen] = useState<Trade | null>(null)
  const accountName = useMemo(() => new Map((accounts.data ?? []).map((account) => [account.id, account.name])), [accounts.data])
  const symbol = useMemo(() => new Map((instruments.data ?? []).map((instrument) => [instrument.id, instrument.symbol])), [instruments.data])

  return (
    <div className="flex flex-col gap-4">
      <div className="flex justify-end">
        <RecordTrade />
      </div>
      <QueryState
        pending={trades.isPending}
        error={trades.error}
        empty={(trades.data ?? []).length === 0}
        emptyState={<EmptyState title={t('trade.emptyTitle')} body={t('trade.emptyBody')} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void trades.refetch()}
        retryLabel={t('common.retry')}
      >
        <Panel>
          <TableScroll>
            <Table>
              <TableHead>
                <TableRow>
                  <TableHeader>{t('trade.day')}</TableHeader>
                  <TableHeader>{t('trade.what')}</TableHeader>
                  <TableHeader numeric>{t('trade.quantity')}</TableHeader>
                  <TableHeader numeric className="hidden sm:table-cell">
                    {t('trade.price')}
                  </TableHeader>
                  <TableHeader numeric>{t('trade.money')}</TableHeader>
                </TableRow>
              </TableHead>
              <TableBody>
                {(trades.data ?? []).map((trade) => (
                  <TableRow key={trade.id}>
                    <TableCell className="whitespace-nowrap text-dim">{formatListDay(trade.occurred_on, locale)}</TableCell>
                    <TableCell>
                      {/* A button rather than a clickable row: reachable by
                          Tab, and it says what opens. */}
                      <Button variant="link" onClick={() => setOpen(trade)} className="font-medium text-text">
                        <Badge variant={trade.kind === 'buy' ? 'good' : trade.kind === 'sell' ? 'info' : 'soft'}>{t(`trade.kinds.${trade.kind}`)}</Badge>{' '}
                        <span className="font-mono">{symbol.get(trade.instrument_id) ?? ''}</span>
                      </Button>
                      <div className="text-xs text-dim">
                        {trade.to_account_id
                          ? t('trade.fromTo', { from: accountName.get(trade.account_id) ?? '', to: accountName.get(trade.to_account_id) ?? '' })
                          : accountName.get(trade.account_id)}
                      </div>
                    </TableCell>
                    <TableCell numeric>{formatQuantity(trade.quantity, locale)}</TableCell>
                    <TableCell numeric className="hidden text-dim sm:table-cell">
                      {trade.price && trade.currency ? <Money amount={trade.price} currency={trade.currency} /> : null}
                    </TableCell>
                    <TableCell numeric>
                      {trade.money && trade.currency ? (
                        <Money amount={trade.money} currency={trade.currency} signDisplay="exceptZero" />
                      ) : (
                        <span className="text-dim">{t('trade.noMoneyShort')}</span>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableScroll>
        </Panel>
      </QueryState>
      {open ? <TradeDialog trade={open} open onOpenChange={(still) => (still ? undefined : setOpen(null))} /> : null}
    </div>
  )
}
