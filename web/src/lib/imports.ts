import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { currencyDigits, over, roundTo, times } from '@/lib/decimal'
import { keys as ledgerKeys } from '@/lib/ledger'
import type { Decimal, Receipt, SlipReading } from '@/lib/types'

/*
 * Documents read into the books, as the screens use them: which modules run,
 * the documents handed over, and what a slip's reading fills a form with.
 */

export const keys = {
  modules: ['installation', 'modules'] as const,
  documents: ['imports', 'documents'] as const,
}

/** Whether this installation runs a module. `undefined` until it says: a
 * screen hides what may not be there rather than flashing it. */
export function useModule(name: string): boolean | undefined {
  const modules = useQuery({ queryKey: keys.modules, queryFn: api.modules, staleTime: Infinity })
  if (!modules.data) return undefined
  return modules.data.modules.some((module) => module.name === name && module.on)
}

export const useDocuments = () => useQuery({ queryKey: keys.documents, queryFn: () => api.documents() })

/** After a document was handed over or booked: the documents, and the books
 * it may have written to. */
export function useDocumentsChanged() {
  const queries = useQueryClient()
  return () => Promise.all([queries.invalidateQueries({ queryKey: keys.documents }), queries.invalidateQueries({ queryKey: ledgerKeys.all })])
}

/** What a receipt is called: the name the shop trades under, else its own. */
export function shopName(receipt: Receipt): string {
  return receipt.issuer.trade_name || receipt.issuer.name
}

/** The day a receipt was issued, `YYYY-MM-DD`. */
export function issuedOn(receipt: Receipt): string {
  return receipt.issued_at.slice(0, 10)
}

/** What an exchange form takes from a slip: the amount given and the amount
 * got, each in its own account's currency - as read, or worked out from the
 * one that was read and the rate. */
export interface SlipFill {
  given?: Decimal
  got?: Decimal
}

/**
 * Places a slip's reading on an exchange from one currency into another.
 *
 * An amount read in a currency is that side's. A side with no amount read is
 * worked out from the other at the slip's rate, rounded to its currency's
 * places - when the rate can be placed: its base or its quote is one of the
 * two. Nothing is guessed beyond that; the person sees the form filled and
 * saves what they see.
 */
export function fillFromSlip(reading: SlipReading, from: string, to: string): SlipFill {
  const read = (currency: string) => reading.amounts.find((money) => money.currency === currency)?.amount
  const fill: SlipFill = { given: read(from), got: read(to) }

  const rate = reading.rate
  let base: string | undefined
  if (rate?.base === from || rate?.base === to) base = rate.base
  else if (rate?.quote === from) base = to
  else if (rate?.quote === to) base = from
  if (!rate || !base) return fill

  // One of `base` is `rate` of the other: into the cheaper one it multiplies,
  // into the dearer one it divides.
  const into = (amount: Decimal, currency: string) => {
    const places = currencyDigits(currency) ?? 8
    return currency === base ? over(amount, rate.rate, places) : roundTo(times(amount, rate.rate), places)
  }
  if (fill.given === undefined && fill.got !== undefined) fill.given = into(fill.got, from)
  if (fill.got === undefined && fill.given !== undefined) fill.got = into(fill.given, to)
  return fill
}
