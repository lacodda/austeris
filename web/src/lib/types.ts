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
