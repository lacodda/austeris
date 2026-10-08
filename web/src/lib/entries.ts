import { negate, sum } from '@/lib/decimal'
import type { Account, Decimal, Entry, NewEntryLine } from '@/lib/types'

/*
 * What a screen needs to know about an entry beyond what the API sends, and
 * the lines a form sends back. Pure, so the rules - the signs, the sums - are
 * tested without a browser.
 */

/** The direction of money a form records. */
export type Direction = 'expense' | 'income'

/** One line of a split, as the form has read it. */
export interface SplitLine {
  categoryId: string
  /** Positive, as typed: the direction gives it its sign. */
  amount: Decimal
  note?: string
}

/**
 * The lines of an expense or an income split across categories.
 *
 * Money leaving an account is negative there and positive on the category it
 * went to, and the two cancel (ADR 0007): an expense of 30 + 15 is the account
 * at -45 and the categories at +30 and +15. An income is the same with the
 * signs turned round. The account's side is the exact sum of the split, so the
 * entry balances by construction rather than by the person's arithmetic.
 */
export function splitLines(direction: Direction, accountId: string, currency: string, split: readonly SplitLine[]): NewEntryLine[] {
  const towardsCategory = (amount: Decimal) => (direction === 'expense' ? amount : negate(amount))
  const categoryLines: NewEntryLine[] = split.map((line) => ({
    category_id: line.categoryId,
    amount: towardsCategory(line.amount),
    currency,
    ...(line.note ? { note: line.note } : {}),
  }))
  const total = sum(split.map((line) => line.amount))
  const accountLine: NewEntryLine = {
    account_id: accountId,
    amount: direction === 'expense' ? negate(total) : total,
    currency,
  }
  return [accountLine, ...categoryLines]
}

/** Money moved between two of a person's own accounts in one currency. */
export function transferLines(from: string, to: string, currency: string, amount: Decimal): NewEntryLine[] {
  return [
    { account_id: from, amount: negate(amount), currency },
    { account_id: to, amount, currency },
  ]
}

/** One account an entry moved, by how much, in its currency. */
export interface Movement {
  accountId: string
  amount: Decimal
  currency: string
}

/**
 * What an entry did to accounts - the column a list of entries is read by.
 *
 * Lines on the same account are added: an exchange's fee is its own line on
 * the account it was paid from, and a person reads the two as one payment.
 * Category and conversion lines are where the money went, not where it was.
 * With an account given, only that account: on its own list, a transfer is
 * the money that left or arrived there.
 */
export function movements(entry: Entry, onlyAccount?: string): Movement[] {
  const byAccount = new Map<string, { amounts: Decimal[]; currency: string }>()
  for (const line of entry.lines) {
    if (line.side !== 'account' || !line.account_id) continue
    if (onlyAccount && line.account_id !== onlyAccount) continue
    const seen = byAccount.get(line.account_id)
    if (seen) seen.amounts.push(line.amount)
    else byAccount.set(line.account_id, { amounts: [line.amount], currency: line.currency })
  }
  return [...byAccount.entries()].map(([accountId, { amounts, currency }]) => ({
    accountId,
    amount: sum(amounts),
    currency,
  }))
}

/** The categories an entry's money went to or came from, each once. */
export function categoriesOf(entry: Entry): string[] {
  const ids: string[] = []
  for (const line of entry.lines) {
    if (line.side === 'category' && line.category_id && !ids.includes(line.category_id)) ids.push(line.category_id)
  }
  return ids
}

/** Whether a form may use an account: open, or already the one chosen. */
export function usable(account: Account, chosen?: string): boolean {
  return !account.closed_at || account.id === chosen
}
