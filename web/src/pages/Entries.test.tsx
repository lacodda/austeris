import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { today } from '@/lib/dates'
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
  // Typing works the line out as it goes; these tests are about recording it.
  'POST /api/v1/ledger/entries/quick/preview': failure(400, 'not yet'),
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
  attachments: [],
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
    // On the person's day, not the server's.
    expect(requests.find((sent) => sent.path === '/api/v1/ledger/entries/quick')?.body).toEqual({
      text: '45000 food lunch #trip',
      occurred_on: today(),
      account_id: null,
      place: null,
    })
    await screen.findByText('Recorded: Weekly shop')
    expect((line as HTMLInputElement).value).toBe('')
  })

  it('records a line that names no account against the account the list is narrowed to', async () => {
    const requests = signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([]),
      'POST /api/v1/ledger/entries/quick': json({ entry: { ...HELD, status: 'cleared' } }, 201),
    })
    render(app(<Entries />, '/entries?account=a1'))
    await userEvent.type(await screen.findByLabelText('Record an entry in one line'), '45000 food{Enter}')
    await screen.findByText('Recorded: Weekly shop')
    expect(requests.find((sent) => sent.path === '/api/v1/ledger/entries/quick')?.body).toMatchObject({ account_id: 'a1' })
  })

  it('shows on the card what happened to the entry and the files kept with it', async () => {
    const posted = { ...HELD, status: 'cleared', cleared_on: '2026-10-09' }
    signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([HELD]),
      '/api/v1/ledger/entries/e1': json({
        ...posted,
        attachments: [{ id: 'f1', name: 'receipt.jpg', media_type: 'image/jpeg', size: 2048, created_at: '2026-10-07T12:00:00Z' }],
      }),
      '/api/v1/ledger/entries/e1/history': json([
        { id: 1, at: '2026-10-07T12:00:00Z', action: 'created', who: { kind: 'person', person_id: 'u1' }, changed: [], after: HELD },
        {
          id: 2,
          at: '2026-10-09T12:00:00Z',
          action: 'changed',
          who: { kind: 'module', module: 'demo' },
          changed: ['description', 'status'],
          before: HELD,
          after: { ...posted, description: 'Groceries' },
        },
      ]),
    })
    render(app(<Entries />, '/entries'))
    await userEvent.click(await screen.findByRole('button', { name: 'Weekly shop' }))

    const files = await screen.findByRole('region', { name: 'Files' })
    expect(within(files).getByRole('link', { name: /receipt\.jpg/ }).getAttribute('href')).toBe('/api/v1/ledger/attachments/f1/content')
    const history = await screen.findByRole('region', { name: 'History' })
    expect(within(history).getByText('Recorded · you')).toBeTruthy()
    expect(within(history).getByText('Changed · the Demo module')).toBeTruthy()
    expect(within(history).getByText('Description: "Weekly shop" → "Groceries"')).toBeTruthy()
    expect(within(history).getByText('Status: held → posted on Oct 9, 2026')).toBeTruthy()
  })

  it('shows a trade`s money as kept by the portfolio, with nothing to delete or repeat', async () => {
    const trade = {
      ...HELD,
      id: 'e7',
      status: 'cleared',
      cleared_on: '2026-10-07',
      description: 'Buy 0.01 BTC',
      source: 'portfolio',
      kept: true,
      counterparty: null,
      place: null,
      tags: [],
      lines: [
        { id: 'l1', side: 'account', account_id: 'a1', amount: '-601', currency: 'PYG', note: '' },
        { id: 'l2', side: 'holding', amount: '601', currency: 'PYG', note: '' },
      ],
    }
    signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([trade]),
      '/api/v1/ledger/entries/e7': json(trade),
      '/api/v1/ledger/entries/e7/history': json([]),
    })
    render(app(<Entries />, '/entries'))
    await userEvent.click(await screen.findByRole('button', { name: 'Buy 0.01 BTC' }))

    expect(await screen.findByText(/This is a trade's money, kept by Portfolio/)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Open the trades' }).getAttribute('href')).toBe('/portfolio/trades')
    expect(screen.getByText('Holdings')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Record again today' })).toBeNull()
    // What is the person's to change on it: not its day, not its name.
    await userEvent.click(screen.getByRole('button', { name: 'Edit' }))
    expect(await screen.findByLabelText(/^Tags/)).toBeTruthy()
    expect(screen.queryByLabelText(/^Description/)).toBeNull()
  })

  it('deletes to the trash, and Undo on the toast brings the entry back', async () => {
    const requests = signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([HELD]),
      '/api/v1/ledger/entries/e1': json(HELD),
      '/api/v1/ledger/entries/e1/history': json([]),
      'DELETE /api/v1/ledger/entries/e1': () => new Response(null, { status: 204 }),
      'POST /api/v1/ledger/trash/entries/e1/restore': json(HELD),
    })
    render(app(<Entries />, '/entries'))
    await userEvent.click(await screen.findByRole('button', { name: 'Weekly shop' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Delete' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('It goes to the trash with its files, and can be brought back from there as it was.')).toBeTruthy()
    await userEvent.click(within(dialog).getByRole('button', { name: 'Delete' }))

    await screen.findByText('Moved to the trash.')
    await userEvent.click(screen.getByRole('button', { name: 'Undo' }))
    await waitFor(() =>
      expect(requests.filter((sent) => sent.method === 'POST' && sent.path === '/api/v1/ledger/trash/entries/e1/restore')).toHaveLength(1),
    )
  })

  it('attaches a file under its own name, and takes one off to the trash', async () => {
    const withFile = {
      ...HELD,
      attachments: [{ id: 'f1', name: 'old.pdf', media_type: 'application/pdf', size: 340 * 1024, created_at: '2026-10-07T12:00:00Z' }],
    }
    const requests = signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([HELD]),
      '/api/v1/ledger/entries/e1': json(withFile),
      '/api/v1/ledger/entries/e1/history': json([]),
      'POST /api/v1/ledger/entries/e1/attachments': json({ id: 'f2', name: 'чек.jpg', media_type: 'image/jpeg', size: 4, created_at: '2026-10-09T12:00:00Z' }, 201),
      'DELETE /api/v1/ledger/attachments/f1': () => new Response(null, { status: 204 }),
      'POST /api/v1/ledger/trash/attachments/f1/restore': json(withFile.attachments[0]),
    })
    render(app(<Entries />, '/entries'))
    await userEvent.click(await screen.findByRole('button', { name: 'Weekly shop' }))
    await screen.findByRole('region', { name: 'Files' })

    await userEvent.upload(screen.getByTestId('attach-files'), new File(['\xFF\xD8\xFF\xE0'], 'чек.jpg', { type: 'image/jpeg' }))
    await waitFor(() =>
      expect(requests.some((sent) => sent.method === 'POST' && sent.path === `/api/v1/ledger/entries/e1/attachments?name=${encodeURIComponent('чек.jpg')}`)).toBe(
        true,
      ),
    )

    await userEvent.click(screen.getByRole('button', { name: 'Remove old.pdf' }))
    await screen.findByText('old.pdf moved to the trash.')
    await userEvent.click(screen.getByRole('button', { name: 'Undo' }))
    await waitFor(() => expect(requests.some((sent) => sent.path === '/api/v1/ledger/trash/attachments/f1/restore')).toBe(true))
  })

  it('says why a file was refused, in the reader\'s language', async () => {
    signedIn({
      ...LISTS,
      '/api/v1/ledger/entries': json([HELD]),
      '/api/v1/ledger/entries/e1': json(HELD),
      '/api/v1/ledger/entries/e1/history': json([]),
      'POST /api/v1/ledger/entries/e1/attachments': failure(413, 'a file can be 10 MB at most', 'attachment.too_large', { limit_mb: 10 }),
    })
    render(app(<Entries />, '/entries'))
    await userEvent.click(await screen.findByRole('button', { name: 'Weekly shop' }))
    await screen.findByRole('region', { name: 'Files' })
    await userEvent.upload(screen.getByTestId('attach-files'), new File(['x'], 'huge.jpg', { type: 'image/jpeg' }))
    expect(await screen.findByText('A file can be up to 10 MB.')).toBeTruthy()
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
