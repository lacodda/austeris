import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { FileText } from 'lucide-react'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
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
import { EmptyState } from '@/components/ui/empty-state'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { useToastManager } from '@/components/ui/toast'
import { api, attachmentUrl } from '@/lib/api'
import { formatSize, isPicture } from '@/lib/attachments'
import { formatDay, formatMoment, formatMomentDay } from '@/lib/dates'
import { movements } from '@/lib/entries'
import { useModules } from '@/lib/imports'
import { useAccounts, useBooksChanged, useTrash } from '@/lib/ledger'
import { formatQuantity, useInstruments, useTradesChanged, useTradeTrash } from '@/lib/portfolio'
import { problem } from '@/lib/problem'
import type { TrashedAttachment, TrashedEntry, TrashedTrade } from '@/lib/types'
import { title } from '@/pages/entries/EntryDetail'

/**
 * What was deleted: entries, files taken off entries and - while the
 * installation runs the portfolio - trades, each until the day it is erased
 * for good, and the way back for each.
 */
export function Trash() {
  const { t } = useTranslation()
  const trash = useTrash()
  const modules = useModules()
  const portfolio = modules?.has('portfolio') ?? false
  const trades = useTradeTrash(portfolio)
  const toasts = useToastManager()
  const changed = useTradesChanged()
  const deletedTrades = portfolio ? (trades.data?.trades ?? []) : []
  const empty = (trash.data?.entries.length ?? 0) + (trash.data?.attachments.length ?? 0) + deletedTrades.length === 0
  // The server's word for how long, so the page cannot promise another.
  const days = trash.data?.keep_days

  const emptyAll = useMutation({
    mutationFn: () => Promise.all([api.emptyTrash(), portfolio ? api.emptyTradeTrash() : Promise.resolve()]),
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('trash.emptied') })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })

  return (
    <Container>
      <PageHeader
        title={t('trash.title')}
        description={days === undefined ? undefined : t('trash.description', { days })}
        actions={
          <Erase
            trigger={t('trash.emptyAll')}
            title={t('trash.emptyTitle')}
            body={t('trash.emptyBody')}
            disabled={empty || emptyAll.isPending}
            onErase={() => emptyAll.mutate()}
          />
        }
      />
      <QueryState
        pending={trash.isPending || (portfolio && trades.isPending)}
        error={trash.error ?? (portfolio ? trades.error : null)}
        empty={empty}
        emptyState={<EmptyState title={t('trash.emptyState')} body={t('trash.emptyStateBody', { days })} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => {
          void trash.refetch()
          if (portfolio) void trades.refetch()
        }}
        retryLabel={t('common.retry')}
      >
        <div className="flex flex-col gap-6">
          {trash.data && trash.data.entries.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t('trash.entries')}>
              <h2 className="caption">{t('trash.entries')}</h2>
              <Panel>
                <ul className="flex flex-col divide-y divide-line">
                  {trash.data.entries.map((item) => (
                    <DeletedEntry key={item.entry.id} item={item} />
                  ))}
                </ul>
              </Panel>
            </section>
          ) : null}
          {trash.data && trash.data.attachments.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t('trash.files')}>
              <h2 className="caption">{t('trash.files')}</h2>
              <Panel>
                <ul className="flex flex-col divide-y divide-line">
                  {trash.data.attachments.map((item) => (
                    <RemovedFile key={item.attachment.id} item={item} />
                  ))}
                </ul>
              </Panel>
            </section>
          ) : null}
          {deletedTrades.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t('trash.trades')}>
              <h2 className="caption">{t('trash.trades')}</h2>
              <Panel>
                <ul className="flex flex-col divide-y divide-line">
                  {deletedTrades.map((item) => (
                    <DeletedTrade key={item.trade.id} item={item} />
                  ))}
                </ul>
              </Panel>
            </section>
          ) : null}
        </div>
      </QueryState>
    </Container>
  )
}

/** A row of the trash: what it is above its two buttons on a phone, where the
 * words need the width, and beside them from `sm` up. */
