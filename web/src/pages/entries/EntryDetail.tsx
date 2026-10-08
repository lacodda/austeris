import { useMemo, useState, type FormEvent } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { X } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import type { Picked } from '@/components/Picker'
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
import { DrawerActions, DrawerBody, DrawerClose, DrawerHeader, DrawerPopup, DrawerTitle } from '@/components/ui/drawer'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { KeyValue, KeyValueRow } from '@/components/ui/key-value'
import { QueryState } from '@/components/ui/query-state'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatDay, today } from '@/lib/dates'
import { parseAmount, sign } from '@/lib/decimal'
import { categoryLabels, placeLabel, useAccounts, useBooksChanged, useCategories } from '@/lib/ledger'
import type { Entry, PlaceBody } from '@/lib/types'
import { CounterpartyField, isTag, PlaceField, TagsField } from '@/pages/entries/fields'
import { resolver } from '@/pages/entries/resolve'

type Mode = 'view' | 'edit' | 'post'

/** One entry, opened from the list: every line of it, and what can be done to
 * it - its details changed, a held payment posted, the entry deleted. */
export function EntryDetail({ id, onDone }: { id: string; onDone: () => void }) {
  const { t } = useTranslation()
  const entry = useQuery({ queryKey: ['ledger', 'entry', id], queryFn: () => api.entry(id) })
  const [mode, setMode] = useState<Mode>('view')

  return (
    <DrawerPopup
      side="right"
      size="lg"
      // dowel 0.35 lays the popup at the start of its viewport whatever the
      // side; ordered from dowel, worked round here.
      className="ml-auto"
    >
      <DrawerHeader
        action={
          <Button variant="icon" size="icon-sm" render={<DrawerClose />} aria-label={t('common.close')}>
            <X aria-hidden />
          </Button>
        }
      >
        <DrawerTitle>{entry.data ? title(entry.data, t('entry.untitled')) : t('entry.title')}</DrawerTitle>
      </DrawerHeader>
      {/* The modes are the drawer's own column - body, then actions pinned to
          the foot - so they sit directly in it; only the waiting and the
          failing go through QueryState, inside a body of their own. */}
      {entry.data ? (
        mode === 'edit' ? (
          <EditEntry entry={entry.data} onDone={() => setMode('view')} />
        ) : mode === 'post' ? (
          <PostEntry entry={entry.data} onDone={() => setMode('view')} />
        ) : (
          <ViewEntry entry={entry.data} onEdit={() => setMode('edit')} onPost={() => setMode('post')} onDeleted={onDone} />
        )
      ) : (
        <DrawerBody>
          <QueryState
            pending={entry.isPending}
            error={entry.error}
            errorLabels={{ title: t('common.loadFailed') }}
            onRetry={() => void entry.refetch()}
            retryLabel={t('common.retry')}
          >
            {null}
          </QueryState>
        </DrawerBody>
      )}
    </DrawerPopup>
  )
}

/** What an entry is called in a list and a heading. */
export function title(entry: Entry, untitled: string): string {
  return entry.description || entry.counterparty?.name || untitled
}

