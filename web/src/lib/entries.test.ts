import { describe, expect, it } from 'vitest'
import { sum } from '@/lib/decimal'
import { categoriesOf, movements, splitLines, transferLines } from '@/lib/entries'
import type { Entry, NewEntryLine } from '@/lib/types'

/** What the ledger checks: every currency's lines add up to nothing. */
function balances(lines: NewEntryLine[]): boolean {
  const currencies = new Set(lines.map((line) => line.currency))
  return [...currencies].every((currency) => {
    const total = sum(lines.filter((line) => line.currency === currency).map((line) => line.amount))
    return /^-?0(\.0+)?$/.test(total)
  })
}

describe('splitLines', () => {
  it('takes an expense out of the account and puts it on the categories', () => {
    const lines = splitLines('expense', 'cash', 'PYG', [
      { categoryId: 'food', amount: '30000' },
      { categoryId: 'home', amount: '15000', note: 'soap' },
    ])
    expect(lines).toEqual([
      { account_id: 'cash', amount: '-45000', currency: 'PYG' },
      { category_id: 'food', amount: '30000', currency: 'PYG' },
      { category_id: 'home', amount: '15000', currency: 'PYG', note: 'soap' },
    ])
    expect(balances(lines)).toBe(true)
  })

  it('turns the signs round for an income', () => {
    const lines = splitLines('income', 'bank', 'USD', [{ categoryId: 'salary', amount: '1200.50' }])
    expect(lines).toEqual([
      { account_id: 'bank', amount: '1200.50', currency: 'USD' },
      { category_id: 'salary', amount: '-1200.50', currency: 'USD' },
    ])
    expect(balances(lines)).toBe(true)
  })

  it('balances to the cent where a float would not', () => {
    const lines = splitLines('expense', 'card', 'USD', [
      { categoryId: 'a', amount: '0.1' },
      { categoryId: 'b', amount: '0.2' },
    ])
    expect(lines[0]?.amount).toBe('-0.3')
    expect(balances(lines)).toBe(true)
  })
})

describe('transferLines', () => {
  it('moves money from one account to the other', () => {
    const lines = transferLines('bank', 'cash', 'PYG', '500000')
    expect(lines).toEqual([
      { account_id: 'bank', amount: '-500000', currency: 'PYG' },
      { account_id: 'cash', amount: '500000', currency: 'PYG' },
    ])
    expect(balances(lines)).toBe(true)
  })
})

const exchange: Entry = {
  id: 'e1',
  occurred_on: '2026-10-01',
  status: 'cleared',
  description: 'Cambios Chaco',
  tags: [],
  lines: [
    { id: '1', side: 'account', account_id: 'pyg', amount: '-600000', currency: 'PYG', note: '' },
    { id: '2', side: 'conversion', amount: '600000', currency: 'PYG', note: '' },
    { id: '3', side: 'conversion', amount: '-100', currency: 'USD', note: '' },
    { id: '4', side: 'account', account_id: 'usd', amount: '100', currency: 'USD', note: '' },
    { id: '5', side: 'account', account_id: 'pyg', amount: '-6500', currency: 'PYG', note: 'fee' },
    { id: '6', side: 'category', category_id: 'fees', amount: '6500', currency: 'PYG', note: 'fee' },
  ],
}

describe('movements', () => {
  it('adds the lines on one account into one movement', () => {
    expect(movements(exchange)).toEqual([
      { accountId: 'pyg', amount: '-606500', currency: 'PYG' },
      { accountId: 'usd', amount: '100', currency: 'USD' },
    ])
  })

  it('shows only the account a list is filtered to', () => {
    expect(movements(exchange, 'usd')).toEqual([{ accountId: 'usd', amount: '100', currency: 'USD' }])
  })
})

describe('categoriesOf', () => {
  it('names each category once, and never a conversion', () => {
    expect(categoriesOf(exchange)).toEqual(['fees'])
  })
})
