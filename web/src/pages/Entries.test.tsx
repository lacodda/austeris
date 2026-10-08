import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { Entries } from '@/pages/Entries'
import { app, failure, json, signedIn } from '@/test/server'

const plain = (text: string | null) => (text ?? '').replace(/\s/g, ' ')

const LISTS = {
  '/api/v1/ledger/accounts': json([{ id: 'a1', kind: 'cash', name: 'Wallet', currency: 'PYG', opening_balance: '0' }]),
  '/api/v1/ledger/categories': json([
    { id: 'c1', flow: 'expense', name: 'Food' },
    { id: 'c2', flow: 'expense', name: 'Groceries', parent_id: 'c1' },
  ]),
  '/api/v1/ledger/tags': json([{ id: 't1', name: 'trip' }]),
  '/api/v1/ledger/counterparties': json([{ id: 'p1', name: 'Casa Rica', key: 'casarica' }]),
  '/api/v1/ledger/places': json([]),
}

const HELD = {
  id: 'e1',
  occurred_on: '2026-10-07',
  status: 'pending',
  description: 'Weekly shop',
  tags: ['trip'],
  counterparty: { id: 'p1', name: 'Casa Rica' },
  place: { id: 'pl1', country: 'PY', city: 'Asunción' },
  lines: [
    { id: 'l1', side: 'account', account_id: 'a1', amount: '-45000.000000000000000000', currency: 'PYG', note: '' },
    { id: 'l2', side: 'category', category_id: 'c2', amount: '45000.000000000000000000', currency: 'PYG', note: '' },
  ],
}

describe('Entries', () => {
  it('lists an entry with who, where, its category path and the money it moved', async () => {
    signedIn({ ...LISTS, '/api/v1/ledger/entries': json([HELD]) })
    render(app(<Entries />, '/entries'))
    const row = (await screen.findByRole('button', { name: 'Weekly shop' })).closest('tr')!
    expect(within(row).getByText('Held')).toBeTruthy()
    expect(screen.getAllByText('Food › Groceries').length).toBeGreaterThan(0)
    expect(screen.getByText('Asunción, Paraguay · #trip')).toBeTruthy()
    expect(plain(screen.getByText(/45,000/).textContent)).toBe('-₲45,000')
  })

  it('asks the server for what the address filters by', async () => {
    const requests = signedIn({ ...LISTS, '/api/v1/ledger/entries': json([]) })
    render(app(<Entries />, '/entries?account=a1&tag=trip&status=pending'))
    expect(await screen.findByText('No entries match these filters.')).toBeTruthy()
    const asked = requests.find((sent) => sent.path.startsWith('/api/v1/ledger/entries'))!.path
    expect(asked).toContain('account=a1')
    expect(asked).toContain('tag=trip')
    expect(asked).toContain('status=pending')
  })

  it('knows there is more only when the server sent more than a page', async () => {
    const page = Array.from({ length: 51 }, (_, index) => ({ ...HELD, id: `e${index}`, description: `Entry ${index}` }))
    signedIn({ ...LISTS, '/api/v1/ledger/entries': json(page) })
    render(app(<Entries />, '/entries'))
    expect(await screen.findByRole('button', { name: 'Entry 0' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Entry 50' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Show more' })).toBeTruthy()
  })

  it('records a typed line and clears it', async () => {
    const requests = signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([]),
      'POST /api/v1/ledger/entries/quick': json({ entry: { ...HELD, status: 'cleared' }, new_tags: ['trip'] }, 201),
    })
    render(app(<Entries />, '/entries'))
    const line = await screen.findByLabelText('Record an entry in one line')
    await userEvent.type(line, '45000 food lunch #trip{Enter}')
    expect(requests.find((sent) => sent.method === 'POST')?.body).toEqual({ text: '45000 food lunch #trip', occurred_on: null, place: null })
    await screen.findByText('Recorded: Weekly shop')
    expect((line as HTMLInputElement).value).toBe('')
  })

  it('keeps a refused line, with the server saying why', async () => {
    signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([]),
      'POST /api/v1/ledger/entries/quick': failure(404, 'you have no category called `fod`; create it first'),
    })
    render(app(<Entries />, '/entries'))
    const line = await screen.findByLabelText('Record an entry in one line')
    await userEvent.type(line, '45000 fod lunch{Enter}')
    expect(await screen.findByText('you have no category called `fod`; create it first')).toBeTruthy()
    expect((line as HTMLInputElement).value).toBe('45000 fod lunch')
  })
})
