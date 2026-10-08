import type { ReactNode } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, vi } from 'vitest'
import { Toasts } from '@/components/Toasts'
import { ToastProvider } from '@/components/ui/toast'
import { SessionProvider } from '@/lib/session'

/*
 * The server, for the length of one test.
 *
 * A test says what the API answers rather than reaching into the screens'
 * state: the providers, the API client and the screens are then the real ones,
 * and what a screen sent is read back from `requests` - the body a form
 * posted is the thing worth asserting, not the state that produced it.
 */

afterEach(() => {
  vi.unstubAllGlobals()
})

type Answer = (body: unknown) => Response | Promise<Response>

export interface Sent {
  method: string
  path: string
  body: unknown
}

/** Answer each `METHOD /path` (or bare `/path`, for GET) with its own
 * response; anything else fails the test. Paths match without the query. */
export function serve(routes: Record<string, Answer>): Sent[] {
  const requests: Sent[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, 'http://test')
      const method = (init?.method ?? 'GET').toUpperCase()
      const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
      requests.push({ method, path: url.pathname + url.search, body })
      const answer = routes[`${method} ${url.pathname}`] ?? (method === 'GET' ? routes[url.pathname] : undefined)
      if (!answer) throw new TypeError(`the test server has no answer for ${method} ${url.pathname}`)
      return answer(body)
    }),
  )
  return requests
}

export const json = (value: unknown, status = 200): Answer => () => Response.json(value, { status })
export const failure = (status: number, message: string): Answer => () =>
  Response.json({ status, error: 'Error', message }, { status })

export const ME = { id: 'u1', email: 'ada@example.com', display_name: 'Ada' }

/** Signed in as Ada, with whatever else the test serves. */
export function signedIn(routes: Record<string, Answer> = {}): Sent[] {
  return serve({ '/api/v1/auth/me': json(ME), 'POST /api/v1/auth/logout': () => new Response(null, { status: 204 }), ...routes })
}

/** Everything a screen is wrapped in, with a fresh cache per render so one
 * test's answers never leak into the next. */
export function app(ui: ReactNode, address = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[address]}>
        <SessionProvider>
          <ToastProvider>
            {ui}
            <Toasts />
          </ToastProvider>
        </SessionProvider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}
