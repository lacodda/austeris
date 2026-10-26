import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { ArrowDown, ArrowUp, Pencil, Trash2, X } from 'lucide-react'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import { Alert } from '@/components/ui/alert'
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
import { DrawerBody, DrawerClose, DrawerHeader, DrawerPopup, DrawerTitle } from '@/components/ui/drawer'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { decimalSeparator, parseAmount, sign } from '@/lib/decimal'
import { useBooksChanged } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import type { Template } from '@/lib/types'

/**
 * The buttons as a list to tidy: renamed, given an amount or told to ask for
 * one, moved up or down, deleted. What a button records is the entry it was
 * made from; to change that, a new one is made from another entry.
 */
export function ManageButtons({ templates }: { templates: Template[] }) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const [editing, setEditing] = useState<string | null>(null)

  const move = useMutation({
    mutationFn: (ids: string[]) => api.orderTemplates(ids),
    onSettled: () => void changed(),
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api.deleteTemplate(id),
    onSuccess: () => {
      void changed()
      toasts.add({ title: t('template.deleted') })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })

  /** The order with one button swapped with its neighbour. */
  function moved(index: number, by: -1 | 1): string[] {
    const ids = templates.map((template) => template.id)
    const other = index + by
    ;[ids[index], ids[other]] = [ids[other] as string, ids[index] as string]
    return ids
  }

  return (
    <DrawerPopup side="right" size="md">
      <DrawerHeader
        action={
          <Button variant="icon" size="icon-sm" render={<DrawerClose />} aria-label={t('common.close')}>
            <X aria-hidden />
          </Button>
        }
      >
        <DrawerTitle>{t('record.buttons')}</DrawerTitle>
      </DrawerHeader>
      <DrawerBody>
        <ul className="flex flex-col divide-y divide-line">
          {templates.map((template, index) => (
            <li key={template.id} className="py-2">
              {editing === template.id ? (
                <EditButton template={template} onDone={() => setEditing(null)} />
              ) : (
                <div className="flex items-center gap-1">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-text">{template.name}</div>
                    <div className="truncate text-xs text-dim">
                      {template.amount ? <Money amount={template.amount} currency={template.currency} /> : t('record.asks')}
                    </div>
                  </div>
                  <Button
                    variant="icon"
                    size="icon-md"
                    aria-label={t('template.up', { name: template.name })}
                    disabled={index === 0 || move.isPending}
                    onClick={() => move.mutate(moved(index, -1))}
                  >
                    <ArrowUp aria-hidden />
                  </Button>
                  <Button
                    variant="icon"
                    size="icon-md"
                    aria-label={t('template.down', { name: template.name })}
                    disabled={index === templates.length - 1 || move.isPending}
                    onClick={() => move.mutate(moved(index, 1))}
                  >
                    <ArrowDown aria-hidden />
                  </Button>
                  <Button variant="icon" size="icon-md" aria-label={t('template.edit', { name: template.name })} onClick={() => setEditing(template.id)}>
                    <Pencil aria-hidden />
                  </Button>
                  <ConfirmDialog>
                    <ConfirmDialogTrigger
                      render={<Button variant="icon" size="icon-md" aria-label={t('template.delete', { name: template.name })} />}
                    >
                      <Trash2 aria-hidden />
                    </ConfirmDialogTrigger>
                    <ConfirmDialogPopup>
                      <ConfirmDialogHeader>
                        <ConfirmDialogTitle>{t('template.deleteTitle', { name: template.name })}</ConfirmDialogTitle>
                        <ConfirmDialogDescription>{t('template.deleteBody')}</ConfirmDialogDescription>
                      </ConfirmDialogHeader>
                      <ConfirmDialogActions>
                        <Button render={<ConfirmDialogClose />}>{t('common.cancel')}</Button>
                        <Button variant="danger" onClick={() => remove.mutate(template.id)} disabled={remove.isPending}>
                          {t('entry.delete')}
                        </Button>
                      </ConfirmDialogActions>
                    </ConfirmDialogPopup>
                  </ConfirmDialog>
                </div>
              )}
            </li>
          ))}
        </ul>
      </DrawerBody>
    </DrawerPopup>
  )
}

/** A button's name and amount, changed in place. */
function EditButton({ template, onDone }: { template: Template; onDone: () => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const changed = useBooksChanged()
  const [name, setName] = useState(template.name)
  // Written the way this language writes a decimal, as it would be typed.
  const [amount, setAmount] = useState((template.amount ?? '').replace('.', decimalSeparator(locale)))
  const [problemText, setProblemText] = useState<string | null>(null)

  const save = useMutation({
    mutationFn: (kept: string | null) => api.updateTemplate(template.id, { name: name.trim(), amount: kept }),
    onSuccess: () => {
      void changed()
      onDone()
    },
    onError: (error: Error) => setProblemText(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblemText(null)
    if (amount.trim() === '') {
      save.mutate(null)
      return
    }
    const kept = parseAmount(amount, locale, template.currency)
    if (kept === null || sign(kept) <= 0) {
      setProblemText(t('form.notAnAmount'))
      return
    }
    save.mutate(kept)
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <Field label={t('template.name')} required>
        <Input value={name} onChange={(event) => setName(event.target.value)} autoComplete="off" />
      </Field>
      <Field label={t('template.amount', { currency: template.currency })} help={t('template.amountHelp')}>
        <AmountInput value={amount} onChange={(event) => setAmount(event.target.value)} />
      </Field>
      {problemText ? (
        <Alert tone="bad" role="alert">
          {problemText}
        </Alert>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>{t('common.cancel')}</Button>
        <Button type="submit" variant="primary" disabled={save.isPending || name.trim() === ''}>
          {t('common.save')}
        </Button>
      </div>
    </form>
  )
}
