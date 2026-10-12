import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { api } from '@/lib/api'
import { formatDay, today } from '@/lib/dates'
import { categoryLabels, keys, useBooksChanged, useCategories } from '@/lib/ledger'
import { problem } from '@/lib/problem'
import { useRecordedToast, useSecondPress } from '@/lib/recording'
import type { IsoDate, Preview, QuickLine as Line } from '@/lib/types'
import { cn } from '@/lib/utils'
import { describe } from '@/pages/entries/ExchangeForm'

interface QuickLineProps {
  /** The account a line that names none is recorded against: the one a
   * screen has chosen. A line saying `from cash` is still taken at its word. */
  accountId?: string
  /** The day it is recorded on. Today in this browser when not given - the
   * person's day, not the server's, which is another one after nine in the
   * evening in Asunción. */
  day?: IsoDate
  className?: string
}

/** How long typing has to pause before the line is worked out. */
const SETTLE_MS = 300

/**
 * An entry in one typed line: `45000 food lunch @casarica #trip`.
 *
 * The same line the command line takes, read by the same parser on the server
 * - one grammar, so a habit learnt in a terminal works here unchanged. While
 * it is typed, the server works it out without recording it, and what it
 * would record is shown under the field: the amount and its sign, the
 * category, the account, who and which tags are new. On a phone the list of
 * entries is not on the screen, and a line that meant something else is seen
 * before it is recorded rather than found in a report.
 */
export function QuickLine({ accountId, day, className }: QuickLineProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const changed = useBooksChanged()
  const recorded = useRecordedToast()
  const secondPress = useSecondPress()
  const [text, setText] = useState('')
  const [problemText, setProblemText] = useState<string | null>(null)
  const settled = useSettled(text.trim(), SETTLE_MS)
  const on = day ?? today()

  const line = (said: string, allowDuplicate = false): Line => ({
    text: said,
    occurred_on: on,
    account_id: accountId ?? null,
    place: null,
    ...(allowDuplicate ? { allow_duplicate: true } : {}),
  })

  const preview = useQuery({
    queryKey: [...keys.all, 'preview', settled, accountId ?? null, on],
    queryFn: ({ signal }) => api.preview(line(settled), signal),
    enabled: settled !== '',
    retry: false,
    placeholderData: keepPreviousData,
  })

  const record = useMutation({
    mutationFn: (allowDuplicate: boolean) => api.quick(line(text.trim(), allowDuplicate)),
    onSuccess: (done) => {
      void changed()
      setText('')
      const notes: string[] = []
      if (done.conversion) notes.push(describe(done.conversion, locale, t))
      if (done.new_counterparty) notes.push(t('quick.newCounterparty', { name: done.new_counterparty.name }))
      if (done.new_tags?.length) notes.push(t('quick.newTags', { tags: done.new_tags.map((tag) => `#${tag}`).join(' ') }))
      recorded(
        done.entry,
        t('quick.recorded', { text: done.entry.description || done.entry.counterparty?.name || '' }),
        notes.length > 0 ? notes.join(' · ') : undefined,
      )
    },
    onError: (error: Error) => {
      // Said to be one press, the line is recorded already and goes.
      if (!secondPress.ask(error, () => record.mutate(true), () => setText(''))) setProblemText(problem(error, t))
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblemText(null)
    if (text.trim() !== '') record.mutate(false)
  }

  const showPreview = text.trim() !== '' && settled !== '' && problemText === null
  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <form onSubmit={submit} className="flex items-start gap-2">
        <Field label={t('quick.label')} labelHidden help={t('quick.help')} error={problemText ?? undefined} className="min-w-0 flex-1">
          <Input
            value={text}
            onChange={(event) => {
              setText(event.target.value)
              if (problemText) setProblemText(null)
            }}
            placeholder={t('quick.placeholder')}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            enterKeyHint="done"
            className="font-mono"
          />
        </Field>
        <Button type="submit" variant="soft" disabled={record.isPending || text.trim() === ''}>
          {t('quick.record')}
        </Button>
      </form>
      {showPreview ? (
        <PreviewLine
          preview={preview.data}
          error={preview.error}
          // A preview of what was typed a moment ago is shown dimmed until
          // the line it belongs to is worked out.
          stale={preview.isPlaceholderData || settled !== text.trim()}
        />
      ) : null}
      {secondPress.dialog}
    </div>
  )
}

/** A value that follows another once it has stopped changing for `ms`. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return settled
}

interface PreviewLineProps {
  preview: Preview | undefined
  error: Error | null
  stale: boolean
}

/** What the line would record, in the words a person checks it by. */
function PreviewLine({ preview, error, stale }: PreviewLineProps) {
  const { t } = useTranslation()
  const locale = useLocale()
  const categories = useCategories()
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])

  if (error && !preview) {
    // Not a failure yet: a line half typed says what it still needs.
    return (
      <p role="status" className="text-xs text-dim">
        {problem(error, t)}
      </p>
    )
  }
  if (!preview) return null

  const details: string[] = []
  // The day is said when it is not today, so "yesterday" chosen on the
  // screen is seen in what is about to be recorded.
  if (preview.occurred_on !== today()) details.push(formatDay(preview.occurred_on, locale))
  if (preview.conversion) details.push(describe(preview.conversion, locale, t))
  return (
    <div
      role="status"
      aria-label={t('quick.previewLabel')}
      className={cn('flex flex-col gap-1 rounded-md bg-soft px-3 py-2 text-sm transition-opacity', stale && 'opacity-60')}
    >
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <Money amount={preview.moves} currency={preview.account.currency} signDisplay="exceptZero" className="font-semibold text-text" />
        <span className="text-text">{labels.get(preview.category.id) ?? preview.category.name}</span>
        <span className="text-dim">
          {preview.flow === 'income' ? t('quick.into', { account: preview.account.name }) : t('quick.from', { account: preview.account.name })}
        </span>
        {preview.pending ? <Badge variant="warn">{t('entry.heldBadge')}</Badge> : null}
      </div>
      {preview.description || preview.counterparty || preview.tags.length > 0 || details.length > 0 ? (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-dim">
          {preview.description ? <span>{preview.description}</span> : null}
          {preview.counterparty ? (
            <span>
              @{preview.counterparty.name}
              {preview.counterparty.id ? null : (
                <Badge variant="accent" className="ml-1">
                  {t('quick.new')}
                </Badge>
              )}
            </span>
          ) : null}
          {preview.tags.map((tag) => (
            <span key={tag}>
              #{tag}
              {preview.new_tags.includes(tag) ? (
                <Badge variant="accent" className="ml-1">
                  {t('quick.new')}
                </Badge>
              ) : null}
            </span>
          ))}
          {details.map((detail) => (
            <span key={detail}>{detail}</span>
          ))}
        </div>
      ) : null}
    </div>
  )
}
