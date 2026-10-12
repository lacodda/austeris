import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { useBooksChanged } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import type { Entry } from '@/lib/types'

interface MakeButtonProps {
  entry: Entry
  /** What the button says until the person says otherwise. */
  suggested: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Making a button out of an entry: what it says, and whether it keeps the
 * amount or asks for it each time.
 *
 * Everything else - the account, the category, who and which tags - is the
 * entry's, so there is nothing else to fill in: a person recognises a coffee
 * they bought more readily than they describe one in a form.
 */
export function MakeButton({ entry, suggested, open, onOpenChange }: MakeButtonProps) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const [name, setName] = useState(suggested)
  const [askAmount, setAskAmount] = useState(false)
  const [problemText, setProblemText] = useState<string | null>(null)

  const make = useMutation({
    mutationFn: () => api.createTemplate(entry.id, name.trim(), askAmount),
    onSuccess: (template) => {
      void changed()
      toasts.add({ type: 'success', title: t('template.made', { name: template.name }) })
      onOpenChange(false)
    },
    onError: (error: Error) => setProblemText(problem(error, t)),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblemText(null)
    if (name.trim() !== '') make.mutate()
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup>
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{t('template.makeTitle')}</DialogTitle>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-4">
            <Field label={t('template.name')} required>
              <Input value={name} onChange={(event) => setName(event.target.value)} autoComplete="off" />
            </Field>
            <Checkbox checked={askAmount} onCheckedChange={setAskAmount}>
              {t('template.askAmount')}
            </Checkbox>
            {problemText ? (
              <Alert tone="bad" role="alert">
                {problemText}
              </Alert>
            ) : null}
          </DialogBody>
          <DialogActions>
            <Button render={<DialogClose />}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" disabled={make.isPending || name.trim() === ''}>
              {t('template.make')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}
