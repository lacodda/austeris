import type { ReactNode } from 'react'
import { Button } from './button'
import { EmptyState } from './empty-state'
import { SkeletonList } from './skeleton'

/*
 * The three screens between asking for data and showing it.
 *
 * Every list in every product writes the same ladder - loading, then failed,
 * then nothing found, then the content - and writes it slightly differently
 * each time. What differs is never deliberate: one screen forgets the empty
 * case, another shows a spinner where the shape was known, a third prints the
 * raw error object. This is that ladder, once.
 *
 * **It takes values, not a query.** `useQuery` from TanStack Query hands back
 * `isPending` and `error`, and those are ordinary values - so this component
 * asks for them rather than for the query result, and works the same with SWR,
 * with a reducer, or with two `useState` calls. Taking the result object would
 * put a library in the registry and therefore in every product that installs
 * this primitive, to save one line at the call site:
 *
 *     const works = useQuery({ queryKey, queryFn })
 *     <QueryState pending={works.isPending} error={works.error} empty={!works.data?.length} …>
 *
 * **The order of the cases is the component**, and it is the part that goes
 * wrong by hand: pending first, because a refetch that already has data should
 * not blank the screen; then error, because an error with stale data is still
 * an error; then empty, which is only knowable once something arrived.
 *
 * What it deliberately does not do is fetch, retry or cache. Those belong to
 * whatever owns the data - and a component that guessed at them would be
 * wrong for the product that owns them differently.
 *
 * **But it offers the retry it is handed.** A failed load with no way to try
 * again is a dead end, and kilna had nineteen of them - every one wrote
 * "could not load" and none called the `refetch` it already had. `onRetry`
 * puts a button on the default error screen, and hands itself to a custom
 * one, so a product passes `works.refetch` once and every failure has a way
 * out.
 */

/*
 * A retry, with the word for it.
 *
 * The two travel together as a union, the way Chip's remove does: a retry
 * button exists only where there is a word for it, and the word is the
 * product's - a string invented here could not be translated.
 */
type Retry =
  | { onRetry: () => void; retryLabel: string }
  | { onRetry?: never; retryLabel?: never }

export type QueryStateProps = QueryStateOwnProps & Retry

interface QueryStateOwnProps {
  /** Nothing has arrived yet. */
  pending?: boolean
  /** It failed. Anything with a `message`, which is what every error library
   * agrees on - or a string, for a product that carries its own. */
  error?: { message: string } | string | null
  /** Something arrived, and it was nothing. Computed by the caller, because
   * only the caller knows whether empty means `[]`, `null` or a count of
   * zero. */
  empty?: boolean
  /** What stands in while pending. A shape, ideally - the default is a list,
   * because most screens are waiting for one, but a screen waiting for a card
   * should say so. */
  skeleton?: ReactNode
  /** What is shown when nothing came back. */
  emptyState?: ReactNode
  /** What is shown when it failed. Given the message, so a product can put it
   * where it likes - or ignore it - and the retry, when there is one. */
  errorState?: (message: string, retry?: () => void) => ReactNode
  /** The words on the default error screen. Required only in the sense that
   * the default screen needs them; pass `errorState` instead and they are
   * never read. */
  errorLabels?: { title: string; body?: string }
  /** Draw the default error screen without its frame, for a list inside a
   * panel or a widget that already has one. The empty screen is the
   * caller's, and says for itself whether it is plain. */
  plain?: boolean
  children: ReactNode
}

export function QueryState({
  pending = false,
  error = null,
  empty = false,
  skeleton,
  emptyState,
  errorState,
  errorLabels,
  plain = false,
  onRetry,
  retryLabel,
  children,
}: QueryStateProps) {
  /* `aria-busy` on the wrapper, in every state.
   *
   * This is what tells a screen reader that the region is working, and it is
   * why the Skeletons themselves are `aria-hidden`: the fact belongs to the
   * region, said once, rather than to a dozen empty boxes announced as
   * content. */
  const wrap = (content: ReactNode) => <div aria-busy={pending || undefined}>{content}</div>

  // Pending first: a refetch that still holds data should not blank the screen
  // it is refreshing, and the caller decides that by passing `pending` only
  // when there is nothing to show.
  if (pending) return wrap(skeleton ?? <SkeletonList />)

  if (error) {
    const message = typeof error === 'string' ? error : error.message
    if (errorState) return wrap(errorState(message, onRetry))
    return wrap(
      <EmptyState
        variant="error"
        plain={plain}
        title={errorLabels?.title ?? message}
        // The message is shown as the body when there is a title above it, and
        // as the title when there is not - so it is never lost, and never
        // printed twice.
        body={errorLabels?.title ? (errorLabels.body ?? message) : errorLabels?.body}
        action={
          onRetry === undefined ? undefined : (
            <Button size="sm" onClick={onRetry}>
              {retryLabel}
            </Button>
          )
        }
      />,
    )
  }

  // Empty last of the three: it is the only one that cannot be known until
  // something has arrived.
  if (empty && emptyState) return wrap(emptyState)

  return wrap(children)
}
