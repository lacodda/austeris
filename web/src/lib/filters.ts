import type { EntryFilter, EntryStatus } from '@/lib/types'

/*
 * The list of entries is filtered from its address: `/entries?account=…&tag=trip`.
 *
 * In the address rather than in component state, so that a balance on the
 * accounts screen can link to that account's entries, a reload keeps what was
 * being looked at, and Back walks out of a filter the way it walks out of a
 * screen.
 */

const FIELDS = ['from', 'to', 'account', 'category', 'tag', 'counterparty', 'place'] as const
const DAY = /^\d{4}-\d{2}-\d{2}$/
const STATUSES: readonly EntryStatus[] = ['pending', 'cleared']

/** The filter an address says, with anything malformed left out rather than
 * sent on: the server would refuse `from=yesterday` with a 400, and a list
 * that shows an error for a typo in the address is the worse answer. */
export function readFilter(params: URLSearchParams): EntryFilter {
  const filter: EntryFilter = {}
  for (const field of FIELDS) {
    const value = params.get(field)?.trim()
    if (!value) continue
    if ((field === 'from' || field === 'to') && !DAY.test(value)) continue
    filter[field] = value
  }
  const status = params.get('status')
  if (status && (STATUSES as readonly string[]).includes(status)) filter.status = status as EntryStatus
  return filter
}

/** The address for a filter: only what is set, in a fixed order, so the same
 * filter is always the same address. */
export function writeFilter(filter: EntryFilter): URLSearchParams {
  const params = new URLSearchParams()
  for (const field of [...FIELDS, 'status'] as const) {
    const value = filter[field]
    if (value) params.set(field, value)
  }
  return params
}

/** Whether anything narrows the list. */
export function isFiltered(filter: EntryFilter): boolean {
  return Object.values(filter).some((value) => value !== undefined && value !== '')
}
