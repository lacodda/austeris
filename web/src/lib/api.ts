/*
 * The server, as this application calls it.
 *
 * The session lives in an HttpOnly cookie that the page cannot read, which is
 * the point: a script injected into the page cannot steal it either. So every
 * call is same-origin, and the only way to know who is signed in is to ask
 * (`me`). In development Vite forwards `/api` to austeris - see
 * `vite.config.ts` - and in production the gateway serves this page itself.
 */
import type {
  Account,
  Balances,
  Category,
  Counterparty,
  CounterpartyKind,
  Entry,
  EntryFilter,
  EntryPatch,
  Exchange,
  Flow,
  Identity,
  IsoDate,
  NewAccount,
  NewEntry,
  Place,
  PlaceBody,
  RateInForce,
  Recorded,
  Tag,
} from '@/lib/types'

/** A call the server answered with anything but success. */
export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }

  /** Nobody is signed in, or the session has ended. */
  get isUnauthorized(): boolean {
    return this.status === 401
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: 'same-origin',
    headers: { ...(init.body ? { 'Content-Type': 'application/json' } : {}), ...init.headers },
  })
  if (!response.ok) {
    throw new ApiError(response.status, await reason(response))
  }
  // Read as text first: signing in answers 200 with the cookie and may carry
  // no body at all, which `response.json()` would turn into a failure of a
  // call that succeeded.
  const text = await response.text()
  return (text === '' ? undefined : JSON.parse(text)) as T
}

/** What the server said went wrong.
 *
 * Every austeris endpoint answers a failure with `{ status, error, message }`,
 * and `message` is the sentence written for a person - "you have no category
 * called `food`; create it first" - while `error` is only the status' name.
 * Anything else (a proxy's error page, say) falls back to the status text. */
async function reason(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { message?: unknown; error?: unknown }
    if (typeof body.message === 'string' && body.message !== '') return body.message
    if (typeof body.error === 'string') return body.error
  } catch {
    // Not JSON. The status says enough.
  }
  return response.statusText || `HTTP ${response.status}`
}

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) })

/** A query string from the fields that are set; empty ones are left out
 * rather than sent as `?tag=`, which the server would read as "no tag". */
export function query(params: Record<string, string | number | undefined>): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== '') search.set(key, String(value))
  }
  const text = search.toString()
  return text === '' ? '' : `?${text}`
}

const LEDGER = '/api/v1/ledger'

export const api = {
  me: () => request<Identity>('/api/v1/auth/me'),
  signIn: (email: string, password: string) => request<void>('/api/v1/auth/login', json('POST', { email, password })),
  signOut: () => request<void>('/api/v1/auth/logout', { method: 'POST' }),

  accounts: () => request<Account[]>(`${LEDGER}/accounts`),
  createAccount: (account: NewAccount) => request<Account>(`${LEDGER}/accounts`, json('POST', account)),
  setClosed: (id: string, closed: boolean) => request<Account>(`${LEDGER}/accounts/${id}/close`, json('POST', { closed })),
  balances: (currency?: string) => request<Balances>(`${LEDGER}/balances${query({ currency })}`),

  categories: () => request<Category[]>(`${LEDGER}/categories`),
  createCategory: (name: string, flow: Flow) => request<Category>(`${LEDGER}/categories`, json('POST', { name, flow })),
  counterparties: () => request<Counterparty[]>(`${LEDGER}/counterparties`),
  createCounterparty: (name: string, defaultCategory?: string, kind?: CounterpartyKind) =>
    request<Counterparty>(
      `${LEDGER}/counterparties`,
      json('POST', { name, default_category_id: defaultCategory ?? null, kind: kind ?? null }),
    ),
  places: () => request<Place[]>(`${LEDGER}/places`),
  tags: () => request<Tag[]>(`${LEDGER}/tags`),

  entries: (filter: EntryFilter, limit: number, offset: number) =>
    request<Entry[]>(`${LEDGER}/entries${query({ ...filter, limit, offset })}`),
  entry: (id: string) => request<Entry>(`${LEDGER}/entries/${id}`),
  createEntry: (entry: NewEntry) => request<Entry>(`${LEDGER}/entries`, json('POST', entry)),
  quick: (text: string, occurredOn?: IsoDate, place?: PlaceBody) =>
    request<Recorded>(`${LEDGER}/entries/quick`, json('POST', { text, occurred_on: occurredOn ?? null, place: place ?? null })),
  updateEntry: (id: string, patch: EntryPatch) => request<Entry>(`${LEDGER}/entries/${id}`, json('PATCH', patch)),
  clearEntry: (id: string, on?: IsoDate, amount?: string) =>
    request<Entry>(`${LEDGER}/entries/${id}/clear`, json('POST', { on: on ?? null, amount: amount ?? null })),
  deleteEntry: (id: string) => request<void>(`${LEDGER}/entries/${id}`, { method: 'DELETE' }),
  exchange: (exchange: Exchange) => request<Recorded>(`${LEDGER}/exchanges`, json('POST', exchange)),

  rateAt: (base: string, quote: string, on?: IsoDate) =>
    request<RateInForce>(`${LEDGER}/rates/at${query({ base, quote, on })}`),
}
