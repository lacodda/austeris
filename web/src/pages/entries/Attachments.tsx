import { useRef } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { FileText, Paperclip, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useToastManager } from '@/components/ui/toast'
import { api, attachmentUrl } from '@/lib/api'
import { ACCEPT, formatSize, isPicture } from '@/lib/attachments'
import { useBooksChanged } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import type { Attachment, Entry } from '@/lib/types'

/** How long a toast that can put a file back stays up, as one that can take
 * an entry back does. */
const UNDO_MS = 10_000

/**
 * The files kept with an entry - the receipt's photo, the bank's PDF - and the
 * way to add one or take one off.
 *
 * On a phone the chooser offers the camera: the receipt is photographed where
 * it was paid. A file taken off goes to the trash, and the toast saying so
 * puts it back.
 */
export function Attachments({ entry }: { entry: Entry }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const chooser = useRef<HTMLInputElement>(null)

  const attach = useMutation({
    // One at a time: a refusal names the file it is about.
    mutationFn: async (files: File[]) => {
      for (const file of files) await api.attach(entry.id, file)
    },
    onSettled: () => void changed(),
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })

  const remove = useMutation({
    mutationFn: (file: Attachment) => api.removeAttachment(file.id),
    onSuccess: (_, file) => {
      void changed()
      let id = ''
      id = toasts.add({
        title: t('files.removed', { name: file.name }),
        timeout: UNDO_MS,
        actionProps: {
          children: t('record.undo'),
          onClick: () => {
            toasts.close(id)
            api
              .restoreAttachment(file.id)
              .then(() => void changed())
              .catch((error: Error) => toasts.add({ type: 'error', title: problem(error, t) }))
          },
        },
      })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })

  return (
    <section className="flex flex-col gap-2" aria-label={t('files.title')}>
      <div className="flex items-center justify-between gap-2">
        <h3 className="caption">{t('files.title')}</h3>
        <Button size="sm" onClick={() => chooser.current?.click()} disabled={attach.isPending}>
          <Paperclip aria-hidden />
          {attach.isPending ? t('files.attaching') : t('files.attach')}
        </Button>
        <input
          ref={chooser}
          type="file"
          accept={ACCEPT}
          multiple
          hidden
          data-testid="attach-files"
          onChange={(event) => {
            const files = Array.from(event.target.files ?? [])
            // Cleared, so choosing the same file again after a refusal is a
            // change the input reports.
            event.target.value = ''
            if (files.length > 0) attach.mutate(files)
          }}
        />
      </div>
      {entry.attachments.length === 0 ? (
        <p className="text-sm text-dim">{t('files.none')}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-line">
          {entry.attachments.map((file) => (
            <li key={file.id} className="flex items-center gap-3 py-1.5">
              <a
                href={attachmentUrl(file.id)}
                target="_blank"
                rel="noopener"
                className="flex min-w-0 flex-1 items-center gap-3 rounded-sm text-sm text-text hover:text-accent"
              >
                {isPicture(file.media_type) ? (
                  <img src={attachmentUrl(file.id)} alt="" className="size-10 shrink-0 rounded-sm border border-line object-cover" />
                ) : (
                  <span className="flex size-10 shrink-0 items-center justify-center rounded-sm border border-line text-dim">
                    <FileText aria-hidden className="size-5" />
                  </span>
                )}
                <span className="flex min-w-0 flex-col">
                  <span className="truncate">{file.name}</span>
                  <span className="text-xs text-dim">{formatSize(file.size, locale)}</span>
                </span>
              </a>
              <Button
                variant="icon"
                size="icon-md"
                aria-label={t('files.remove', { name: file.name })}
                disabled={remove.isPending}
                onClick={() => remove.mutate(file)}
              >
                <X aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
