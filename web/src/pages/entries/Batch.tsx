import { useMemo, useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { ArrowRightLeft, FolderInput, Tags, X } from 'lucide-react'
import { Picker, type Picked } from '@/components/Picker'
import { ActionBar, ActionBarButton, ActionBarSpacer } from '@/components/ui/action-bar'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogDescription, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { categoryLabels, openAccounts, useAccounts, useBooksChanged, useCategories } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import type { BatchAction, Entry } from '@/lib/types'
import { AccountSelect, isTag, TagsField } from '@/pages/entries/fields'

/*
 * What is done to many entries at once: a category given to all of them,
 * their money moved to another account, labels put on and taken off. One
 * request for the lot, which the ledger does all of or none of; each entry's
 * history says who changed it.
 */

/** What a batch does. */
export type BatchKind = 'categorize' | 'move' | 'tag'

/** The account the entries' money can be moved from: the one the list is
 * narrowed to, or the one every entry moves - each through one account, the
 * same one. `undefined` when they do not agree, and a move would not know
 * which side to move. */
export function movableFrom(entries: readonly Entry[], listAccount: string | undefined): string | undefined {
  if (listAccount) return listAccount
  let from: string | undefined
  for (const entry of entries) {
    const accounts = [...new Set(entry.lines.filter((line) => line.side === 'account').map((line) => line.account_id))]
    if (accounts.length !== 1 || !accounts[0]) return undefined
    if (from !== undefined && from !== accounts[0]) return undefined
    from = accounts[0]
  }
  return from
}

interface BatchBarProps {
  /** How many are chosen. */
  count: number
  /** Whether the money of the chosen can be moved: they agree on an account. */
  movable: boolean
  onAct: (kind: BatchKind) => void
  onClear: () => void
}

/** The strip under the list while entries are chosen: what can be done to
 * them, and the way out. */
export function BatchBar({ count, movable, onAct, onClear }: BatchBarProps) {
  const { t } = useTranslation()
  return (
    <ActionBar
      position="bottom"
      aria-label={t('batch.label')}
      className="mt-3 flex-wrap gap-y-2 rounded-b-md px-1"
    >
      <span className="text-sm font-medium text-text" aria-live="polite">
        {t('batch.chosen', { count })}
      </span>
      <ActionBarSpacer />
      <ActionBarButton render={<Button size="sm" />} onClick={() => onAct('categorize')}>
        <FolderInput aria-hidden />
        {t('batch.categorize')}
      </ActionBarButton>
      <ActionBarButton
        render={<Button size="sm" />}
        onClick={() => onAct('move')}
        disabled={!movable}
        title={movable ? undefined : t('batch.notMovable')}
      >
        <ArrowRightLeft aria-hidden />
        {t('batch.move')}
      </ActionBarButton>
      <ActionBarButton render={<Button size="sm" />} onClick={() => onAct('tag')}>
        <Tags aria-hidden />
        {t('batch.tag')}
      </ActionBarButton>
      <ActionBarButton render={<Button variant="icon" size="icon-sm" />} onClick={onClear} aria-label={t('batch.clear')}>
        <X aria-hidden />
      </ActionBarButton>
    </ActionBar>
  )
}

interface BatchDialogProps {
  kind: BatchKind | null
  /** The entries it is done to. */
  entries: readonly Entry[]
  /** Where their money can be moved from, for a move. */
  from: string | undefined
  onOpenChange: (open: boolean) => void
  /** After it was done. */
  onDone: () => void
}

/** The question a batch asks before it is done: which category, which
 * account, which labels. */
export function BatchDialog({ kind, entries, from, onOpenChange, onDone }: BatchDialogProps) {
  return (
    <Dialog open={kind !== null} onOpenChange={onOpenChange}>
      {kind === null ? null : (
        <DialogPopup>
          <BatchForm kind={kind} entries={entries} from={from} onDone={onDone} />
        </DialogPopup>
      )}
    </Dialog>
  )
}

function BatchForm({ kind, entries, from, onDone }: { kind: BatchKind; entries: readonly Entry[]; from: string | undefined; onDone: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const categories = useCategories()
  const accounts = useAccounts()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const [category, setCategory] = useState<Picked | null>(null)
  const [to, setTo] = useState<string | undefined>()
  const [add, setAdd] = useState<string[]>([])
  const [remove, setRemove] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)

  const source = accounts.data?.find((account) => account.id === from)
  // Only an open account in the same currency: moving money into another
  // currency would need a rate nobody stated.
  const targets = openAccounts(accounts.data).filter((account) => account.id !== from && account.currency === source?.currency)
  const categoryOptions = useMemo(
    () =>
      (categories.data ?? [])
        .map((each) => ({ value: each.id, label: labels.get(each.id) ?? each.name }))
        .sort((a, b) => a.label.localeCompare(b.label, locale)),
    [categories.data, labels, locale],
  )

  const batch = useMutation({
    mutationFn: (action: BatchAction) => api.batch(entries.map((entry) => entry.id), action),
    onSuccess: (done) => {
      void changed()
      toasts.add({
        type: 'success',
        title: t('batch.done', { count: done.changed.length }),
        description: done.unchanged.length > 0 ? t('batch.unchanged', { count: done.unchanged.length }) : undefined,
      })
      onDone()
    },
    onError: (failure: Error) => setError(problem(failure, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    if (kind === 'categorize') {
      if (category === null || !('id' in category)) return setError(t('form.incomplete'))
      batch.mutate({ categorize: category.id })
    } else if (kind === 'move') {
      if (!from || !to) return setError(t('form.incomplete'))
      batch.mutate({ move: { from, to } })
    } else {
      if ([...add, ...remove].some((tag) => !isTag(tag))) return
      if (add.length + remove.length === 0) return setError(t('form.incomplete'))
      batch.mutate({ tag: { add, remove } })
    }
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DialogHeader>
        <DialogTitle>{t(`batch.${kind}Title`, { count: entries.length })}</DialogTitle>
        <DialogDescription>{t(`batch.${kind}Body`, { account: source?.name ?? '' })}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-4">
        {kind === 'categorize' ? (
          <Field label={t('entry.category')} required>
            <Picker options={categoryOptions} value={category} onValueChange={setCategory} placeholder={t('entry.category')} emptyLabel={t('picker.empty')} />
          </Field>
        ) : kind === 'move' ? (
          <AccountSelect accounts={targets} value={to} onValueChange={setTo} label={t('batch.moveTo')} />
        ) : (
          <>
            <TagsField value={add} onValueChange={setAdd} label={t('batch.putOn')} />
            <TagsField value={remove} onValueChange={setRemove} label={t('batch.takeOff')} help={t('batch.takeOffHelp')} />
          </>
        )}
        {error ? (
          <Alert tone="bad" role="alert">
            {error}
          </Alert>
        ) : null}
      </DialogBody>
      <DialogActions>
        <Button type="button" render={<DialogClose />}>{t('common.cancel')}</Button>
        <Button type="submit" variant="primary" disabled={batch.isPending}>
          {t(`batch.${kind}`)}
        </Button>
      </DialogActions>
    </form>
  )
}
