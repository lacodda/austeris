import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { AmountInput } from '@/components/AmountInput'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { parseAmount } from '@/lib/decimal'
import { useBooksChanged } from '@/lib/ledger'
import { isIsoCurrency } from '@/lib/money'
import { ACCOUNT_KINDS, type AccountKind } from '@/lib/types'

/** A currency's name in a language, for the hint under the code: `PYG` -
 * Paraguayan Guarani. Nothing for a code that is not ISO 4217: a coin is
 * named by its code, and a guess would be worse than silence. */
function currencyName(code: string, locale: string): string | undefined {
  if (!isIsoCurrency(code)) return undefined
  try {
    return new Intl.DisplayNames([locale], { type: 'currency' }).of(code.toUpperCase())
  } catch {
    return undefined
  }
}

/** Opening an account: what it is called, what kind of place it is, its
 * currency, and what was in it before austeris knew about it. */
export function NewAccount() {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [kind, setKind] = useState<AccountKind>('bank')
  const [currency, setCurrency] = useState('')
  const [opening, setOpening] = useState('')
  const [problem, setProblem] = useState<string | null>(null)

  const create = useMutation({
    mutationFn: api.createAccount,
    onSuccess: (account) => {
      void changed()
      toasts.add({ type: 'success', title: t('accounts.created', { name: account.name }) })
      setOpen(false)
      setName('')
      setCurrency('')
      setOpening('')
    },
    onError: (error: Error) => setProblem(error.message),
  })

  const code = currency.trim().toUpperCase()
  const amount = opening.trim() === '' ? undefined : parseAmount(opening, locale)

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    if (amount === null) {
      setProblem(t('form.notAnAmount'))
      return
    }
    create.mutate({ name: name.trim(), kind, currency: code, opening_balance: amount })
  }

  const kinds = Object.fromEntries(ACCOUNT_KINDS.map((each) => [each, t(`accountKind.${each}`)]))

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Button variant="primary" onClick={() => setOpen(true)}>
        {t('accounts.new')}
      </Button>
      <DialogPopup>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{t('accounts.newTitle')}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <Field label={t('accounts.name')} required>
              <Input required value={name} onChange={(event) => setName(event.target.value)} autoFocus />
            </Field>
            <Field label={t('accounts.kind')}>
              <Select items={kinds} value={kind} onValueChange={(value) => setKind(value as AccountKind)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectPopup>
                  {ACCOUNT_KINDS.map((each) => (
                    <SelectItem key={each} value={each}>
                      {kinds[each]}
                    </SelectItem>
                  ))}
                </SelectPopup>
              </Select>
            </Field>
            <Field label={t('accounts.currency')} help={currencyName(code, locale) ?? t('accounts.currencyHelp')} required>
              <Input
                required
                value={currency}
                onChange={(event) => setCurrency(event.target.value.toUpperCase())}
                maxLength={10}
                autoCapitalize="characters"
                autoComplete="off"
                className="font-mono uppercase"
              />
            </Field>
            <Field label={t('accounts.opening')} help={t('accounts.openingHelp')}>
              <AmountInput value={opening} onChange={(event) => setOpening(event.target.value)} placeholder="0" />
            </Field>
            {problem ? (
              <Alert tone="bad" role="alert">
                {problem}
              </Alert>
            ) : null}
          </DialogBody>
          <DialogActions>
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={create.isPending}>
              {t('accounts.create')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}
