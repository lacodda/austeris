import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Drawer } from '@/components/ui/drawer'
import { EmptyState } from '@/components/ui/empty-state'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { formatListDay } from '@/lib/dates'
import { categoriesOf, movements } from '@/lib/entries'
import { isFiltered, readFilter, writeFilter } from '@/lib/filters'
import { categoryLabels, placeLabel, useAccounts, useCategories, useEntries } from '@/lib/ledger'
import type { Entry, EntryFilter } from '@/lib/types'
import { EntryDetail, title } from '@/pages/entries/EntryDetail'
import { EntryForm } from '@/pages/entries/EntryForm'
import { ExchangeForm } from '@/pages/entries/ExchangeForm'
import { Filters, PhoneFilters } from '@/pages/entries/Filters'
import { QuickLine } from '@/pages/entries/QuickLine'

/** Every movement of money, newest first, narrowed by what the address says. */
export function Entries() {
  const { t } = useTranslation()
  const [params, setParams] = useSearchParams()
  const filter = useMemo(() => readFilter(params), [params])
  const entries = useEntries(filter)
  const rows = useMemo(() => entries.data?.pages.flatMap((page) => page.rows) ?? [], [entries.data])
  const [adding, setAdding] = useState(false)
  const [exchanging, setExchanging] = useState(false)
  const [opened, setOpened] = useState<string | null>(null)

  const setFilter = (next: EntryFilter) => setParams(writeFilter(next), { replace: true })

  return (
    <Container width="wide">
      <PageHeader
        title={t('entries.title')}
        description={t('entries.description')}
        actions={
          <>
            <Button onClick={() => setExchanging(true)}>{t('entries.exchange')}</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>
              {t('entries.new')}
            </Button>
          </>
        }
      >
        <QuickLine />
        <div className="hidden sm:block">
          <Filters filter={filter} onFilterChange={setFilter} />
        </div>
        <PhoneFilters filter={filter} onFilterChange={setFilter} />
      </PageHeader>

      <QueryState
        pending={entries.isPending}
        error={entries.error}
        empty={rows.length === 0}
        emptyState={
          isFiltered(filter) ? (
            <EmptyState
              variant="filtered"
              title={t('entries.noneMatch')}
              action={<Button onClick={() => setFilter({})}>{t('filters.clear')}</Button>}
            />
          ) : (
            <EmptyState title={t('entries.emptyTitle')} body={t('entries.emptyBody')} />
          )
        }
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void entries.refetch()}
        retryLabel={t('common.retry')}
      >
        <Panel>
          <TableScroll>
            <EntryTable rows={rows} account={filter.account} onOpen={setOpened} />
          </TableScroll>
        </Panel>
        {entries.hasNextPage ? (
          <div className="mt-3 flex justify-center">
            <Button onClick={() => void entries.fetchNextPage()} disabled={entries.isFetchingNextPage}>
              {t('entries.more')}
            </Button>
          </div>
        ) : null}
      </QueryState>

      <Drawer swipeDirection="right" open={adding} onOpenChange={setAdding}>
        {adding ? <EntryForm defaultAccount={filter.account} onDone={() => setAdding(false)} /> : null}
      </Drawer>
      <Dialog open={exchanging} onOpenChange={setExchanging}>
        {exchanging ? <ExchangeForm onDone={() => setExchanging(false)} /> : null}
      </Dialog>
      <Drawer swipeDirection="right" open={opened !== null} onOpenChange={(open) => (open ? undefined : setOpened(null))}>
        {opened ? <EntryDetail id={opened} onDone={() => setOpened(null)} /> : null}
      </Drawer>
    </Container>
  )
}

interface EntryTableProps {
  rows: Entry[]
  /** The account the list is filtered to, whose side of each entry is shown. */
  account: string | undefined
  onOpen: (id: string) => void
}

function EntryTable({ rows, account, onOpen }: EntryTableProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const categories = useCategories()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const names = useMemo(() => new Map((accounts.data ?? []).map((each) => [each.id, each.name])), [accounts.data])

  return (
    <Table>
      <TableHead>
        <TableRow>
          {/* On a phone the date and the category move under the name, so the
              amount - what the list is read for - stays on the screen rather
              than past its right edge. */}
          <TableHeader className="hidden w-28 sm:table-cell">{t('entries.date')}</TableHeader>
          <TableHeader>{t('entries.what')}</TableHeader>
          <TableHeader className="hidden sm:table-cell">{t('entries.category')}</TableHeader>
          <TableHeader numeric>{t('entries.amount')}</TableHeader>
        </TableRow>
      </TableHead>
      <TableBody>
        {rows.map((entry) => {
          const moved = movements(entry, account)
          const what = [entry.place ? placeLabel(entry.place, locale) : null, ...entry.tags.map((tag) => `#${tag}`)].filter(Boolean)
          const category =
            categoriesOf(entry)
              .map((id) => labels.get(id))
              .filter(Boolean)
              .join(', ') || (moved.length > 1 ? t('entries.between') : '')
          const day = formatListDay(entry.occurred_on, locale)
          return (
            <TableRow key={entry.id}>
              <TableCell className="hidden whitespace-nowrap text-dim sm:table-cell">{day}</TableCell>
              <TableCell>
                <div className="flex items-center gap-2">
                  {/* The entry opens from its name: a real button, reachable by
                      Tab and announced as one, rather than a clickable row. */}
                  <Button variant="link" className="text-left font-medium text-text" onClick={() => onOpen(entry.id)}>
                    {title(entry, t('entry.untitled'))}
                  </Button>
                  {entry.status === 'pending' ? <Badge variant="warn">{t('entry.heldBadge')}</Badge> : null}
                </div>
                {entry.description && entry.counterparty ? <div className="text-xs text-dim">{entry.counterparty.name}</div> : null}
                {what.length > 0 ? <div className="text-xs text-faint">{what.join(' · ')}</div> : null}
                <div className="text-xs text-dim sm:hidden">{[day, category].filter(Boolean).join(' · ')}</div>
              </TableCell>
              <TableCell className="hidden text-dim sm:table-cell">{category}</TableCell>
              <TableCell numeric>
                {moved.map((movement) => (
                  <div key={movement.accountId}>
                    <Money amount={movement.amount} currency={movement.currency} signDisplay="exceptZero" />
                    {account ? null : <div className="text-xs text-dim">{names.get(movement.accountId)}</div>}
                  </div>
                ))}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}
