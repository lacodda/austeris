import type { Attachment, Decimal, Entry, IsoDate, Line, Place, Revision } from '@/lib/types'

/*
 * What a change to an entry changed, as a card lists it.
 *
 * The server says which parts changed and sends the entry before and after;
 * this turns each part into the facts a sentence is written from - the old
 * date and the new, the tags put on and taken off. Pure, so what a history
 * says is tested without a browser.
 */

/** When an entry posted, as a person reads it. */
export type Posting = { held: true } | { held: false; on: IsoDate | null }

export type Change =
  | { field: 'occurred_on'; from: IsoDate; to: IsoDate }
  | { field: 'description'; from: string; to: string }
  | { field: 'status'; from: Posting; to: Posting }
  | { field: 'counterparty'; from: string | null; to: string | null }
  | { field: 'place'; from: Place | null; to: Place | null }
  | { field: 'tags'; added: string[]; removed: string[] }
  /** The amount an entry moved its one account by, when it moved one. */
  | { field: 'amount'; from: { amount: Decimal; currency: string }; to: { amount: Decimal; currency: string } }
  /** Lines written again in a shape no one amount says. */
  | { field: 'lines' }
  | { field: 'attachments'; added: Attachment[]; removed: Attachment[] }

/** Everything a change changed, in the order the server listed the parts. */
export function changes(revision: Revision): Change[] {
  const { before, after } = revision
  if (revision.action !== 'changed' || !before || !after) return []
  return revision.changed.map((field): Change => {
    switch (field) {
      case 'occurred_on':
        return { field, from: before.occurred_on, to: after.occurred_on }
      case 'description':
        return { field, from: before.description, to: after.description }
      case 'status':
        return { field, from: posting(before), to: posting(after) }
      case 'counterparty':
        return { field, from: before.counterparty?.name ?? null, to: after.counterparty?.name ?? null }
      case 'place':
        return { field, from: before.place ?? null, to: after.place ?? null }
      case 'tags':
        return {
          field,
          added: after.tags.filter((tag) => !before.tags.includes(tag)),
          removed: before.tags.filter((tag) => !after.tags.includes(tag)),
        }
      case 'lines': {
        const was = accountLine(before.lines)
        const is = accountLine(after.lines)
        return was && is ? { field: 'amount', from: was, to: is } : { field: 'lines' }
      }
      case 'attachments': {
        const ids = (entry: Entry) => new Set(entry.attachments.map((file) => file.id))
        return {
          field,
          added: after.attachments.filter((file) => !ids(before).has(file.id)),
          removed: before.attachments.filter((file) => !ids(after).has(file.id)),
        }
      }
    }
  })
}

function posting(entry: Entry): Posting {
  if (entry.status === 'pending') return { held: true }
  // Posted the day it happened is just "posted": the day is the entry's own.
  return { held: false, on: entry.cleared_on && entry.cleared_on !== entry.occurred_on ? entry.cleared_on : null }
}

/** The one line moving an account, when there is only one. */
function accountLine(lines: Line[]): { amount: Decimal; currency: string } | null {
  const [only, ...more] = lines.filter((line) => line.side === 'account')
  return only && more.length === 0 ? { amount: only.amount, currency: only.currency } : null
}
