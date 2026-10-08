import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Accounts } from '@/pages/Accounts'
import { app, json, signedIn } from '@/test/server'

const plain = (text: string | null) => (text ?? '').replace(/\s/g, ' ')

const ACCOUNTS = [
  { id: 'a1', kind: 'cash', name: 'Wallet', currency: 'PYG', opening_balance: '0', closed_at: null },
  { id: 'a2', kind: 'bank', name: 'Itaú', currency: 'PYG', opening_balance: '0', closed_at: null },
  { id: 'a3', kind: 'crypto_wallet', name: 'Cold', currency: 'BTC', opening_balance: '0', closed_at: null },
  { id: 'a4', kind: 'card', name: 'Old card', currency: 'USD', opening_balance: '0', closed_at: '2026-01-01T00:00:00Z' },
]

const BALANCES = {
  as_of: '2026-10-08',
  accounts: [
    // As the server writes them: eighteen decimals, whatever the currency.
    { account_id: 'a1', name: 'Wallet', currency: 'PYG', amount: '1250000.000000000000000000', available: '1250000.000000000000000000' },
    { account_id: 'a2', name: 'Itaú', currency: 'PYG', amount: '8000000.000000000000000000', available: '7820000.000000000000000000' },
    { account_id: 'a3', name: 'Cold', currency: 'BTC', amount: '0.015000000000000000', available: '0.015000000000000000' },
  ],
  total: { currency: 'PYG', amount: '9250000', available: '9070000', unconverted: ['BTC'] },
}

describe('Accounts', () => {
  it('writes guaraníes without kopecks, and a coin with its digits', async () => {
    signedIn({ '/api/v1/ledger/accounts': json(ACCOUNTS), '/api/v1/ledger/balances': json(BALANCES) })
    render(app(<Accounts />))
    const wallet = (await screen.findByRole('link', { name: 'Wallet' })).closest('tr')!
    expect(plain(within(wallet).getAllByText(/₲/)[0]!.textContent)).toBe('₲1,250,000')
    const cold = screen.getByRole('link', { name: 'Cold' }).closest('tr')!
    expect(plain(cold.textContent)).toContain('0.015 BTC')
  })

  it('names the currencies the total leaves out', async () => {
    signedIn({ '/api/v1/ledger/accounts': json(ACCOUNTS), '/api/v1/ledger/balances': json(BALANCES) })
    render(app(<Accounts />))
    expect(await screen.findByText('Not counted - no rate known for BTC.')).toBeTruthy()
    expect(plain(screen.getByText('₲9,250,000').textContent)).toBe('₲9,250,000')
  })

  it('shows what is available only where something is held', async () => {
    signedIn({ '/api/v1/ledger/accounts': json(ACCOUNTS), '/api/v1/ledger/balances': json(BALANCES) })
    render(app(<Accounts />))
    expect(await screen.findByRole('columnheader', { name: 'Available' })).toBeTruthy()
    const bank = screen.getByRole('link', { name: 'Itaú' }).closest('tr')!
    expect(plain(bank.textContent)).toContain('₲7,820,000')
  })

  it('keeps a closed account apart, with a way back', async () => {
    signedIn({ '/api/v1/ledger/accounts': json(ACCOUNTS), '/api/v1/ledger/balances': json(BALANCES) })
    render(app(<Accounts />))
    const closed = await screen.findByRole('heading', { name: 'Closed accounts' })
    const section = closed.closest('section')!
    expect(within(section).getByText('Old card')).toBeTruthy()
    expect(within(section).getByRole('button', { name: 'Reopen' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Old card' })?.closest('tr')).toBeFalsy()
  })

  it('invites the first account into empty books', async () => {
    signedIn({ '/api/v1/ledger/accounts': json([]), '/api/v1/ledger/balances': json({ as_of: '2026-10-08', accounts: [] }) })
    render(app(<Accounts />))
    expect(await screen.findByText('No accounts yet')).toBeTruthy()
  })
})