const ROW = 'flex flex-col gap-2 px-3 py-2.5 text-sm sm:flex-row sm:items-center sm:gap-3'
const ACTIONS = 'flex shrink-0 justify-end gap-2'

function DeletedEntry({ item }: { item: TrashedEntry }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const name = title(item.entry, t('entry.untitled'))
  const restore = useRestore(() => api.restoreEntry(item.entry.id))
  const erase = useErase(() => api.eraseEntry(item.entry.id))
  const files = item.entry.attachments.length

  return (
    <li className={ROW}>
      <div className="flex min-w-0 flex-1 items-start gap-3 sm:items-center">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-medium text-text">{name}</span>
          <span className="text-xs text-dim">
            {[formatDay(item.entry.occurred_on, locale), files > 0 ? t('trash.withFiles', { count: files }) : null].filter(Boolean).join(' · ')}
          </span>
          <span className="text-xs text-faint">
            {t('trash.deletedAt', { when: formatMoment(item.deleted_at, locale) })} · {t('trash.erasedOn', { date: formatMomentDay(item.erased_after, locale) })}
          </span>
        </div>
        <span className="flex flex-col items-end">
          {movements(item.entry).map((movement) => (
            <Money key={movement.accountId} amount={movement.amount} currency={movement.currency} signDisplay="exceptZero" />
          ))}
        </span>
      </div>
      <span className={ACTIONS}>
        <Button size="sm" variant="primary" onClick={() => restore.mutate()} disabled={restore.isPending} aria-label={t('trash.restoreNamed', { name })}>
          {t('trash.restore')}
        </Button>
        <Erase
          trigger={t('trash.erase')}
          triggerLabel={t('trash.eraseNamed', { name })}
          title={t('trash.eraseTitle')}
          body={t('trash.eraseEntryBody')}
          disabled={erase.isPending}
          onErase={() => erase.mutate()}
          small
        />
      </span>
    </li>
  )
}

function RemovedFile({ item }: { item: TrashedAttachment }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const { attachment } = item
  const restore = useRestore(() => api.restoreAttachment(attachment.id))
  const erase = useErase(() => api.eraseAttachment(attachment.id))
  const from = item.entry_title || t('entry.untitled')

  return (
    <li className={ROW}>
      <div className="flex min-w-0 flex-1 items-start gap-3 sm:items-center">
        {isPicture(attachment.media_type) ? (
          <img src={attachmentUrl(attachment.id)} alt="" className="size-10 shrink-0 rounded-sm border border-line object-cover" />
        ) : (
          <span className="flex size-10 shrink-0 items-center justify-center rounded-sm border border-line text-dim">
            <FileText aria-hidden className="size-5" />
          </span>
        )}
        <div className="flex min-w-0 flex-1 flex-col">
          <a href={attachmentUrl(attachment.id)} target="_blank" rel="noopener" className="truncate font-medium text-text hover:text-accent">
            {attachment.name}
          </a>
          <span className="text-xs text-dim">
            {t('trash.fromEntry', { title: from, date: formatDay(item.entry_occurred_on, locale) })} · {formatSize(attachment.size, locale)}
          </span>
          <span className="text-xs text-faint">
            {t('trash.removedAt', { when: formatMoment(item.removed_at, locale) })} · {t('trash.erasedOn', { date: formatMomentDay(item.erased_after, locale) })}
          </span>
        </div>
      </div>
      <span className={ACTIONS}>
        <Button
          size="sm"
          variant="primary"
          onClick={() => restore.mutate()}
          disabled={restore.isPending}
          aria-label={t('trash.restoreNamed', { name: attachment.name })}
        >
          {t('trash.restore')}
        </Button>
        <Erase
          trigger={t('trash.erase')}
          triggerLabel={t('trash.eraseNamed', { name: attachment.name })}
          title={t('trash.eraseTitle')}
          body={t('trash.eraseFileBody')}
          disabled={erase.isPending}
          onErase={() => erase.mutate()}
          small
        />
      </span>
    </li>
  )
}

/** A trade in the trash: what, where, how many and what its money was; back,
 * its money is booked again. */
