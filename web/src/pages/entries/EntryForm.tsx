import { useMemo, useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Plus, X } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import { Picker, type Picked } from '@/components/Picker'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DatePicker } from '@/components/ui/date-picker'
import { DrawerActions, DrawerBody, DrawerClose, DrawerHeader, DrawerPopup, DrawerTitle } from '@/components/ui/drawer'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { today } from '@/lib/dates'
import { parseAmount, sign, sum } from '@/lib/decimal'
import { splitLines, transferLines, type Direction } from '@/lib/entries'
import { categoryLabels, openAccounts, useAccounts, useBooksChanged, useCategories, useCounterparties } from '@/lib/ledger'
import type { Flow, NewEntry, PlaceBody } from '@/lib/types'
import { AccountSelect, CounterpartyField, isTag, PlaceField, TagsField } from '@/pages/entries/fields'
import { resolver } from '@/pages/entries/resolve'

type Kind = Direction | 'transfer'

interface DraftLine {
  key: number
  category: Picked | null
  amount: string
  note: string
}

let nextKey = 0
const blankLine = (): DraftLine => ({ key: nextKey++, category: null, amount: '', note: '' })

/**
 * Recording an entry by hand: an expense or an income split across
 * categories, or money moved between two accounts in one currency.
 *
 * The account's side is never typed. It is the sum of the split, worked out
 * exactly (`splitLines`), so the entry balances by construction - the
 * person types what each part was, and the total follows.
 *
 * Money in another currency than the account's is the one-line entry's job
 * (`10 usd fun streaming @5965`) or an exchange's: a split in two currencies
 * needs a conversion the person cannot see, and this form would have to
 * invent one.
 */
