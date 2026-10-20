import { useMemo, useRef, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { FileUp } from 'lucide-react'
import { Money } from '@/components/Money'
import { Picker, type Picked } from '@/components/Picker'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Field } from '@/components/ui/field'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { formatDay, formatMoment } from '@/lib/dates'
import { issuedOn, shopName, useDocuments, useDocumentsChanged } from '@/lib/imports'
import { categoryLabels, openAccounts, useAccounts, useCategories } from '@/lib/ledger'
import { problem, reasonText } from '@/lib/problem'
import type { ImportDocument } from '@/lib/types'
import { AccountSelect } from '@/pages/entries/fields'
import { resolver } from '@/pages/entries/resolve'

/** What the chooser offers: a receipt's XML, as the shop e-mails it. */
const ACCEPT = 'application/xml,text/xml,.xml'

/**
 * Receipts read into the books.
 *
 * A SIFEN receipt's XML - chosen here, imported from a terminal, or dropped in
 * the installation's folder - is one entry with a line per item. What a
 * receipt does not say is asked here, once: the shop then knows what it is
 * usually for, and the next receipt from it goes straight in.
 */
export function Import() {
  const { t } = useTranslation()
  const documents = useDocuments()
  const accounts = useAccounts()
  const usable = useMemo(() => openAccounts(accounts.data), [accounts.data])
  const toasts = useToastManager()
  const changed = useDocumentsChanged()
  const chooser = useRef<HTMLInputElement>(null)
  const ANY = ''
  const [paidFrom, setPaidFrom] = useState(ANY)

  const all = documents.data ?? []
  const waiting = all.filter((document) => document.state === 'waiting')
  const recent = all.filter((document) => document.state !== 'waiting')

  const upload = useMutation({
    // One at a time: each is answered for itself, and a refusal names its file.
    mutationFn: async (files: File[]) => {
      const tally = { booked: 0, waiting: 0, refused: 0 }
      for (const file of files) {
        const { document } = await api.upload(file, paidFrom || undefined)
        tally[document.state] += 1
      }
      return tally
    },
    onSuccess: (tally) => toasts.add({ type: tally.refused > 0 ? 'warning' : 'success', title: t('imports.uploaded', tally) }),
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
    onSettled: () => void changed(),
  })

  const paidFromItems: Record<string, string> = { [ANY]: t('imports.asReceiptsSay') }
  for (const account of usable) paidFromItems[account.id] = `${account.name} · ${account.currency}`

  return (
    <Container>
      <PageHeader title={t('imports.title')} description={t('imports.description')} />
      <Panel className="mb-6 flex flex-col gap-3 p-4 sm:flex-row sm:items-end">
        <Field label={t('imports.paidFrom')} help={t('imports.paidFromHelp')} className="min-w-0 flex-1">
          <Select items={paidFromItems} value={paidFrom} onValueChange={(id) => setPaidFrom(id as string)}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectPopup>
              {Object.entries(paidFromItems).map(([id, label]) => (
                <SelectItem key={id} value={id}>
                  {label}
                </SelectItem>
              ))}
            </SelectPopup>
          </Select>
        </Field>
        <Button variant="primary" onClick={() => chooser.current?.click()} disabled={upload.isPending} className="sm:mb-6">
          <FileUp aria-hidden />
          {upload.isPending ? t('imports.adding') : t('imports.add')}
        </Button>
        <input
          ref={chooser}
          type="file"
          accept={ACCEPT}
          multiple
          hidden
          data-testid="import-files"
          onChange={(event) => {
            const files = Array.from(event.target.files ?? [])
            // Cleared, so the same file chosen again is a change the input
            // reports.
            event.target.value = ''
            if (files.length > 0) upload.mutate(files)
          }}
        />
      </Panel>
      <QueryState
        pending={documents.isPending}
        error={documents.error}
        empty={all.length === 0}
        emptyState={<EmptyState title={t('imports.emptyTitle')} body={t('imports.emptyBody')} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void documents.refetch()}
        retryLabel={t('common.retry')}
      >
        <div className="flex flex-col gap-6">
          {waiting.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t('imports.waiting')}>
              <h2 className="caption">{t('imports.waiting')}</h2>
              <p className="text-sm text-dim">{t('imports.waitingHelp')}</p>
              <Panel>
                <ul className="flex flex-col divide-y divide-line">
                  {waiting.map((document) => (
                    <Waiting key={document.id} document={document} />
                  ))}
                </ul>
              </Panel>
            </section>
          ) : null}
          {recent.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t('imports.recent')}>
              <h2 className="caption">{t('imports.recent')}</h2>
              <Panel>
                <ul className="flex flex-col divide-y divide-line">
                  {recent.map((document) => (
                    <Done key={document.id} document={document} />
                  ))}
                </ul>
              </Panel>
            </section>
          ) : null}
          <p className="text-xs text-faint">{t('imports.folder')}</p>
        </div>
      </QueryState>
    </Container>
  )
}

/** What a document is called: its shop as the books name it, else its file. */
function nameOf(document: ImportDocument): string {
  if (document.shop) return document.shop
  return document.receipt ? shopName(document.receipt) : document.name
}

/** The receipt in one line: the day, how many items, and what it is when it
 * is not a plain purchase. */