function DeletedTrade({ item }: { item: TrashedTrade }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const accounts = useAccounts()
  const instruments = useInstruments()
  const { trade } = item
  const symbol = instruments.data?.find((instrument) => instrument.id === trade.instrument_id)?.symbol ?? ''
  const account = (id: string) => accounts.data?.find((each) => each.id === id)?.name ?? ''
  const kind = t(`trade.kinds.${trade.kind}`)
  const name = `${kind} ${formatQuantity(trade.quantity, locale)} ${symbol}`
  const changed = useTradesChanged()
  const restore = useRestore(() => api.restoreTrade(trade.id), changed)
  const erase = useErase(() => api.eraseTrade(trade.id), changed)

  return (
    <li className={ROW}>
      <div className="flex min-w-0 flex-1 items-start gap-3 sm:items-center">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-center gap-2 font-medium text-text">
            <Badge variant={trade.kind === 'buy' ? 'good' : trade.kind === 'sell' ? 'info' : 'soft'}>{kind}</Badge>
            <span className="truncate">
              {formatQuantity(trade.quantity, locale)} <span className="font-mono">{symbol}</span>
            </span>
          </span>
          <span className="text-xs text-dim">
            {[
              formatDay(trade.occurred_on, locale),
              trade.to_account_id ? t('trade.fromTo', { from: account(trade.account_id), to: account(trade.to_account_id) }) : account(trade.account_id),
            ].join(' · ')}
          </span>
          <span className="text-xs text-faint">
            {t('trash.deletedAt', { when: formatMoment(item.deleted_at, locale) })} · {t('trash.erasedOn', { date: formatMomentDay(item.erased_after, locale) })}
          </span>
        </div>
        {trade.money && trade.currency ? <Money amount={trade.money} currency={trade.currency} signDisplay="exceptZero" /> : null}
      </div>
      <span className={ACTIONS}>
        <Button size="sm" variant="primary" onClick={() => restore.mutate()} disabled={restore.isPending} aria-label={t('trash.restoreNamed', { name })}>
          {t('trash.restore')}
        </Button>
        <Erase
          trigger={t('trash.erase')}
          triggerLabel={t('trash.eraseNamed', { name })}
          title={t('trash.eraseTitle')}
          body={t('trash.eraseTradeBody')}
          disabled={erase.isPending}
          onErase={() => erase.mutate()}
          small
        />
      </span>
    </li>
  )
}

/** Putting something back: the books are read again, since its money is in
 * them once more - and the portfolio, for a trade. */
function useRestore(restore: () => Promise<unknown>, after?: () => unknown) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const books = useBooksChanged()
  const changed = after ?? books
  return useMutation({
    mutationFn: restore,
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('trash.restored') })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })
}

function useErase(erase: () => Promise<unknown>, after?: () => unknown) {
  const { t } = useTranslation()
  const toasts = useToastManager()
  const books = useBooksChanged()
  const changed = after ?? books
  return useMutation({
    mutationFn: erase,
    onSuccess: () => {
      void changed()
      toasts.add({ type: 'success', title: t('trash.erased') })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: problem(error, t) }),
  })
}

interface EraseProps {
  trigger: string
  /** What the button is called to a screen reader, when its word alone does
   * not say which row it erases. */
  triggerLabel?: string
  title: string
  body: string
  disabled: boolean
  onErase: () => void
  small?: boolean
}

/** Erasing for good is asked about first: nothing brings it back. */
function Erase({ trigger, triggerLabel, title, body, disabled, onErase, small }: EraseProps) {
  const { t } = useTranslation()
  return (
    <ConfirmDialog>
      <ConfirmDialogTrigger render={<Button variant="danger" size={small ? 'sm' : undefined} disabled={disabled} aria-label={triggerLabel} />}>
        {trigger}
      </ConfirmDialogTrigger>
      <ConfirmDialogPopup>
        <ConfirmDialogHeader>
          <ConfirmDialogTitle>{title}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{body}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <Button render={<ConfirmDialogClose />}>{t('common.cancel')}</Button>
          <Button variant="danger" render={<ConfirmDialogClose />} onClick={onErase}>
            {trigger}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}
