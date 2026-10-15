import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { useLocale } from 'dowel-ui'
import { Money } from '@/components/Money'
import { formatDay, formatMoment } from '@/lib/dates'
import { changes, type Change, type Posting } from '@/lib/history'
import { placeLabel, useHistory } from '@/lib/ledger'
import { useSession } from '@/lib/session'
import type { Revision, Who } from '@/lib/types'

/**
 * What happened to an entry, oldest first: who recorded it, each change and
 * what it changed from and to, a deletion and a return from the trash.
 *
 * What makes a balance that moved explainable: the entry behind it says when
 * it changed, by whose hand, and what it was before.
 */
export function History({ id }: { id: string }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const history = useHistory(id)
  if (!history.data) return null

  return (
    <section className="flex flex-col gap-2" aria-label={t('history.title')}>
      <h3 className="caption">{t('history.title')}</h3>
      <ol className="flex flex-col gap-2">
        {history.data.map((revision) => (
          <li key={revision.id} className="flex flex-col gap-0.5 border-l-2 border-line pl-3 text-sm">
            <span className="text-text">
              {t(`history.${revision.action}`)} · <WhoName who={revision.who} />
            </span>
            <span className="text-xs text-dim">{formatMoment(revision.at, locale)}</span>
            <Changes revision={revision} />
          </li>
        ))}
      </ol>
    </section>
  )
}

function WhoName({ who }: { who: Who }) {
  const { t } = useTranslation()
  const { user } = useSession()
  switch (who.kind) {
    case 'person':
      return <>{who.person_id === user?.id ? t('history.you') : t('history.someone')}</>
    case 'module':
      return <>{who.module ? t('history.module', { module: who.module }) : t('history.aModule')}</>
    case 'outside':
      return <>{t('history.outside')}</>
  }
}

function Changes({ revision }: { revision: Revision }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const said = changes(revision)
  if (said.length === 0) return null
  return (
    <ul className="flex flex-col text-xs text-dim">
      {said.map((change, index) => (
        <li key={`${change.field}-${index}`}>{describe(change, t, locale)}</li>
      ))}
    </ul>
  )
}

/** One change, as a line of the history. */
function describe(change: Change, t: TFunction, locale: string): ReactNode {
  const quoted = (text: string) => (text ? t('history.quoted', { text }) : t('history.nothing'))
  switch (change.field) {
    case 'occurred_on':
      return t('history.dayChanged', { from: formatDay(change.from, locale), to: formatDay(change.to, locale) })
    case 'description':
      return t('history.descriptionChanged', { from: quoted(change.from), to: quoted(change.to) })
    case 'status':
      return t('history.statusChanged', { from: posting(change.from, t, locale), to: posting(change.to, t, locale) })
    case 'counterparty':
      return t('history.counterpartyChanged', { from: change.from ?? t('history.noOne'), to: change.to ?? t('history.noOne') })
    case 'place':
      return t('history.placeChanged', {
        from: change.from ? placeLabel(change.from, locale) : t('entry.nowhere'),
        to: change.to ? placeLabel(change.to, locale) : t('entry.nowhere'),
      })
    case 'tags':
      return [...change.added.map((tag) => `+#${tag}`), ...change.removed.map((tag) => `−#${tag}`)].join(' ')
    case 'amount':
      return (
        <>
          {t('history.amountChanged')} <Money amount={change.from.amount} currency={change.from.currency} signDisplay="exceptZero" /> →{' '}
          <Money amount={change.to.amount} currency={change.to.currency} signDisplay="exceptZero" />
        </>
      )
    case 'lines':
      return t('history.linesChanged')
    case 'attachments':
      return [
        ...change.added.map((file) => t('history.attached', { name: file.name })),
        ...change.removed.map((file) => t('history.removed', { name: file.name })),
      ].join('; ')
  }
}

function posting(state: Posting, t: TFunction, locale: string): string {
  if (state.held) return t('history.held')
  return state.on ? t('history.postedOn', { date: formatDay(state.on, locale) }) : t('history.posted')
}
