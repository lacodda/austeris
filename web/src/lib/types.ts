/*
 * The shapes the API answers with, as the OpenAPI document describes them.
 *
 * Every amount and rate is a `string`, never a `number`: a JSON number reaching
 * a browser is an IEEE double, and a ledger's figures are lost on the way in
 * before anything renders them (ADR 0004). They stay strings until `Intl`
 * writes them out, and the arithmetic a form needs is done on them as decimals
 * (`decimal.ts`).
 */

/** An amount written as a decimal: `-45000.00`. */
export type Decimal = string

/** A day, `YYYY-MM-DD`. */
export type IsoDate = string

export interface Identity {
  id: string
  email: string
  display_name: string
}

export type AccountKind = 'cash' | 'bank' | 'card' | 'brokerage' | 'crypto_wallet' | 'deposit' | 'loan'

export const ACCOUNT_KINDS: readonly AccountKind[] = ['cash', 'bank', 'card', 'brokerage', 'crypto_wallet', 'deposit', 'loan']

export interface Account {
  id: string
  kind: AccountKind
  name: string
  currency: string
  opening_balance: Decimal
  closed_at?: string | null
}

export interface NewAccount {
  kind: AccountKind
  name: string
  currency: string
  opening_balance?: Decimal
}

export interface RateInForce {
  base_currency: string
  quote_currency: string
  rate: Decimal
  on_date: IsoDate
  source: string
  age_days: number
  stale: boolean
}

export interface Balance {
  account_id: string
  name: string
  currency: string
  amount: Decimal
  available: Decimal
  converted?: Decimal | null
  converted_available?: Decimal | null
  rate_used?: RateInForce | null
}

export interface Total {
  currency: string
  amount: Decimal
  available: Decimal
  /** Absent when empty, as every list the ledger omits when there is nothing
   * to say. */
  stale?: string[]
  unconverted?: string[]
}

export interface Balances {
  as_of: IsoDate
  accounts: Balance[]
  total?: Total | null
}

export type Flow = 'income' | 'expense'

export interface Category {
  id: string
  flow: Flow
  name: string
  parent_id?: string | null
  purpose?: 'exchange_fees' | null
}

export type CounterpartyKind = 'shop' | 'service' | 'person' | 'organisation'

export interface Counterparty {
  id: string
  name: string
  key: string
  kind?: CounterpartyKind | null
  default_category_id?: string | null
}

export interface Place {
  id: string
  country: string
  city?: string | null
}

/** A place as a client states it: found by these, or created. */
export interface PlaceBody {
  country: string
  city?: string | null
}

export interface Tag {
  id: string
  name: string
}

export type LineSide = 'account' | 'category' | 'conversion'

export interface Line {
  id: string
  side: LineSide
  amount: Decimal
  currency: string
  note: string
  account_id?: string | null
  category_id?: string | null
}

export type EntryStatus = 'pending' | 'cleared'

export interface Entry {
  id: string
  occurred_on: IsoDate
  cleared_on?: IsoDate | null
  status: EntryStatus
  description: string
  tags: string[]
  counterparty?: { id: string; name: string } | null
  place?: Place | null
  source?: string | null
  lines: Line[]
}

export interface NewEntryLine {
  amount: Decimal
  currency: string
  account_id?: string
  category_id?: string
  conversion?: boolean
  note?: string
}

export interface NewEntry {
  lines: NewEntryLine[]
  description?: string
  occurred_on?: IsoDate
  counterparty_id?: string | null
  place?: PlaceBody | null
  tags?: string[]
  pending?: boolean
}

export interface EntryPatch {
  description?: string
  occurred_on?: IsoDate
  counterparty_id?: string | null
  place?: PlaceBody | null
  tags?: string[]
}

export interface Exchange {
  from_account: string
  given: Decimal
  to_account: string
  got: Decimal
  occurred_on?: IsoDate
  description?: string
  counterparty_id?: string | null
  place?: PlaceBody | null
  tags?: string[]
}

export interface Money {
  amount: Decimal
  currency: string
}

export interface ConversionSummary {
  given: Money
  got: Money
  deal: { base: string; quote: string; rate: Decimal }
  reference?: RateInForce | null
  fee?: Money | null
  no_fee?: 'no_reference' | 'stale_reference' | null
}

export interface Recorded {
  entry: Entry
  conversion?: ConversionSummary | null
  new_counterparty?: Counterparty | null
  new_tags?: string[]
}

/** What narrows the list of entries; every field optional. */
export interface EntryFilter {
  from?: IsoDate
  to?: IsoDate
  account?: string
  category?: string
  tag?: string
  counterparty?: string
  place?: string
  status?: EntryStatus
}
