import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'
import { ApiError } from '@/lib/api'

/** Who to tell when the server says the session is over. Set by the session
 * provider; a module-level hook because the client is created before any
 * component exists. */
let unauthorized: (() => void) | null = null

export function onUnauthorized(listener: (() => void) | null) {
  unauthorized = listener
}

/** A session that ends while a screen is open - expired, or signed out on
 * another device with "sign out everywhere" - surfaces as a 401 on whatever
 * asked next. That is the moment to show the sign-in form, rather than an
 * error panel on every screen that cannot be fixed by retrying. */
function noticeUnauthorized(error: unknown) {
  if (error instanceof ApiError && error.isUnauthorized) unauthorized?.()
}

/**
 * The one client the application shares, on the line's defaults (kilna's).
 *
 * Data stays fresh for 30 seconds, so moving between screens does not refetch
 * what was just shown. Regaining focus does not refetch everything either. And
 * a failed query fails at once instead of being retried three times behind a
 * spinner - the screen says so, and offers a retry of its own.
 */
export const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError: noticeUnauthorized }),
  mutationCache: new MutationCache({ onError: noticeUnauthorized }),
  defaultOptions: {
    queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: false },
    mutations: { retry: false },
  },
})
