import { useMemo, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { currencyDigits, negate, roundTo, sum, times } from '@/lib/decimal'
import { keys as ledgerKeys } from '@/lib/ledger'
import type { Account, Decimal, Trade, TradeBody, TradeKind } from '@/lib/types'

/*
 * The portfolio and the market, as the screens read them.
 *
 * A trade moves money in the books - its entry is the ledger's - so whatever
 * changes a trade refetches the books as well as the portfolio. A price said
 * by hand moves what everything is worth, so it refetches the portfolio's
 * value with the market's prices.
 */

export const keys = {
  portfolio: ['portfolio'] as const,
  trades: ['portfolio', 'trades'] as const,
  value: (currency: string) => ['portfolio', 'value', currency] as const,
  snapshots: ['portfolio', 'snapshots'] as const,
  snapshot: (id: string, against: string | undefined) => ['portfolio', 'snapshot', id, against ?? null] as const,
  history: (id: string) => ['portfolio', 'history', id] as const,
  market: ['market'] as const,
  instruments: ['market', 'instruments'] as const,
  prices: (currency: string) => ['market', 'prices', currency] as const,
}

export const useTrades = () => useQuery({ queryKey: keys.trades, queryFn: api.trades })
export const useValue = (currency: string | undefined) =>
  useQuery({ queryKey: keys.value(currency ?? ''), queryFn: () => api.value(currency ?? 'USD'), enabled: currency !== undefined })
export const useSnapshots = () => useQuery({ queryKey: keys.snapshots, queryFn: api.snapshots })
export const useComparison = (id: string, against?: string) =>
  useQuery({ queryKey: keys.snapshot(id, against), queryFn: () => api.snapshot(id, against) })
export const useTradeHistory = (id: string) => useQuery({ queryKey: keys.history(id), queryFn: () => api.tradeHistory(id) })
export const useInstruments = () => useQuery({ queryKey: keys.instruments, queryFn: api.instruments })
export const usePrices = (currency: string | undefined) =>
  useQuery({ queryKey: keys.prices(currency ?? ''), queryFn: () => api.prices(currency ?? 'USD'), enabled: currency !== undefined })

/** After a trade was recorded, changed or deleted: the portfolio, and the
 * books its money is in. */
export function useTradesChanged() {
  const queries = useQueryClient()
  return () =>
    Promise.all([queries.invalidateQueries({ queryKey: keys.portfolio }), queries.invalidateQueries({ queryKey: ledgerKeys.all })])
}

/** After an instrument was added or priced: the market, and what the
 * portfolio is worth. */
export function useMarketChanged() {
  const queries = useQueryClient()
  return () => Promise.all([queries.invalidateQueries({ queryKey: keys.market }), queries.invalidateQueries({ queryKey: keys.portfolio })])
}

/** The kinds of account positions are held on: a broker's and a wallet's,
 * whose balance is the money a trade there is paid in. */
const HOLDING_KINDS = new Set(['brokerage', 'crypto_wallet'])

/** The accounts a trade can happen on: a broker's or a wallet's, open - or
 * already the one chosen. */
export function holdingAccounts(accounts: readonly Account[] | undefined, chosen?: string): Account[] {
  return (accounts ?? []).filter((account) => HOLDING_KINDS.has(account.kind) && (!account.closed_at || account.id === chosen))
}

/**
 * What a trade's money moves its account by, as the server works it out:
 * negative for a buy - its price and its fee - and positive for a sale - its
 * price less its fee - rounded the way the account's currency is. A
 * transfer moves none.
 */
export function tradeMoney(kind: TradeKind, quantity: Decimal, price: Decimal, fee: Decimal, currency: string): Decimal | null {
  if (kind === 'transfer') return null
  const gross = times(quantity, price)
  const signed = kind === 'buy' ? negate(sum([gross, fee])) : sum([gross, negate(fee)])
  const places = currencyDigits(currency)
  return places === undefined ? signed : roundTo(signed, places)
}

/** `2026-10-20T14:30`, as a form's date and time fields say a moment, from an
 * instant - on the browser's own clock. */
export function localMoment(at: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`
}

/**
 * A moment a form says - `2026-10-20T14:30`, on the browser's own clock - as
 * the instant with its offset: `2026-10-20T14:30:00-03:00`. The offset is what
 * tells the server the person's day, which is the day the trade's money is
 * booked under.
 */
export function withOffset(local: string): string {
  const at = new Date(local)
  if (Number.isNaN(at.getTime())) return local
  const minutes = -at.getTimezoneOffset()
  const sign = minutes >= 0 ? '+' : '-'
  const pad = (n: number) => String(Math.trunc(Math.abs(n))).padStart(2, '0')
  return `${local.length === 16 ? `${local}:00` : local}${sign}${pad(minutes / 60)}:${pad(minutes % 60)}`
}

/** What a trade form holds, as typed and already read as decimals. */
export interface TradeDraft {
  kind: TradeKind
  accountId: string
  toAccountId?: string
  instrumentId: string
  quantity: Decimal
  price?: Decimal
  fee?: Decimal
  /** `2026-10-20T14:30`, on the browser's own clock. */
  at: string
  note: string
}

/** What a trade form sends: a transfer with no price, a buy or a sale with no
 * destination - the server refuses either way round. */
export function tradeBody(draft: TradeDraft): TradeBody {
  const transfer = draft.kind === 'transfer'
  return {
    kind: draft.kind,
    account_id: draft.accountId,
    to_account_id: transfer ? (draft.toAccountId ?? null) : null,
    instrument_id: draft.instrumentId,
    quantity: draft.quantity,
    price: transfer ? null : (draft.price ?? null),
    fee: draft.fee ?? null,
    occurred_at: withOffset(draft.at),
    note: draft.note.trim(),
  }
}

/** A trade as its form starts out when it is changed. */
export function draftOf(trade: Trade): TradeDraft {
  return {
    kind: trade.kind,
    accountId: trade.account_id,
    toAccountId: trade.to_account_id ?? undefined,
    instrumentId: trade.instrument_id,
    quantity: trade.quantity,
    price: trade.price ?? undefined,
    fee: trade.fee,
    at: localMoment(new Date(trade.occurred_at)),
    note: trade.note,
  }
}


/** A number of units in a language, every digit it has: `0.0099` stays
 * `0.0099`, and a thousand shares are `1,000`. */
export function formatQuantity(quantity: Decimal, locale: string): string {
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 18 }).format(quantity as Intl.StringNumericLiteral)
}

const VALUE_CURRENCY = 'austeris.portfolioCurrency'

/**
 * The currency the portfolio is valued in, and the ones it can be.
 *
 * Remembered per browser, like the total on the accounts screen: it is how
 * this person reads their money. Until they choose, the currency most of
 * their accounts are in - the one a total means something in - with the
 * dollar and the euro beside it, which price sources quote in.
 */
export function useValueCurrency(accounts: readonly Account[] | undefined): [string | undefined, (currency: string) => void, string[]] {
  const [chosen, setChosen] = useState<string | null>(() => {
    try {
      return localStorage.getItem(VALUE_CURRENCY)
    } catch {
      return null
    }
  })
  const options = useMemo(() => {
    const count = new Map<string, number>()
    for (const account of accounts ?? []) {
      if (!account.closed_at) count.set(account.currency, (count.get(account.currency) ?? 0) + 1)
    }
    const held = [...count.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([currency]) => currency)
    return [...new Set([...held, 'USD', 'EUR'])]
  }, [accounts])
  const current = accounts === undefined ? undefined : (chosen ?? options[0])
  const choose = (currency: string) => {
    setChosen(currency)
    try {
      localStorage.setItem(VALUE_CURRENCY, currency)
    } catch {
      // Not remembered; still chosen for this visit.
    }
  }
  return [current, choose, chosen && !options.includes(chosen) ? [chosen, ...options] : options]
}
