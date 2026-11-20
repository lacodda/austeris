import { useMemo, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Merge, Paperclip, Trash2 } from 'lucide-react'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { useCommands } from '@/components/ui/commands'
import { Dialog } from '@/components/ui/dialog'
import { Drawer } from '@/components/ui/drawer'
import { EmptyState } from '@/components/ui/empty-state'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { useRowCursor, type RowCursor } from '@/lib/cursor'
import { formatListDay } from '@/lib/dates'
import { categoriesOf, movements } from '@/lib/entries'
import { isFiltered, readFilter, writeFilter } from '@/lib/filters'
import { categoryLabels, placeLabel, useAccounts, useCategories, useCounterparties, useEntries } from '@/lib/ledger'
import type { Entry, EntryFilter } from '@/lib/types'
import { BatchBar, BatchDialog, movableFrom, type BatchKind } from '@/pages/entries/Batch'
import { EntryDetail, title } from '@/pages/entries/EntryDetail'
import { EntryForm } from '@/pages/entries/EntryForm'
import { ExchangeForm } from '@/pages/entries/ExchangeForm'
import { Filters, PhoneFilters } from '@/pages/entries/Filters'
import { MergeDialog } from '@/pages/entries/Merge'
import { QuickLine } from '@/pages/entries/QuickLine'

/** Every movement of money, newest first, narrowed by what the address says.
 *
 * Sorting a pile of them is the screen's other job: entries are chosen - by a
 * box each, or X under the keyboard's cursor - and given a category, moved to
 * another account or labelled together, and a counterparty typed two ways is
 * merged into one. */