export function EntryForm({ onDone, defaultAccount }: { onDone: () => void; defaultAccount?: string }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const accounts = useAccounts()
  const categories = useCategories()
  const counterparties = useCounterparties()
  const usable = useMemo(() => openAccounts(accounts.data), [accounts.data])

  const [kind, setKind] = useState<Kind>('expense')
  const [account, setAccount] = useState<string | undefined>(defaultAccount)
  const [to, setTo] = useState<string | undefined>()
  const [date, setDate] = useState(today())
  const [description, setDescription] = useState('')
  const [counterparty, setCounterparty] = useState<Picked | null>(null)
  const [lines, setLines] = useState<DraftLine[]>(() => [blankLine()])
  const [amount, setAmount] = useState('')
  const [tags, setTags] = useState<string[]>([])
  const [place, setPlace] = useState<PlaceBody | null>(null)
  const [held, setHeld] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)

  const from = usable.find((each) => each.id === account)
  const target = usable.find((each) => each.id === to)
  const flow: Flow = kind === 'income' ? 'income' : 'expense'
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const categoryOptions = useMemo(
    () =>
      (categories.data ?? [])
        .filter((category) => category.flow === flow)
        .map((category) => ({ value: category.id, label: labels.get(category.id) ?? category.name }))
        .sort((a, b) => a.label.localeCompare(b.label, locale)),
    [categories.data, flow, labels, locale],
  )

  const parsed = lines.map((line) => (line.amount.trim() === '' ? null : parseAmount(line.amount, locale)))
  const total = parsed.every((value) => value !== null) ? sum(parsed as string[]) : null

  const update = (key: number, change: Partial<DraftLine>) =>
    setLines((current) => current.map((line) => (line.key === key ? { ...line, ...change } : line)))

  /** A counterparty with a usual category fills in the first line, when the
   * first line has none yet and the category goes the way this entry does. */
  function chooseCounterparty(picked: Picked | null) {
    setCounterparty(picked)
    if (picked === null || !('id' in picked)) return
    const usual = counterparties.data?.find((each) => each.id === picked.id)?.default_category_id
    const category = categories.data?.find((each) => each.id === usual)
    const first = lines[0]
    if (category && category.flow === flow && first && first.category === null) {
      update(first.key, { category: { id: category.id } })
    }
  }

  const record = useMutation({
    mutationFn: async () => {
      const resolve = resolver()
      const shared = {
        description: description.trim(),
        occurred_on: date,
        tags,
        place: place && place.country ? place : null,
        pending: held,
      }
      if (kind === 'transfer') {
        const value = parseAmount(amount, locale)
        if (!from || !target || value === null) throw new Error(t('form.incomplete'))
        const entry: NewEntry = { ...shared, lines: transferLines(from.id, target.id, from.currency, value) }
        return api.createEntry(entry)
      }
      if (!from) throw new Error(t('form.incomplete'))
      const split = await Promise.all(
        lines.map(async (line, index) => {
          const value = parsed[index]
          if (!line.category || !value) throw new Error(t('form.incomplete'))
          return { categoryId: await resolve.category(line.category, flow), amount: value, note: line.note.trim() }
        }),
      )
      const counterpartyId = await resolve.counterparty(counterparty, split[0]?.categoryId)
      const entry: NewEntry = {
        ...shared,
        counterparty_id: counterpartyId,
        lines: splitLines(kind, from.id, from.currency, split),
      }
      return api.createEntry(entry)
    },
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('entry.recorded') })
      onDone()
    },
    // Whatever was created on the way - a category, a counterparty - is real
    // now, so the lists are refreshed even when the entry itself failed.
    onError: (error: Error) => {
      void changed()
      setProblem(error.message)
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    if (tags.some((tag) => !isTag(tag))) return
    const amounts = kind === 'transfer' ? [parseAmount(amount, locale)] : parsed
    if (amounts.some((value) => value === null || sign(value) <= 0)) {
      setProblem(t('form.positiveAmounts'))
      return
    }
    if (kind === 'transfer' && from && target && from.currency !== target.currency) {
      setProblem(t('entry.transferCurrencies'))
      return
    }
    record.mutate()
  }

  return (
    <DrawerPopup
      side="right"
      size="lg"
      // dowel 0.35 lays the popup at the start of its viewport whatever the
      // side; ordered from dowel, worked round here.
      className="ml-auto"
    >
      <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
        <DrawerHeader action={<Button variant="icon" size="icon-sm" render={<DrawerClose />} aria-label={t('common.close')}><X aria-hidden /></Button>}>
          <DrawerTitle>{t('entry.newTitle')}</DrawerTitle>
        </DrawerHeader>
        <DrawerBody className="flex flex-col gap-4">
          <SegmentedControl aria-label={t('entry.kind')} value={kind} onValueChange={(value) => setKind(value as Kind)}>
            <Segment value="expense">{t('entry.expense')}</Segment>
            <Segment value="income">{t('entry.income')}</Segment>
            <Segment value="transfer">{t('entry.transfer')}</Segment>
          </SegmentedControl>

          <div className="grid gap-4 sm:grid-cols-2">
            <AccountSelect
              accounts={usable}
              value={account}
              onValueChange={setAccount}
              label={kind === 'transfer' ? t('entry.from') : t('entry.account')}
            />
            {kind === 'transfer' ? (
              <AccountSelect accounts={usable} value={to} onValueChange={setTo} label={t('entry.to')} exclude={account} />
            ) : (
              <Field label={t('entry.date')} required>
                <DatePicker
                  value={date}
                  onValueChange={setDate}
                  placeholder={t('entry.date')}
                  previousMonthLabel={t('calendar.previous')}
                  nextMonthLabel={t('calendar.next')}
                />
              </Field>
            )}
          </div>

          <Field label={t('entry.description')}>
            <Input value={description} onChange={(event) => setDescription(event.target.value)} />
          </Field>

          {kind === 'transfer' ? (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label={t('entry.amount')} required help={from ? from.currency : undefined}>
                <AmountInput value={amount} onChange={(event) => setAmount(event.target.value)} />
              </Field>
              <Field label={t('entry.date')} required>
                <DatePicker
                  value={date}
                  onValueChange={setDate}
                  placeholder={t('entry.date')}
                  previousMonthLabel={t('calendar.previous')}
                  nextMonthLabel={t('calendar.next')}
                />
              </Field>
            </div>
          ) : (
            <>
              <CounterpartyField value={counterparty} onValueChange={chooseCounterparty} />
              <fieldset className="flex flex-col gap-2">
                <legend className="caption mb-2">{t('entry.split')}</legend>
                {lines.map((line, index) => (
                  <div key={line.key} className="grid grid-cols-[minmax(0,1fr)_8rem_auto] items-start gap-2">
                    <div className="flex flex-col gap-1">
                      <Picker
                        aria-label={t('entry.lineCategory', { n: index + 1 })}
                        options={categoryOptions}
                        value={line.category}
                        onValueChange={(category) => update(line.key, { category })}
                        placeholder={t('entry.category')}
                        emptyLabel={t('picker.empty')}
                        createLabel={(name) => t(flow === 'income' ? 'picker.createIncome' : 'picker.createExpense', { name })}
                      />
                      {lines.length > 1 ? (
                        <Input
                          aria-label={t('entry.lineNote', { n: index + 1 })}
                          placeholder={t('entry.note')}
                          value={line.note}
                          onChange={(event) => update(line.key, { note: event.target.value })}
                          className="h-control-sm text-xs"
                        />
                      ) : null}
                    </div>
                    <AmountInput
                      aria-label={t('entry.lineAmount', { n: index + 1 })}
                      value={line.amount}
                      onChange={(event) => update(line.key, { amount: event.target.value })}
                      aria-invalid={line.amount.trim() !== '' && parsed[index] === null ? true : undefined}
                    />
                    <Button
                      variant="icon"
                      size="icon-md"
                      aria-label={t('entry.removeLine', { n: index + 1 })}
                      disabled={lines.length === 1}
                      onClick={() => setLines((current) => current.filter((each) => each.key !== line.key))}
                    >
                      <X aria-hidden />
                    </Button>
                  </div>
                ))}
                <div className="flex items-center justify-between gap-2">
                  <Button size="sm" onClick={() => setLines((current) => [...current, blankLine()])}>
                    <Plus aria-hidden />
                    {t('entry.addLine')}
                  </Button>
                  {from && total !== null ? (
                    <span className="text-sm text-dim">
                      {t('entry.total')}{' '}
                      <Money amount={total} currency={from.currency} className="font-semibold text-text" />
                    </span>
                  ) : null}
                </div>
              </fieldset>
            </>
          )}

          <TagsField value={tags} onValueChange={setTags} />
          <PlaceField value={place} onValueChange={setPlace} />
          <Checkbox checked={held} onCheckedChange={setHeld}>
            {t('entry.held')}
          </Checkbox>
          {problem ? (
            <Alert tone="bad" role="alert">
              {problem}
            </Alert>
          ) : null}
        </DrawerBody>
        <DrawerActions>
          <Button render={<DrawerClose />}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" disabled={record.isPending}>
            {t('entry.record')}
          </Button>
        </DrawerActions>
      </form>
    </DrawerPopup>
  )
}
