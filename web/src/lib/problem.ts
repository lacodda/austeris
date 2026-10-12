import type { TFunction } from 'i18next'
import i18n from '@/i18n'
import { ApiError } from '@/lib/api'

/*
 * What went wrong, as the person reads it.
 *
 * An error the server names with a code is written here in the interface's
 * language, from the facts it sent beside the code; one it does not name yet
 * is the server's own sentence. Codes live under `errors.` in the locale
 * files, and a test of the server's holds every code it can answer with to
 * having a sentence there in each language.
 */

/** The code the server answers a second press with. */
export const POSSIBLE_DUPLICATE = 'entry.possible_duplicate'

/** The sentence for an error, in the interface's language when it can be. */
export function problem(error: Error, t: TFunction): string {
  if (error instanceof ApiError && error.code) {
    const key = `errors.${error.code}`
    if (i18n.exists(key)) return t(key, error.params)
  }
  return error.message
}

/** Whether the server refused an entry as the same one recorded a moment ago. */
export function isSecondPress(error: unknown): error is ApiError {
  return error instanceof ApiError && error.code === POSSIBLE_DUPLICATE
}
