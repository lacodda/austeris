import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { useBooksChanged } from '@/lib/ledger'
import { describe } from '@/pages/entries/ExchangeForm'

/**
 * An entry in one typed line: `45000 food lunch @casarica #trip`.
 *
 * The same line the command line takes, read by the same parser on the server
 * - one grammar, so a habit learnt in a terminal works here unchanged. What
 * the server says back is said here too: the conversion when there was one,
 * and the counterparty or tags the line created, so a typo is seen the first
 * time rather than found later in a report.
 */
export function QuickLine() {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useBooksChanged()
  const [text, setText] = useState('')
  const [problem, setProblem] = useState<string | null>(null)

  const record = useMutation({
    mutationFn: () => api.quick(text.trim()),
    onSuccess: (recorded) => {
      void changed()
      setText('')
      const notes: string[] = []
      if (recorded.conversion) notes.push(describe(recorded.conversion, locale, t))
      if (recorded.new_counterparty) notes.push(t('quick.newCounterparty', { name: recorded.new_counterparty.name }))
      if (recorded.new_tags?.length) notes.push(t('quick.newTags', { tags: recorded.new_tags.map((tag) => `#${tag}`).join(' ') }))
      toasts.add({
        type: 'success',
        title: t('quick.recorded', { text: recorded.entry.description || recorded.entry.counterparty?.name || '' }),
        description: notes.length > 0 ? notes.join(' · ') : undefined,
      })
    },
    onError: (error: Error) => setProblem(error.message),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    setProblem(null)
    if (text.trim() !== '') record.mutate()
  }

  return (
    <form onSubmit={submit} className="flex items-start gap-2">
      <Field label={t('quick.label')} labelHidden help={t('quick.help')} error={problem ?? undefined} className="min-w-0 flex-1">
        <Input
          value={text}
          onChange={(event) => {
            setText(event.target.value)
            if (problem) setProblem(null)
          }}
          placeholder={t('quick.placeholder')}
          autoComplete="off"
          spellCheck={false}
          className="font-mono"
        />
      </Field>
      <Button type="submit" variant="soft" disabled={record.isPending || text.trim() === ''}>
        {t('quick.record')}
      </Button>
    </form>
  )
}
