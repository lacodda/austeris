import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  ConfirmDialog,
  ConfirmDialogActions,
  ConfirmDialogClose,
  ConfirmDialogDescription,
  ConfirmDialogHeader,
  ConfirmDialogPopup,
  ConfirmDialogTitle,
} from '@/components/ui/confirm-dialog'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { useBooksChanged } from '@/lib/ledger'
import { isSecondPress, problem } from '@/lib/problem'
import type { Entry } from '@/lib/types'

/*
 * What every way of recording shares: the question after a second press, and
 * the way back after a press that was a mistake.
 *
 * On a phone a button is pressed twice by accident, and pressed again when
 * nothing seemed to happen. The server refuses an entry the same as one
 * recorded in the last minute; the person is asked whether this is a second
 * one, in a dialog - a question in a toast is a question the reader misses by
 * looking away. And whatever one press records, one press takes back: the
 * toast that says it was recorded carries Undo.
 */

/** How long a toast that can take an entry back stays up. */
const UNDO_MS = 10_000

interface Asking {
  seconds: number
  again: () => void
  kept?: () => void
}

/**
 * The second-press question: `ask(error, again, kept)` opens it when the
 * server refused a second press - and answers whether it did, so the caller
 * shows any other error itself. `again` sends the same entry, saying it is a
 * second one; `kept` is told when it was one after all, and the entry already
 * recorded is the one that stands. `dialog` goes anywhere in the screen.
 */
export function useSecondPress() {
  const { t } = useTranslation()
  const [asking, setAsking] = useState<Asking | null>(null)

  const ask = useCallback((error: unknown, again: () => void, kept?: () => void): boolean => {
    if (!isSecondPress(error)) return false
    setAsking({ seconds: Number(error.params.seconds ?? 0), again, kept })
    return true
  }, [])

  const keep = () => {
    const kept = asking?.kept
    setAsking(null)
    kept?.()
  }

  const dialog = (
    <ConfirmDialog open={asking !== null} onOpenChange={(open) => (open ? undefined : keep())}>
      <ConfirmDialogPopup>
        <ConfirmDialogHeader>
          <ConfirmDialogTitle>{t('secondPress.title')}</ConfirmDialogTitle>
          <ConfirmDialogDescription>
            {t('errors.entry.possible_duplicate', { seconds: asking?.seconds ?? 0 })} {t('secondPress.body')}
          </ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <Button render={<ConfirmDialogClose />}>{t('secondPress.keep')}</Button>
          <Button
            variant="primary"
            onClick={() => {
              const again = asking?.again
              setAsking(null)
              again?.()
            }}
          >
            {t('secondPress.again')}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )

  return { ask, dialog }
}

/**
 * Says an entry was recorded, with Undo on the toast: one press records, one
 * press takes it back, and the books are read again either way.
 */
export function useRecordedToast() {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useBooksChanged()

  return useCallback(
    (entry: Entry, title: string, description?: string) => {
      let id = ''
      const undo = () => {
        // Gone with the press that took it back: a second Undo would be
        // asking to delete what is not there any more.
        toasts.close(id)
        api
          .deleteEntry(entry.id)
          .then(() => {
            void changed()
            toasts.add({ title: t('record.undone') })
          })
          .catch((error: Error) => toasts.add({ type: 'error', title: problem(error, t) }))
      }
      id = toasts.add({
        type: 'success',
        title,
        description,
        // Longer than a plain note: the way back has to outlast reading the
        // toast and reaching for it with a thumb.
        timeout: UNDO_MS,
        actionProps: { children: t('record.undo'), onClick: undo },
      })
    },
    [changed, t, toasts],
  )
}
