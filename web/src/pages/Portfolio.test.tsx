import { render, screen, within } from '@testing-library/react'
import { Route, Routes } from 'react-router'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import type { Comparison, Gains, Valuation } from '@/lib/types'
import { Portfolio } from '@/pages/Portfolio'
import { app, failure, json, signedIn } from '@/test/server'

const plain = (text: string | null) => (text ?? '').replace(/\s/g, ' ')

/** The screen where the application puts it: its parts are routes under it. */
const screenAt = (address: string) =>
  app(
    <Routes>
      <Route path="/portfolio/*" element={<Portfolio />} />
    </Routes>,
    address,
  )

const ACCOUNTS = [
  { id: 'a1', kind: 'crypto_wallet', name: 'Exchange', currency: 'USDT', opening_balance: '1000' },
  { id: 'a2', kind: 'bank', name: 'Bank', currency: 'USD', opening_balance: '0' },
  { id: 'a3', kind: 'crypto_wallet', name: 'Cold wallet', currency: 'USD', opening_balance: '0' },
]

const INSTRUMENTS = [
  { id: 'i1', kind: 'crypto', symbol: 'BTC', name: 'Bitcoin' },
  { id: 'i2', kind: 'crypto', symbol: 'ETH', name: 'Ethereum' },
  { id: 'i3', kind: 'fx', symbol: 'EUR', name: 'Euro' },
]

const VALUE: Valuation = {
  on: '2026-10-26',
  at: '2026-10-26T15:00:00Z',
  currency: 'USD',
  accounts: [
    {
      account_id: 'a1',
      name: 'Exchange',
      total: '768.851',
      holdings: [
        { kind: 'cash', symbol: 'USDT', name: '', quantity: '149', price: '0.999', value: '148.851', priced_at: '2026-10-26T00:00:00Z', stale: false },
        { kind: 'position', instrument_id: 'i1', symbol: 'BTC', name: 'Bitcoin', quantity: '0.01', price: '62000', value: '620', priced_at: '2026-10-26T14:55:00Z', stale: false },
        { kind: 'position', instrument_id: 'i2', symbol: 'ETH', name: 'Ethereum', quantity: '0.1', stale: false },
      ],
    },
  ],
  total: '768.851',
  unvalued: ['ETH'],
  stale: false,
}

const BOOKS = {
  '/api/v1/ledger/accounts': json(ACCOUNTS),
  '/api/v1/market/instruments': json(INSTRUMENTS),
}

