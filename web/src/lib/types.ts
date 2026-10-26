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

/** Where a line's money is: an account, a category, money changing currency,
 * or a holding - money that became something a module holds, a coin or a
 * share, which no report of spending counts. */
export type LineSide = 'account' | 'category' | 'conversion' | 'holding'

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
  /** Kept by the module that recorded it - a trade's money: its day,
   * description and lines change there, and it is deleted there. */
  kept: boolean
  lines: Line[]
  /** The files kept with it, in the order they were attached. */
  attachments: Attachment[]
}

/** A file kept with an entry: what it is, without its bytes. */
export interface Attachment {
  id: string
  name: string
  /** What the server read the bytes as: `image/jpeg`, `application/pdf`. */
  media_type: string
  /** In bytes. */
  size: number
  created_at: string
}

/** Who changed an entry: a person, a module, or a write from outside austeris. */
export interface Who {
  kind: 'person' | 'module' | 'outside'
  person_id?: string
  module?: string
}

export type RevisionAction = 'created' | 'changed' | 'deleted' | 'restored'

/** A part of an entry a change can touch, in the order an entry is read. */
export type EntryField = 'occurred_on' | 'description' | 'status' | 'counterparty' | 'place' | 'tags' | 'lines' | 'attachments'

/** One thing that happened to an entry. */
export interface Revision {
  id: number
  at: string
  action: RevisionAction
  who: Who
  /** What a change changed; empty for the other actions. */
  changed: EntryField[]
  /** Absent when it did not exist yet. */
  before?: Entry | null
  /** Absent once it was deleted. */
  after?: Entry | null
}

export interface TrashedEntry {
  entry: Entry
  deleted_at: string
  who: Who
  /** When it is erased for good, with its history and its files. */
  erased_after: string
}

export interface TrashedAttachment {
  attachment: Attachment
  entry_id: string
  entry_title: string
  entry_occurred_on: IsoDate
  removed_at: string
  erased_after: string
}

