import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DateRangePicker } from '@/components/ui/date-range-picker'
import { Drawer, DrawerBody, DrawerClose, DrawerHeader, DrawerPopup, DrawerTitle } from '@/components/ui/drawer'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { isFiltered } from '@/lib/filters'
import { categoryLabels, useAccounts, useCategories, useCounterparties, useTags } from '@/lib/ledger'
import type { EntryFilter, EntryStatus } from '@/lib/types'

/** The value a Select holds for "no filter on this". */
const ANY = ''

interface FiltersProps {
  filter: EntryFilter
  onFilterChange: (filter: EntryFilter) => void
  /** One control under another, full width - the phone's sheet. */
  stacked?: boolean
}

/** What narrows the list: a period, an account, a category, a tag, a
 * counterparty, and whether the bank has posted it. */
export function Filters({ filter, onFilterChange, stacked = false }: FiltersProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const categories = useCategories()
  const tags = useTags()
  const counterparties = useCounterparties()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])

  const set = (change: Partial<EntryFilter>) => {
    const next = { ...filter, ...change }
    for (const key of Object.keys(next) as (keyof EntryFilter)[]) if (!next[key]) delete next[key]
    onFilterChange(next)
  }

  const accountItems = options(t('filters.anyAccount'), (accounts.data ?? []).map((account) => [account.id, account.name]))
  const categoryItems = options(
    t('filters.anyCategory'),
    (categories.data ?? [])
      .map((category) => [category.id, labels.get(category.id) ?? category.name] as [string, string])
      .sort((a, b) => a[1].localeCompare(b[1], locale)),
  )
  const tagItems = options(t('filters.anyTag'), (tags.data ?? []).map((tag) => [tag.name, `#${tag.name}`]))
  const counterpartyItems = options(
    t('filters.anyCounterparty'),
    (counterparties.data ?? []).map((counterparty) => [counterparty.id, counterparty.name]),
  )

  return (
    <div className={stacked ? 'flex flex-col items-stretch gap-3' : 'flex flex-wrap items-center gap-2'}>
      <DateRangePicker
        aria-label={t('filters.period')}
        value={{ start: filter.from, end: filter.to }}
        onValueChange={(range) => {
          // Half a range is a range being chosen; the list waits for both ends
          // rather than refetching on the first click.
          if (range.start && !range.end) return
          set({ from: range.start, to: range.end })
        }}
        placeholder={t('filters.anyPeriod')}
        previousMonthLabel={t('calendar.previous')}
        nextMonthLabel={t('calendar.next')}
        className={stacked ? 'w-full' : 'w-auto'}
      />
      <FilterSelect wide={stacked} label={t('filters.account')} items={accountItems} value={filter.account} onChange={(account) => set({ account })} />
      <FilterSelect wide={stacked} label={t('filters.category')} items={categoryItems} value={filter.category} onChange={(category) => set({ category })} />
      {Object.keys(tagItems).length > 1 ? (
        <FilterSelect wide={stacked} label={t('filters.tag')} items={tagItems} value={filter.tag} onChange={(tag) => set({ tag })} />
      ) : null}
      {Object.keys(counterpartyItems).length > 1 ? (
        <FilterSelect
          wide={stacked}
          label={t('filters.counterparty')}
          items={counterpartyItems}
          value={filter.counterparty}
          onChange={(counterparty) => set({ counterparty })}
        />
      ) : null}
      <SegmentedControl
        aria-label={t('filters.status')}
        value={filter.status ?? ANY}
        onValueChange={(status) => set({ status: (status || undefined) as EntryStatus | undefined })}
      >
        <Segment value={ANY}>{t('filters.allStatuses')}</Segment>
        <Segment value="pending">{t('filters.held')}</Segment>
        <Segment value="cleared">{t('filters.posted')}</Segment>
      </SegmentedControl>
      {isFiltered(filter) ? (
        <Button variant="link" className="text-sm" onClick={() => onFilterChange({})}>
          {t('filters.clear')}
        </Button>
      ) : null}
    </div>
  )
}

function options(any: string, rows: [string, string][]): Record<string, string> {
  return Object.fromEntries([[ANY, any], ...rows])
}

function FilterSelect({
  label,
  items,
  value,
  onChange,
  wide,
}: {
  wide: boolean
  label: string
  items: Record<string, string>
  value: string | undefined
  onChange: (value: string | undefined) => void
}) {
  return (
    <Select items={items} value={value ?? ANY} onValueChange={(next) => onChange((next as string) || undefined)}>
      <SelectTrigger aria-label={label} size="sm" className={wide ? 'w-full' : 'w-auto max-w-56 min-w-36'}>
        <SelectValue />
      </SelectTrigger>
      <SelectPopup>
        {Object.entries(items).map(([id, name]) => (
          <SelectItem key={id} value={id}>
            {name}
          </SelectItem>
        ))}
      </SelectPopup>
    </Select>
  )
}

/**
 * The phone's filters: one button, and the same controls in a sheet from the
 * bottom. Laid inline they took five rows of a 390-pixel screen before the
 * first entry; the button says how many are on, so a narrowed list never
 * passes for the whole of it.
 */
export function PhoneFilters({ filter, onFilterChange }: Omit<FiltersProps, 'stacked'>) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const count = Object.keys(filter).filter((key) => key !== 'to').length
  return (
    <Drawer swipeDirection="down" open={open} onOpenChange={setOpen}>
      <Button className="self-start sm:hidden" onClick={() => setOpen(true)}>
        {t('filters.title')}
        {count > 0 ? <Badge variant="accent">{count}</Badge> : null}
      </Button>
      <DrawerPopup side="bottom" size="lg">
        <DrawerHeader>
          <DrawerTitle>{t('filters.title')}</DrawerTitle>
        </DrawerHeader>
        <DrawerBody>
          <Filters filter={filter} onFilterChange={onFilterChange} stacked />
          <Button variant="primary" className="mt-4 w-full" render={<DrawerClose />}>
            {t('filters.show')}
          </Button>
        </DrawerBody>
      </DrawerPopup>
    </Drawer>
  )
}
