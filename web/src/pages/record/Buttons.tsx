import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { AmountInput } from '@/components/AmountInput'
import { Money } from '@/components/Money'
import { Button } from '@/components/ui/button'
import { Drawer, DrawerBody, DrawerHeader, DrawerPopup, DrawerTitle } from '@/components/ui/drawer'
import { Field } from '@/components/ui/field'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { parseAmount, sign } from '@/lib/decimal'
import { useAccounts, useBooksChanged, useTemplates } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import { useRecordedToast, useSecondPress } from '@/lib/recording'
import type { Decimal, IsoDate, Template } from '@/lib/types'
import { describe } from '@/pages/entries/ExchangeForm'
import { ManageButtons } from '@/pages/record/ManageButtons'

interface Press {
  template: Template
  amount?: Decimal
  again: boolean
}

/**
 * The person's buttons: one press records the payment a button stands for,
 * on the day the screen is set to.
 *
 * A button that keeps its amount records at once, and its toast carries Undo;
 * one that asks for the amount opens a sheet with the phone's number keypad
 * and nothing else on it.
 */
export function Buttons({ day }: { day: IsoDate }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const templates = useTemplates()
  const accounts = useAccounts()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const recorded = useRecordedToast()
  const secondPress = useSecondPress()
  const [asking, setAsking] = useState<Template | null>(null)
  const [managing, setManaging] = useState(false)

  const press = useMutation({
    mutationFn: ({ template, amount, again }: Press) => api.recordTemplate(template.id, day, amount, again),
    onSuccess: (done, { template }) => {
      void changed()
      setAsking(null)
      recorded(done.entry, t('record.pressed', { name: template.name }), done.conversion ? describe(done.conversion, locale, t) : undefined)
    },
    onError: (error: Error, sent) => {
      if (!secondPress.ask(error, () => press.mutate({ ...sent, again: true }), () => setAsking(null))) {
        toasts.add({ type: 'error', title: problem(error, t) })
      }
    },
  })

  const list = templates.data ?? []
  const accountName = (id: string) => accounts.data?.find((account) => account.id === id)?.name ?? ''

  return (
    <section aria-labelledby="record-buttons" className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <h2 id="record-buttons" className="caption">
          {t('record.buttons')}
        </h2>
        {list.length > 0 ? (
          <Button size="sm" onClick={() => setManaging(true)}>
            {t('record.manage')}
          </Button>
        ) : null}
      </div>
      {templates.isPending ? null : list.length === 0 ? (
        <p className="text-sm text-dim">{t('record.noButtons')}</p>
      ) : (
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
          {list.map((template) => (
            <Button
              key={template.id}
              variant="soft"
              className="h-auto min-h-14 flex-col items-start justify-center gap-0.5 px-3 py-2 text-left"
              disabled={press.isPending}
              onClick={() => (template.amount ? press.mutate({ template, again: false }) : setAsking(template))}
            >
              <span className="w-full truncate font-medium text-text">{template.name}</span>
              <span className="w-full truncate text-xs font-normal text-dim">
                {template.amount ? <Money amount={template.amount} currency={template.currency} /> : t('record.asks')}
                {accountName(template.account_id) ? ` · ${accountName(template.account_id)}` : null}
              </span>
            </Button>
          ))}
        </div>
      )}

      <Drawer swipeDirection="down" open={asking !== null} onOpenChange={(open) => (open ? undefined : setAsking(null))}>
        {asking ? (
          <AmountSheet
            template={asking}
            pending={press.isPending}
            onRecord={(amount) => press.mutate({ template: asking, amount, again: false })}
          />
        ) : null}
      </Drawer>
      <Drawer swipeDirection="right" open={managing} onOpenChange={setManaging}>
        {managing ? <ManageButtons templates={list} /> : null}
      </Drawer>
      {secondPress.dialog}
    </section>
  )
}

interface AmountSheetProps {
  template: Template
  pending: boolean
  onRecord: (amount: Decimal) => void
}

/** How much, this time: one field, the keypad, one button. */
function AmountSheet({ template, pending, onRecord }: AmountSheetProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const [text, setText] = useState('')
  const [problemText, setProblemText] = useState<string | null>(null)

  function submit(event: FormEvent) {
    event.preventDefault()
    const amount = parseAmount(text, locale, template.currency)
    if (amount === null || sign(amount) <= 0) {
      setProblemText(t('form.notAnAmount'))
      return
    }
    onRecord(amount)
  }

  return (
    <DrawerPopup side="bottom" size="sm">
      <form onSubmit={submit}>
        <DrawerHeader>
          <DrawerTitle>{template.name}</DrawerTitle>
        </DrawerHeader>
        <DrawerBody className="flex flex-col gap-3">
          <Field label={t('record.howMuch', { currency: template.currency })} error={problemText ?? undefined}>
            <AmountInput
              value={text}
              onChange={(event) => {
                setText(event.target.value)
                if (problemText) setProblemText(null)
              }}
              // The sheet opened because this is the one thing to type.
              autoFocus
              enterKeyHint="done"
            />
          </Field>
          <Button type="submit" variant="primary" className="w-full" disabled={pending || text.trim() === ''}>
            {t('quick.record')}
          </Button>
        </DrawerBody>
      </form>
    </DrawerPopup>
  )
}
