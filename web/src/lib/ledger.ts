import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Account, Category, EntryFilter, Place } from '@/lib/types'

/*
 * The books, as the screens read them: one hook per list, one key per list,
 * and one way to say "the books changed".
 *
 * Every key starts with `ledger`. A write - an entry, an exchange, an account
 * closed - can move a balance, a list of entries, a new tag or counterparty
 * all at once, and working out which of them a given write touched is a
 * second copy of the ledger's rules in the browser. Refetching what is on
 * screen is cheap; a balance that silently disagrees with its entries is not.
 */

export const keys = {
  all: ['ledger'] as const,
  accounts: ['ledger', 'accounts'] as const,
  balances: (currency: string | undefined) => ['ledger', 'balances', currency ?? null] as const,
  categories: ['ledger', 'categories'] as const,
  counterparties: ['ledger', 'counterparties'] as const,
  places: ['ledger', 'places'] as const,
  tags: ['ledger', 'tags'] as const,
  entries: (filter: EntryFilter) => ['ledger', 'entries', filter] as const,
  rate: (given: string, got: string, on: string) => ['ledger', 'rate', given, got, on] as const,
}

/** After any write to the books. */
export function useBooksChanged() {
  const queries = useQueryClient()
  return () => queries.invalidateQueries({ queryKey: keys.all })
}

export const useAccounts = () => useQuery({ queryKey: keys.accounts, queryFn: api.accounts })
export const useBalances = (currency: string | undefined) =>
  useQuery({ queryKey: keys.balances(currency), queryFn: () => api.balances(currency) })
export const useCategories = () => useQuery({ queryKey: keys.categories, queryFn: api.categories })
export const useCounterparties = () => useQuery({ queryKey: keys.counterparties, queryFn: api.counterparties })
export const usePlaces = () => useQuery({ queryKey: keys.places, queryFn: api.places })
export const useTags = () => useQuery({ queryKey: keys.tags, queryFn: api.tags })

/** How many entries a page of the list holds. */
export const PAGE = 50

/**
 * The entries a filter selects, a page at a time, newest first.
 *
 * The server answers a page without a count, so each page asks for one more
 * than it shows: the extra row is how the list knows there is another page,
 * without a second query for a number nobody reads.
 */
export function useEntries(filter: EntryFilter) {
  return useInfiniteQuery({
    queryKey: keys.entries(filter),
    initialPageParam: 0,
    queryFn: async ({ pageParam }) => {
      const rows = await api.entries(filter, PAGE + 1, pageParam)
      return { rows: rows.slice(0, PAGE), more: rows.length > PAGE, offset: pageParam }
    },
    getNextPageParam: (last) => (last.more ? last.offset + PAGE : undefined),
  })
}

/** Accounts still in use, in the order the server lists them. */
export function openAccounts(accounts: readonly Account[] | undefined): Account[] {
  return (accounts ?? []).filter((account) => !account.closed_at)
}

/**
 * Each category's name with the ones above it: `Food › Groceries`.
 *
 * A child's own name is often a word that only means something under its
 * parent - "Other", "Fees" - and a picker that lists it bare offers three
 * identical rows.
 */
export function categoryLabels(categories: readonly Category[] | undefined): Map<string, string> {
  const byId = new Map((categories ?? []).map((category) => [category.id, category]))
  const labels = new Map<string, string>()
  for (const category of byId.values()) {
    const path: string[] = []
    let at: Category | undefined = category
    // Bounded by the number of categories, so a cycle the server should never
    // hold cannot hang the page.
    for (let depth = 0; at && depth <= byId.size; depth += 1) {
      path.unshift(at.name)
      at = at.parent_id ? byId.get(at.parent_id) : undefined
    }
    labels.set(category.id, path.join(' › '))
  }
  return labels
}

/** A country's name in a language, from its ISO code; the code itself when
 * the language has no name for it. */
export function countryName(code: string, locale: string): string {
  try {
    return new Intl.DisplayNames([locale], { type: 'region', fallback: 'code' }).of(code.toUpperCase()) ?? code
  } catch {
    return code
  }
}

/** `Asunción, Paraguay`, or the country alone. */
export function placeLabel(place: Pick<Place, 'country' | 'city'>, locale: string): string {
  const country = countryName(place.country, locale)
  return place.city ? `${place.city}, ${country}` : country
}