describe('Portfolio', () => {
  it('shows each account`s money and positions, what they are worth, and what the total leaves out', async () => {
    const requests = signedIn({ ...BOOKS, '/api/v1/portfolio/value': json(VALUE) })
    render(screenAt('/portfolio'))

    const exchange = await screen.findByRole('region', { name: 'Exchange' })
    const rows = within(exchange).getAllByRole('row')
    expect(within(rows[1]!).getByText('USDT')).toBeTruthy()
    expect(within(rows[1]!).getByText('Money')).toBeTruthy()
    expect(within(rows[2]!).getByText('Bitcoin')).toBeTruthy()
    expect(plain(within(rows[2]!).getAllByText(/620/)[0]!.textContent)).toBe('$620.00')
    expect(within(rows[3]!).getByText('No price')).toBeTruthy()
    expect(screen.getByText('Not in the total, for want of a price: ETH')).toBeTruthy()
    // Valued in the currency most accounts are in, until another is chosen.
    expect(requests.find((sent) => sent.path.startsWith('/api/v1/portfolio/value'))?.path).toContain('currency=USD')
  })

  it('records a buy with its moment`s offset, and says what it moves the money by first', async () => {
    const requests = signedIn({
      ...BOOKS,
      '/api/v1/portfolio/trades': json([]),
      'POST /api/v1/portfolio/trades': json({}, 201),
    })
    render(screenAt('/portfolio/trades'))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Record a trade' }))[0]!)

    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Account' }))
    // Only a broker's or a wallet's: a bank account holds no positions.
    expect(screen.queryByRole('option', { name: /Bank/ })).toBeNull()
    await userEvent.click(await screen.findByRole('option', { name: /Exchange/ }))
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Instrument' }))
    // A currency is money, not something to buy here.
    expect(screen.queryByRole('option', { name: /EUR/ })).toBeNull()
    await userEvent.click(await screen.findByRole('option', { name: /BTC/ }))
    await userEvent.type(within(dialog).getByLabelText(/^Quantity/), '0.01')
    await userEvent.type(within(dialog).getByLabelText(/^Price/), '60000')
    await userEvent.type(within(dialog).getByLabelText(/^Fee/), '1')
    expect(plain(within(dialog).getByText(/Moves the money on Exchange by/).textContent)).toContain('-601 USDT')

    await userEvent.click(within(dialog).getByRole('button', { name: 'Record a trade' }))
    const sent = requests.find((request) => request.method === 'POST' && request.path === '/api/v1/portfolio/trades')
    expect(sent?.body).toMatchObject({ kind: 'buy', account_id: 'a1', instrument_id: 'i1', quantity: '0.01', price: '60000', fee: '1', to_account_id: null })
    expect((sent?.body as { occurred_at: string }).occurred_at).toMatch(/T\d\d:\d\d:00[+-]\d\d:\d\d$/)
  })

  it('says why a trade was refused, in the reader`s language', async () => {
    signedIn({
      ...BOOKS,
      '/api/v1/portfolio/trades': json([]),
      'POST /api/v1/portfolio/trades': failure(409, 'that leaves 0.01 BTC fewer', 'trade.oversold', { symbol: 'BTC', short: '0.01', at: '2026-10-20' }),
    })
    render(screenAt('/portfolio/trades'))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Record a trade' }))[0]!)
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('radio', { name: 'Sell' }))
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Account' }))
    await userEvent.click(await screen.findByRole('option', { name: /Exchange/ }))
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Instrument' }))
    await userEvent.click(await screen.findByRole('option', { name: /BTC/ }))
    await userEvent.type(within(dialog).getByLabelText(/^Quantity/), '0.02')
    await userEvent.type(within(dialog).getByLabelText(/^Price/), '61000')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Record a trade' }))
    expect((await within(dialog).findByRole('alert')).textContent).toBe('That would leave 0.01 BTC fewer than were held at the time.')
  })

  it('sets a snapshot beside now, line by line', async () => {
    const comparison: Comparison = {
      snapshot: { ...VALUE, id: 's1', on: '2026-09-01', note: 'Before the summer', taken_at: '2026-09-01T12:00:00Z', total: '600', unvalued: [] },
      against: VALUE,
      differences: [
        {
          account_id: 'a1',
          account_name: 'Exchange',
          kind: 'position',
          instrument_id: 'i1',
          symbol: 'BTC',
          quantity_then: '0.005',
          quantity_now: '0.01',
          value_then: '300',
          value_now: '620',
        },
      ],
    }
    signedIn({ ...BOOKS, '/api/v1/portfolio/snapshots': json([]), '/api/v1/portfolio/snapshots/s1': json(comparison) })
    render(screenAt('/portfolio/snapshots/s1'))

    expect(await screen.findByText('Snapshot of Sep 1, 2026')).toBeTruthy()
    expect(screen.getByText('Before the summer')).toBeTruthy()
    const row = screen.getAllByRole('row').find((each) => within(each).queryByText('BTC'))!
    expect(within(row).getByText('0.005')).toBeTruthy()
    expect(within(row).getByText('0.01')).toBeTruthy()
    expect(plain(within(row).getByText(/320/).textContent)).toBe('+$320.00')
  })
})