function Summary({ document }: { document: ImportDocument }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const receipt = document.receipt
  const parts = receipt
    ? [
        formatDay(issuedOn(receipt), locale),
        t('imports.items', { count: receipt.items.length }),
        receipt.kind === 'credit_note' ? t('imports.creditNote') : null,
        receipt.on_credit ? t('imports.onCredit') : null,
      ]
    : [formatMoment(document.received_at, locale)]
  if (document.origin === 'folder') parts.push(t('imports.fromFolder'))
  return <span className="text-xs text-dim">{parts.filter(Boolean).join(' · ')}</span>
}

const ROW = 'flex flex-col gap-3 px-3 py-3 text-sm'

/** A receipt waiting for what it does not say: the account, the category, or
 * the books to answer. */
function Waiting({ document }: { document: ImportDocument }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const toasts = useToastManager()
  const changed = useDocumentsChanged()
  const accounts = useAccounts()
  const categories = useCategories()
  const usable = useMemo(() => openAccounts(accounts.data), [accounts.data])
  const labels = useMemo(() => categoryLabels(categories.data), [categories.data])
  const options = useMemo(
    () =>
      (categories.data ?? [])
        .filter((category) => category.flow === 'expense')
        .map((category) => ({ value: category.id, label: labels.get(category.id) ?? category.name }))
        .sort((a, b) => a.label.localeCompare(b.label, locale)),
    [categories.data, labels, locale],
  )
  const [account, setAccount] = useState<string | undefined>(document.account_id ?? undefined)
  const [category, setCategory] = useState<Picked | null>(document.category_id ? { id: document.category_id } : null)
  const name = nameOf(document)
  const needsAccount = document.needs.includes('account')
  const needsCategory = document.needs.includes('category')

  const book = useMutation({
    mutationFn: async () => {
      const categoryId = category ? await resolver().category(category, 'expense') : undefined
      return api.bookDocument(document.id, account, categoryId)
    },
    onSuccess: (after) =>
      toasts.add(
        after.state === 'booked'
          ? { type: 'success', title: t('imports.booked', { name }) }
          : { type: 'warning', title: t('imports.stillWaiting', { name }), description: after.reason ? reasonText(after.reason, t) : undefined },
      ),
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
    onSettled: () => void changed(),
  })
  const dismiss = useDismiss(document)

  return (
    <li className={ROW}>
      <div className="flex items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-medium text-text">{name}</span>
          <Summary document={document} />
        </div>
        {document.receipt ? <Money amount={document.receipt.total} currency={document.receipt.currency} /> : null}
      </div>
      {document.reason ? <p className="text-sm text-warn">{reasonText(document.reason, t)}</p> : null}
      <div className="grid gap-3 sm:grid-cols-2">
        {needsAccount || document.account_id || document.reason ? (
          <AccountSelect
            accounts={usable}
            value={account}
            onValueChange={setAccount}
            label={document.receipt?.kind === 'credit_note' ? t('imports.needsAccountBack') : t('imports.needsAccount')}
          />
        ) : null}
        {needsCategory || document.category_id ? (
          <Field label={t('imports.needsCategory')} required={needsCategory}>
            <Picker
              options={options}
              value={category}
              onValueChange={setCategory}
              placeholder={t('imports.category')}
              emptyLabel={t('picker.empty')}
              createLabel={(typed) => t('picker.createExpense', { name: typed })}
            />
          </Field>
        ) : null}
      </div>
      <div className="flex justify-end gap-2">
        <Button size="sm" onClick={() => dismiss.mutate()} disabled={dismiss.isPending} aria-label={t('imports.dismissNamed', { name })}>
          {t('imports.dismiss')}
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={() => book.mutate()}
          disabled={book.isPending || (needsAccount && !account) || (needsCategory && !category)}
          aria-label={t('imports.bookNamed', { name })}
        >
          {t('imports.book')}
        </Button>
      </div>
    </li>
  )
}

/** A receipt in the books, or a file that could not be read into them. */
function Done({ document }: { document: ImportDocument }) {
  const { t } = useTranslation()
  const name = nameOf(document)
  const dismiss = useDismiss(document)
  const booked = document.state === 'booked'
  return (
    <li className="flex flex-col gap-2 px-3 py-2.5 text-sm sm:flex-row sm:items-center sm:gap-3">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-text">{name}</span>
          <Badge variant={booked ? 'good' : 'bad'}>{booked ? t('imports.inBooks') : t('imports.notRead')}</Badge>
        </span>
        <Summary document={document} />
        {!booked && document.reason ? <span className="text-xs text-dim">{reasonText(document.reason, t)}</span> : null}
      </div>
      <span className="flex shrink-0 items-center justify-end gap-3">
        {document.receipt ? <Money amount={document.receipt.total} currency={document.receipt.currency} /> : null}
        <Button size="sm" variant="ghost" onClick={() => dismiss.mutate()} disabled={dismiss.isPending} aria-label={t('imports.dismissNamed', { name })}>
          {t('imports.dismiss')}
        </Button>
      </span>
    </li>
  )
}

/** Dismissing leaves the books as they are: an entry a receipt became stays. */
function useDismiss(document: ImportDocument) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const changed = useDocumentsChanged()
  return useMutation({
    mutationFn: () => api.dismissDocument(document.id),
    onSuccess: () => toasts.add({ type: 'success', title: t('imports.dismissed') }),
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
    onSettled: () => void changed(),
  })
}