function ViewEntry({ entry, onEdit, onPost, onDeleted }: { entry: Entry; onEdit: () => void; onPost: () => void; onDeleted: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const categories = useCategories()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const accountName = (accountId: string) => accounts.data?.find((account) => account.id === accountId)?.name ?? ''
  const changed = useBooksChanged()
  const toasts = useToastManager()

  const remove = useMutation({
    mutationFn: () => api.deleteEntry(entry.id),
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('entry.deleted') })
      onDeleted()
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: error.message }),
  })

  return (
    <>
      <DrawerBody className="flex flex-col gap-5">
        <KeyValue>
          <KeyValueRow label={t('entry.date')}>{formatDay(entry.occurred_on, locale)}</KeyValueRow>
          <KeyValueRow label={t('entry.status')}>
            {entry.status === 'pending' ? (
              <Badge variant="warn">{t('entry.heldBadge')}</Badge>
            ) : entry.cleared_on && entry.cleared_on !== entry.occurred_on ? (
              t('entry.postedOn', { date: formatDay(entry.cleared_on, locale) })
            ) : (
              t('entry.posted')
            )}
          </KeyValueRow>
          {entry.counterparty ? <KeyValueRow label={t('entry.counterparty')}>{entry.counterparty.name}</KeyValueRow> : null}
          {entry.place ? <KeyValueRow label={t('entry.place')}>{placeLabel(entry.place, locale)}</KeyValueRow> : null}
          {entry.tags.length > 0 ? (
            <KeyValueRow label={t('entry.tags')}>
              <span className="flex flex-wrap gap-1">
                {entry.tags.map((tag) => (
                  <Badge key={tag} variant="soft">
                    #{tag}
                  </Badge>
                ))}
              </span>
            </KeyValueRow>
          ) : null}
          {entry.source ? <KeyValueRow label={t('entry.source')}>{entry.source}</KeyValueRow> : null}
        </KeyValue>

        <section className="flex flex-col gap-2">
          <h3 className="caption">{t('entry.lines')}</h3>
          <Table>
            <TableBody>
              {entry.lines.map((line) => (
                <TableRow key={line.id}>
                  <TableCell>
                    <div className={line.side === 'account' ? 'font-medium text-text' : 'text-text'}>
                      {line.side === 'account' && line.account_id
                        ? accountName(line.account_id)
                        : line.side === 'category' && line.category_id
                          ? (labels.get(line.category_id) ?? '')
                          : t('entry.conversion')}
                    </div>
                    {line.note ? <div className="text-xs text-dim">{line.note}</div> : null}
                  </TableCell>
                  <TableCell numeric>
                    <Money amount={line.amount} currency={line.currency} signDisplay="exceptZero" />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </section>
      </DrawerBody>
      <DrawerActions
        start={
          <ConfirmDialog>
            <ConfirmDialogTrigger render={<Button variant="danger" />}>{t('entry.delete')}</ConfirmDialogTrigger>
            <ConfirmDialogPopup>
              <ConfirmDialogHeader>
                <ConfirmDialogTitle>{t('entry.deleteTitle')}</ConfirmDialogTitle>
                <ConfirmDialogDescription>{t('entry.deleteBody')}</ConfirmDialogDescription>
              </ConfirmDialogHeader>
              <ConfirmDialogActions>
                <Button render={<ConfirmDialogClose />}>{t('common.cancel')}</Button>
                <Button variant="danger" onClick={() => remove.mutate()} disabled={remove.isPending}>
                  {t('entry.delete')}
                </Button>
              </ConfirmDialogActions>
            </ConfirmDialogPopup>
          </ConfirmDialog>
        }
      >
        <Button onClick={onEdit}>{t('entry.edit')}</Button>
        {entry.status === 'pending' ? (
          <Button variant="primary" onClick={onPost}>
            {t('entry.post')}
          </Button>
        ) : null}
      </DrawerActions>
    </>
  )
}

/** Changing what an entry says about itself. Its lines are not edited here:
 * an amount that was wrong is an entry deleted and recorded again, which
 * keeps every balance it ever produced explainable. */
function EditEntry({ entry, onDone }: { entry: Entry; onDone: () => void }) {
  const { t } = useTranslation()
  const changed = useBooksChanged()
  const toasts = useToastManager()
  const [description, setDescription] = useState(entry.description)
  const [date, setDate] = useState(entry.occurred_on)
  const [counterparty, setCounterparty] = useState<Picked | null>(entry.counterparty ? { id: entry.counterparty.id } : null)
  const [tags, setTags] = useState(entry.tags)
  const [place, setPlace] = useState<PlaceBody | null>(entry.place ? { country: entry.place.country, city: entry.place.city ?? null } : null)
  const [problem, setProblem] = useState<string | null>(null)

  const save = useMutation({
    mutationFn: async () => {
      const firstCategory = entry.lines.find((line) => line.side === 'category')?.category_id ?? undefined
      return api.updateEntry(entry.id, {
        description: description.trim(),
        occurred_on: date,
        counterparty_id: await resolver().counterparty(counterparty, firstCategory),
        tags,
        place: place && place.country ? place : null,
      })
    },
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('entry.saved') })
      onDone()
    },
    onError: (error: Error) => {
      void changed()
      setProblem(error.message)
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    if (tags.some((tag) => !isTag(tag))) return
    save.mutate()
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DrawerBody className="flex flex-col gap-4">
        <Field label={t('entry.description')}>
          <Input value={description} onChange={(event) => setDescription(event.target.value)} />
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
        <CounterpartyField value={counterparty} onValueChange={setCounterparty} />
        <TagsField value={tags} onValueChange={setTags} />
        <PlaceField value={place} onValueChange={setPlace} />
        {problem ? (
          <Alert tone="bad" role="alert">
            {problem}
          </Alert>
        ) : null}
      </DrawerBody>
      <DrawerActions>
        <Button onClick={onDone}>{t('common.cancel')}</Button>
        <Button type="submit" variant="primary" disabled={save.isPending}>
          {t('common.save')}
        </Button>
      </DrawerActions>
    </form>
  )
}

/**
 * Posting a held payment: the day the bank posted it, and - when that is not
 * what was held - what the account actually moved by. A hotel's
 * pre-authorisation, a fuel pump's, a charge in another currency at the bank's
 * rate: the ledger recomputes the conversion and its fee from it.
 */
function PostEntry({ entry, onDone }: { entry: Entry; onDone: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const changed = useBooksChanged()
  const toasts = useToastManager()
  const [on, setOn] = useState(today())
  const [amount, setAmount] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const held = entry.lines.find((line) => line.side === 'account')
  // A split cannot post for another amount - the ledger would have to guess
  // which of its parts changed - so the field is not offered for one.
  const split = entry.lines.filter((line) => line.side === 'category').length > 1

  const post = useMutation({
    mutationFn: (actual: string | undefined) => api.clearEntry(entry.id, on, actual),
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('entry.postedToast') })
      onDone()
    },
    onError: (error: Error) => setProblem(error.message),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    if (amount.trim() === '') {
      post.mutate(undefined)
      return
    }
    const actual = parseAmount(amount, locale)
    if (actual === null || sign(actual) <= 0) {
      setProblem(t('form.positiveAmounts'))
      return
    }
    post.mutate(actual)
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DrawerBody className="flex flex-col gap-4">
        <Field label={t('entry.postedOnLabel')} required>
          <DatePicker
            value={on}
            onValueChange={setOn}
            min={entry.occurred_on}
            placeholder={t('entry.date')}
            previousMonthLabel={t('calendar.previous')}
            nextMonthLabel={t('calendar.next')}
          />
        </Field>
        {split ? null : (
          <Field
            label={t('entry.actualAmount')}
            help={
              held ? (
                <>
                  {t('entry.actualAmountHelp')} <Money amount={held.amount.replace(/^-/, '')} currency={held.currency} />
                </>
              ) : undefined
            }
          >
            <AmountInput value={amount} onChange={(event) => setAmount(event.target.value)} />
          </Field>
        )}
        {problem ? (
          <Alert tone="bad" role="alert">
            {problem}
          </Alert>
        ) : null}
      </DrawerBody>
      <DrawerActions>
        <Button onClick={onDone}>{t('common.cancel')}</Button>
        <Button type="submit" variant="primary" disabled={post.isPending}>
          {t('entry.post')}
        </Button>
      </DrawerActions>
    </form>
  )
}