describe('Portfolio gains', () => {
  const GAINS: Gains = {
    currency: 'USD',
    from: `${new Date().getFullYear()}-01-01`,
    to: `${new Date().getFullYear()}-12-31`,
    sales: [
      {
        trade_id: 't3',
        account_id: 'a1',
        account_name: 'Exchange',
        instrument_id: 'i1',
        symbol: 'BTC',
        occurred_at: '2026-10-21T13:00:00Z',
        occurred_on: '2026-10-21',
        method: 'fifo',
        quantity: '0.01',
        proceeds: '799.2',
        cost: '601',
        gain: '198.2',
        lots: [],
      },
      {
        trade_id: 't4',
        account_id: 'a1',
        account_name: 'Exchange',
        instrument_id: 'i2',
        symbol: 'ETH',
        occurred_at: '2026-10-22T13:00:00Z',
        occurred_on: '2026-10-22',
        method: 'fifo',
        quantity: '0.1',
        proceeds: '250',
        cost: '300',
        gain: '-50',
        lots: [],
      },
    ],
    proceeds: '1049.2',
    cost: '901',
    gain: '148.2',
    unvalued: [],
  }
  const PORTFOLIO_ACCOUNTS = [{ account_id: 'a1', name: 'Exchange', currency: 'USDT', closed: false, cost_method: 'fifo' }]
  const LOTS = [
    {
      account_id: 'a1',
      instrument_id: 'i1',
      symbol: 'BTC',
      method: 'fifo',
      quantity: '0.01',
      lots: [{ trade_id: 't2', acquired_at: '2026-10-20T13:00:00Z', acquired_on: '2026-10-20', quantity: '0.01', cost: '701', unit_cost: '70100', currency: 'USDT' }],
    },
  ]

  it('says what each sale gained - a loss with its sign - and counts an account another way when told', async () => {
    const requests = signedIn({
      ...BOOKS,
      '/api/v1/portfolio/trades': json([]),
      '/api/v1/portfolio/gains': json(GAINS),
      '/api/v1/portfolio/accounts': json(PORTFOLIO_ACCOUNTS),
      '/api/v1/portfolio/lots': json(LOTS),
      'PUT /api/v1/portfolio/accounts/a1': json({ ...PORTFOLIO_ACCOUNTS[0], cost_method: 'average' }),
    })
    render(screenAt('/portfolio/gains'))

    const sales = await screen.findByRole('region', { name: 'Sales' })
    const rows = within(sales).getAllByRole('row')
    expect(plain(within(rows[1]!).getAllByText(/198/)[0]!.textContent)).toBe('+$198.20')
    // A loss says so in its sign, not only in its colour.
    expect(plain(within(rows[2]!).getAllByText(/50/).at(-1)!.textContent)).toBe('-$50.00')
    expect(requests.find((sent) => sent.path.startsWith('/api/v1/portfolio/gains'))?.path).toContain(`from=${GAINS.from}`)

    const lots = await screen.findByRole('region', { name: 'Lots held' })
    expect(within(lots).getByText('BTC')).toBeTruthy()

    await userEvent.click(screen.getByRole('combobox', { name: 'How Exchange counts' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Average cost' }))
    const sent = requests.find((request) => request.method === 'PUT')
    expect(sent).toMatchObject({ path: '/api/v1/portfolio/accounts/a1', body: { cost_method: 'average' } })
  })

  it('receives units with no fee, and leaves their worth to the market when none is said', async () => {
    const requests = signedIn({
      ...BOOKS,
      '/api/v1/portfolio/trades': json([]),
      'POST /api/v1/portfolio/trades': json({}, 201),
    })
    render(screenAt('/portfolio/trades'))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Record a trade' }))[0]!)
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('radio', { name: 'Receive' }))
    expect(within(dialog).queryByLabelText(/^Fee/)).toBeNull()
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Account' }))
    await userEvent.click(await screen.findByRole('option', { name: /Exchange/ }))
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Instrument' }))
    await userEvent.click(await screen.findByRole('option', { name: /ETH/ }))
    await userEvent.type(within(dialog).getByLabelText(/^Quantity/), '0.05')
    expect(within(dialog).getByText(/Units received move no money/)).toBeTruthy()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Record a trade' }))
    const sent = requests.find((request) => request.method === 'POST' && request.path === '/api/v1/portfolio/trades')
    expect(sent?.body).toMatchObject({ kind: 'receive', quantity: '0.05', price: null, fee: null })
  })
})
