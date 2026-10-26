import { describe, expect, it } from 'vitest'
import { draftOf, holdingAccounts, localMoment, tradeBody, tradeMoney, withOffset } from '@/lib/portfolio'
import type { Account, Trade } from '@/lib/types'

describe('a trade on the screen', () => {
  it('costs its price and its fee, and a sale brings its price less its fee', () => {
    expect(tradeMoney('buy', '0.01', '60000', '1', 'USDT')).toBe('-601.00')
    expect(tradeMoney('sell', '0.004', '65000', '0.26', 'USDT')).toBe('259.740')
    expect(tradeMoney('transfer', '0.01', '0', '0.0001', 'USDT')).toBeNull()
  })

  it('rounds its money the way the account is kept, as the server books it', () => {
    // Dollars have cents; a stablecoin keeps every digit.
    expect(tradeMoney('buy', '3', '101.333', '0', 'USD')).toBe('-304.00')
    expect(tradeMoney('buy', '3', '101.333', '0', 'USDT')).toBe('-303.999')
  })

  it('says its moment with the offset that tells the server the person`s day', () => {
    const sent = withOffset('2026-10-20T14:30')
    expect(sent).toMatch(/^2026-10-20T14:30:00[+-]\d\d:\d\d$/)
    // The same instant the browser means by that moment, wherever it is.
    expect(new Date(sent).getTime()).toBe(new Date('2026-10-20T14:30').getTime())
    expect(localMoment(new Date('2026-10-20T14:30'))).toBe('2026-10-20T14:30')
  })

  it('sends a transfer without a price and a buy without a destination', () => {
    const common = { accountId: 'a1', toAccountId: 'a2', instrumentId: 'i1', quantity: '0.01', price: '60000', fee: '1', at: '2026-10-20T14:30', note: ' cold storage ' }
    const transfer = tradeBody({ ...common, kind: 'transfer' })
    expect(transfer.price).toBeNull()
    expect(transfer.to_account_id).toBe('a2')
    expect(transfer.note).toBe('cold storage')
    const buy = tradeBody({ ...common, kind: 'buy' })
    expect(buy.to_account_id).toBeNull()
    expect(buy.price).toBe('60000')
  })

  it('opens a trade as it was recorded', () => {
    const trade: Trade = {
      id: 't1',
      kind: 'sell',
      account_id: 'a1',
      instrument_id: 'i1',
      quantity: '0.004',
      price: '65000',
      currency: 'USDT',
      fee: '0.26',
      occurred_at: new Date('2026-10-20T14:30').toISOString(),
      occurred_on: '2026-10-20',
      note: '',
      created_at: '2026-10-20T17:30:00Z',
      updated_at: '2026-10-20T17:30:00Z',
    }
    expect(draftOf(trade)).toMatchObject({ kind: 'sell', quantity: '0.004', price: '65000', fee: '0.26', at: '2026-10-20T14:30' })
  })
})

describe('the accounts positions are held on', () => {
  const account = (id: string, kind: Account['kind'], closed = false): Account => ({
    id,
    kind,
    name: id,
    currency: 'USD',
    opening_balance: '0',
    closed_at: closed ? '2026-10-01T00:00:00Z' : null,
  })

  it('are a broker`s and a wallet`s, open - or the one a trade is already on', () => {
    const accounts = [account('bank', 'bank'), account('broker', 'brokerage'), account('wallet', 'crypto_wallet'), account('old', 'brokerage', true)]
    expect(holdingAccounts(accounts).map((each) => each.id)).toEqual(['broker', 'wallet'])
    expect(holdingAccounts(accounts, 'old').map((each) => each.id)).toEqual(['broker', 'wallet', 'old'])
  })
})