export function Entries() {
  const { t } = useTranslation()
  const [params, setParams] = useSearchParams()
  const filter = useMemo(() => readFilter(params), [params])
  const entries = useEntries(filter)
  const counterparties = useCounterparties()
  const rows = useMemo(() => entries.data?.pages.flatMap((page) => page.rows) ?? [], [entries.data])
  const [adding, setAdding] = useState(false)
  const [exchanging, setExchanging] = useState(false)
  const [opened, setOpened] = useState<string | null>(null)
  const [merging, setMerging] = useState(false)
  // The chosen entries, by id, for as long as the list is the same list: a
  // new filter is a new list, and what was chosen in the old one is not on it.
  const [chosen, setChosen] = useState<{ list: string; ids: Set<string> }>({ list: '', ids: new Set() })
  const list = params.toString()
  const ids = chosen.list === list ? chosen.ids : new Set<string>()
  const [acting, setActing] = useState<{ kind: BatchKind; entries: Entry[] } | null>(null)
  const table = useRef<HTMLDivElement>(null)

  const setFilter = (next: EntryFilter) => setParams(writeFilter(next), { replace: true })
  const choose = (next: Set<string>) => setChosen({ list, ids: next })
  const toggle = (id: string) => {
    const next = new Set(ids)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    choose(next)
  }
  const covered = adding || exchanging || opened !== null || merging || acting !== null
  const cursor = useRowCursor({
    table,
    count: rows.length,
    labels: { group: t('keys.list'), next: t('keys.next'), previous: t('keys.previous'), open: t('keys.open'), choose: t('keys.choose') },
    onChoose: (index) => {
      const entry = rows[index]
      if (entry) toggle(entry.id)
    },
    enabled: !covered,
  })

  // What a batch is done to: the chosen entries, or the one under the cursor
  // when none is chosen - one entry sorted by the same keys as a hundred.
  const targets = ids.size > 0 ? rows.filter((entry) => ids.has(entry.id)) : cursor.index === null ? [] : rows.slice(cursor.index, cursor.index + 1)
  const from = movableFrom(targets, filter.account)
  const act = (kind: BatchKind) => {
    if (targets.length === 0 || (kind === 'move' && from === undefined)) return
    setActing({ kind, entries: targets })
  }
  const merged = filter.counterparty ? counterparties.data?.find((each) => each.id === filter.counterparty) : undefined

  useCommands([
    { id: 'entries.new', label: t('entries.new'), group: t('keys.entries'), keys: 'N', run: () => setAdding(true), enabled: !covered },
    {
      id: 'entries.quick',
      label: t('keys.quick'),
      group: t('keys.entries'),
      keys: '/',
      run: () => document.querySelector<HTMLInputElement>('[data-quick-line]')?.focus(),
      enabled: !covered,
    },
    { id: 'entries.categorize', label: t('batch.categorize'), group: t('keys.entries'), keys: 'C', run: () => act('categorize'), enabled: !covered && targets.length > 0 },
    { id: 'entries.move', label: t('batch.move'), group: t('keys.entries'), keys: 'M', run: () => act('move'), enabled: !covered && from !== undefined && targets.length > 0 },
    { id: 'entries.tag', label: t('batch.tag'), group: t('keys.entries'), keys: 'T', run: () => act('tag'), enabled: !covered && targets.length > 0 },
    { id: 'entries.unchoose', label: t('batch.clear'), group: t('keys.entries'), keys: 'Escape', run: () => choose(new Set()), enabled: !covered && ids.size > 0 },
  ])

  return (
    <Container width="wide">
      <PageHeader
        title={t('entries.title')}
        description={t('entries.description')}
        actions={
          <>
            <Button variant="icon" render={<Link to="/entries/trash" />} aria-label={t('trash.title')}>
              <Trash2 aria-hidden />
            </Button>
            {/* A list narrowed to one counterparty is where a typo of its
                name is noticed, and where it is merged away. */}
            {merged ? (
              <Button onClick={() => setMerging(true)}>
                <Merge aria-hidden />
                {t('merge.action')}
              </Button>
            ) : null}
            <Button onClick={() => setExchanging(true)}>{t('entries.exchange')}</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>
              {t('entries.new')}
            </Button>
          </>
        }
      >
        {/* A list narrowed to one account records a line that names none
            against that account. */}
        <QuickLine accountId={filter.account} />
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
          filter.unsorted ? (
            <EmptyState
              variant="filtered"
              title={t('entries.noneUnsorted')}
              action={<Button onClick={() => setFilter({ ...filter, unsorted: undefined })}>{t('filters.clear')}</Button>}
            />
          ) : isFiltered(filter) ? (
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
        <div ref={table}>
          <Panel>
            <TableScroll>
              <EntryTable
                rows={rows}
                account={filter.account}
                onOpen={setOpened}
                chosen={ids}
                onToggle={toggle}
                onToggleAll={() => choose(ids.size === rows.length ? new Set() : new Set(rows.map((entry) => entry.id)))}
                cursor={cursor}
              />
            </TableScroll>
          </Panel>
        </div>
        {ids.size > 0 ? <BatchBar count={ids.size} movable={from !== undefined} onAct={act} onClear={() => choose(new Set())} /> : null}
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
      <BatchDialog
        kind={acting?.kind ?? null}
        entries={acting?.entries ?? []}
        from={from}
        onOpenChange={(open) => (open ? undefined : setActing(null))}
        onDone={() => {
          setActing(null)
          choose(new Set())
        }}
      />
      {merged ? (
        <MergeDialog
          counterparty={merged}
          open={merging}
          onOpenChange={setMerging}
          onMerged={(into) => {
            setMerging(false)
            setFilter({ ...filter, counterparty: into.id })
          }}
        />
      ) : null}
    </Container>
  )
}

interface EntryTableProps {
  rows: Entry[]
  /** The account the list is filtered to, whose side of each entry is shown. */
  account: string | undefined
  onOpen: (id: string) => void
  /** The entries chosen for a batch. */
  chosen: Set<string>
  onToggle: (id: string) => void
  onToggleAll: () => void
  cursor: RowCursor
}

function EntryTable({ rows, account, onOpen, chosen, onToggle, onToggleAll, cursor }: EntryTableProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const categories = useCategories()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const names = useMemo(() => new Map((accounts.data ?? []).map((each) => [each.id, each.name])), [accounts.data])
  const all = rows.length > 0 && chosen.size === rows.length

  return (
    <Table onFocus={cursor.onFocus}>
      <TableHead>
        <TableRow>
          <TableHeader className="w-10">
            <Checkbox
              aria-label={t('batch.chooseAll')}
              checked={all}
              indeterminate={chosen.size > 0 && !all}
              onCheckedChange={onToggleAll}
            />
          </TableHeader>
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
        {rows.map((entry, index) => {
          const moved = movements(entry, account)
          const what = [entry.place ? placeLabel(entry.place, locale) : null, ...entry.tags.map((tag) => `#${tag}`)].filter(Boolean)
          const draft = entry.lines.some((line) => line.side === 'unsorted')
          const category =
            categoriesOf(entry)
              .map((id) => labels.get(id))
              .filter(Boolean)
              .join(', ') || (moved.length > 1 ? t('entries.between') : '')
          const day = formatListDay(entry.occurred_on, locale)
          const name = title(entry, t('entry.untitled'))
          return (
            <TableRow key={entry.id} data-row={index} selected={cursor.index === index}>
              <TableCell>
                <Checkbox aria-label={t('batch.choose', { name })} checked={chosen.has(entry.id)} onCheckedChange={() => onToggle(entry.id)} />
              </TableCell>
              <TableCell className="hidden whitespace-nowrap text-dim sm:table-cell">{day}</TableCell>
              <TableCell>
                <div className="flex items-center gap-2">
                  {/* The entry opens from its name: a real button, reachable by
                      Tab and announced as one, rather than a clickable row. It
                      is also where the keyboard's cursor lands. */}
                  <Button variant="link" data-row-control className="text-left font-medium text-text" onClick={() => onOpen(entry.id)}>
                    {name}
                  </Button>
                  {entry.status === 'pending' ? <Badge variant="warn">{t('entry.heldBadge')}</Badge> : null}
                  {draft ? <Badge variant="info">{t('entries.unsortedBadge')}</Badge> : null}
                  {entry.attachments.length > 0 ? (
                    <Paperclip className="size-3.5 shrink-0 text-dim" aria-label={t('files.count', { count: entry.attachments.length })} />
                  ) : null}
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
