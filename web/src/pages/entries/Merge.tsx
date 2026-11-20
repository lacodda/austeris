import { useMemo, useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Picker, type Picked } from '@/components/Picker'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogDescription, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { useBooksChanged, useCounterparties } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import type { Counterparty } from '@/lib/types'

interface MergeDialogProps {
  /** The counterparty merged away. */
  counterparty: Counterparty
  open: boolean
  onOpenChange: (open: boolean) => void
  /** After the merge, with the one it was merged into. */
  onMerged: (into: Counterparty) => void
}

/**
 * Merging a counterparty into another: a typo of a shop's name made a second
 * one, and its entries belong to the first. Everything moves in one go - the
 * entries, the buttons, the usual category when the other has none - and the
 * way it was typed finds the other from then on.
 */
export function MergeDialog({ counterparty, open, onOpenChange, onMerged }: MergeDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? (
        <DialogPopup>
          <MergeForm counterparty={counterparty} onMerged={onMerged} />
        </DialogPopup>
      ) : null}
    </Dialog>
  )
}

function MergeForm({ counterparty, onMerged }: { counterparty: Counterparty; onMerged: (into: Counterparty) => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const counterparties = useCounterparties()
  const [into, setInto] = useState<Picked | null>(null)
  const [error, setError] = useState<string | null>(null)
  const options = useMemo(
    () =>
      (counterparties.data ?? [])
        .filter((each) => each.id !== counterparty.id)
        .map((each) => ({ value: each.id, label: each.name }))
        .sort((a, b) => a.label.localeCompare(b.label, locale)),
    [counterparties.data, counterparty.id, locale],
  )

  const merge = useMutation({
    mutationFn: (target: string) => api.mergeCounterparty(counterparty.id, target),
    onSuccess: (merged) => {
      void changed()
      toasts.add({ type: 'success', title: t('merge.done', { from: counterparty.name, into: merged.counterparty.name, count: merged.entries }) })
      onMerged(merged.counterparty)
    },
    onError: (failure: Error) => setError(problem(failure, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    if (into === null || !('id' in into)) return setError(t('form.incomplete'))
    merge.mutate(into.id)
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DialogHeader>
        <DialogTitle>{t('merge.title', { name: counterparty.name })}</DialogTitle>
        <DialogDescription>{t('merge.body', { name: counterparty.name })}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-4">
        <Field label={t('merge.into')} required>
          <Picker options={options} value={into} onValueChange={setInto} placeholder={t('merge.into')} emptyLabel={t('picker.empty')} />
        </Field>
        {error ? (
          <Alert tone="bad" role="alert">
            {error}
          </Alert>
        ) : null}
      </DialogBody>
      <DialogActions>
        <Button type="button" render={<DialogClose />}>{t('common.cancel')}</Button>
        <Button type="submit" variant="primary" disabled={merge.isPending}>
          {t('merge.confirm')}
        </Button>
      </DialogActions>
    </form>
  )
}