export interface Trash {
  /** How many days it keeps what is put in it before erasing it. */
  keep_days: number
  entries: TrashedEntry[]
  attachments: TrashedAttachment[]
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
  allow_duplicate?: boolean
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
  allow_duplicate?: boolean
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

/** A typed line, and what is beside it rather than in it. */
export interface QuickLine {
  text: string
  occurred_on?: IsoDate | null
  place?: PlaceBody | null
  /** The account when the line names none. */
  account_id?: string | null
  allow_duplicate?: boolean
}

/** What a typed line would record, worked out and not written. */
export interface Preview {
  occurred_on: IsoDate
  flow: Flow
  account: Account
  category: Category
  /** The amount as the line said it. */
  amount: Money
  /** What the account would move by, signed, in its currency. */
  moves: Decimal
  description: string
  /** Who; without an `id` it is a name recording would create. */
  counterparty?: { id?: string; name: string } | null
  tags: string[]
  new_tags: string[]
  pending: boolean
  conversion?: ConversionSummary | null
}

/** A button that records the same payment again. */
export interface Template {
  id: string
  name: string
  account_id: string
  category_id: string
  flow: Flow
  /** Absent when it asks for the amount each time. */
  amount?: Decimal | null
  currency: string
  description: string
  counterparty?: { id: string; name: string } | null
  place?: Place | null
  tags: string[]
  pending: boolean
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

/** A module of the installation, and whether it runs. */
export interface Module {
  name: string
  on: boolean
}

/** Where a document handed over stands. */
export type DocumentState = 'waiting' | 'booked' | 'refused'

/** What a waiting document needs the person to say. */
export type Needed = 'account' | 'category'

/** Why a document waits or was refused: a code said in the interface's
 * language from its params, and the server's sentence to fall back on. */
export interface Reason {
  code: string
  params: Record<string, unknown>
  message: string
}

/** A receipt as its SIFEN document says it. */
export interface Receipt {
  code?: string | null
  kind: 'invoice' | 'credit_note'
  number?: string | null
  /** In the shop's own time, without a zone: `2026-10-18T16:12:43`. */
  issued_at: string
  issuer: {
    name: string
    trade_name?: string | null
    tax_id?: string | null
    city?: string | null
  }
  currency: string
  items: { description: string; quantity: Decimal; unit?: string | null; total: Decimal }[]
  total: Decimal
  on_credit: boolean
  payments: { means: string; amount: Decimal; currency: string; card_digits?: string | null }[]
}

/** A document handed to austeris to be read into the books. */
export interface ImportDocument {
  id: string
  name: string
  size: number
  origin: 'upload' | 'folder'
  state: DocumentState
  receipt?: Receipt | null
  /** The name its shop is filed under in the books. */
  shop?: string | null
  account_id?: string | null
  category_id?: string | null
  needs: Needed[]
  reason?: Reason | null
  entry_id?: string | null
  received_at: string
  booked_at?: string | null
}

/** What handing a document over came to. */
export interface Upload {
  document: ImportDocument
  /** It had arrived before, as this file or as the same receipt. */
  already: boolean
}

/** What an exchange office's slip was read as. */
export interface SlipReading {
  amounts: { amount: Decimal; currency: string }[]
  /** One of `base` is `rate` of `quote`; either currency may be unknown. */
  rate?: { base?: string | null; quote?: string | null; rate: Decimal; stated: boolean } | null
  text: string
}

/*
 * The market: what can be priced, and what it is worth.
 */

export type InstrumentKind = 'crypto' | 'stock' | 'bond' | 'fund' | 'fx' | 'manual'

/** The kinds a person adds by hand: a currency pair comes from the central
 * banks, never from a form. */
export const INSTRUMENT_KINDS: readonly InstrumentKind[] = ['crypto', 'stock', 'bond', 'fund', 'manual']

export interface Instrument {
  id: string
  kind: InstrumentKind
  symbol: string
  name: string
  decimals?: number | null
}

export interface NewInstrument {
  kind: InstrumentKind
  symbol: string
  name: string
}

/** The latest price of an instrument, and whether it is still current. */
export interface LatestPrice {
  instrument_id: string
  quote_currency: string
  observed_at: string
  price: Decimal
  source: string
  stale: boolean
}

export interface NewPrice {
  price: Decimal
  currency: string
  /** Now when left out. */
  observed_at?: string
}

/*
 * The portfolio: trades, what they add up to, and what it is worth.
 */

export type TradeKind = 'buy' | 'sell' | 'transfer'

export const TRADE_KINDS: readonly TradeKind[] = ['buy', 'sell', 'transfer']

export interface Trade {
  id: string
  kind: TradeKind
  account_id: string
  to_account_id?: string | null
  instrument_id: string
  quantity: Decimal
  /** In `currency`; a transfer has none. */
  price?: Decimal | null
  currency?: string | null
  /** In `currency` for a buy or a sale, in units for a transfer. */
  fee: Decimal
  occurred_at: string
  occurred_on: IsoDate
  note: string
  entry_id?: string | null
  /** What its money moved the account by: negative for a buy. */
  money?: Decimal | null
  created_at: string
  updated_at: string
}

export interface TradeBody {
  kind: TradeKind
  account_id: string
  to_account_id?: string | null
  instrument_id: string
  quantity: Decimal
  price?: Decimal | null
  fee?: Decimal | null
  /** The instant, with the offset of where it happened. */
  occurred_at: string
  note?: string
}

export interface Position {
  account_id: string
  instrument_id: string
  quantity: Decimal
}

export type HoldingKind = 'cash' | 'position'

/** A line of what an account holds, and what it is worth. */
export interface Holding {
  kind: HoldingKind
  instrument_id?: string | null
  /** The instrument's ticker, or the money's currency. */
  symbol: string
  name: string
  quantity: Decimal
  /** In the valuation's currency; absent when nothing is known. */
  price?: Decimal | null
  priced_at?: string | null
  value?: Decimal | null
  stale: boolean
}

export interface AccountValue {
  account_id: string
  name: string
  holdings: Holding[]
  total: Decimal
}

export interface Valuation {
  on: IsoDate
  at: string
  currency: string
  accounts: AccountValue[]
  total: Decimal
  /** Left out of the total for want of a price, by ticker or currency. */
  unvalued: string[]
  stale: boolean
}

export interface Snapshot extends Valuation {
  id: string
  note: string
  taken_at: string
}

export interface SnapshotSummary {
  id: string
  on: IsoDate
  currency: string
  note: string
  taken_at: string
  total: Decimal
  incomplete: boolean
  stale: boolean
}

export interface Difference {
  account_id: string
  account_name: string
  kind: HoldingKind
  instrument_id?: string | null
  symbol: string
  quantity_then: Decimal
  quantity_now: Decimal
  value_then?: Decimal | null
  value_now?: Decimal | null
}

export interface Comparison {
  snapshot: Snapshot
  against: Valuation
  against_id?: string | null
  differences: Difference[]
}

export interface TradeState {
  kind: TradeKind
  account_id: string
  to_account_id?: string | null
  instrument_id: string
  quantity: Decimal
  price?: Decimal | null
  currency?: string | null
  fee: Decimal
  occurred_at: string
  occurred_on: IsoDate
  note: string
}

export interface TradeRevision {
  id: number
  at: string
  action: 'created' | 'changed' | 'deleted'
  who: Who
  before?: TradeState | null
  after?: TradeState | null
}
