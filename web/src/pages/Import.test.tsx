import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import type { Account, Category, ImportDocument, Receipt } from '@/lib/types'
import { Import } from '@/pages/Import'
import { app, json, signedIn, type Sent } from '@/test/server'

const RECEIPT: Receipt = {
  code: '01800999991001001000012322026101810000000011',
  kind: 'invoice',
  number: '001-001-0000123',
  issued_at: '2026-10-18T16:12:43',
  issuer: { name: 'SOL ALIMENTOS S.A.', trade_name: 'SUPERMERCADO SOL', tax_id: '80099999-1', city: 'ASUNCION (DISTRITO)' },
  currency: 'PYG',
  items: [
    { description: 'LECHE ENTERA 1LT', quantity: '2', total: '17000' },
    { description: 'PAN FELIPE X KG', quantity: '0.534', unit: 'KG', total: '6408' },
  ],
  total: '23408',
  on_credit: false,
  payments: [{ means: 'debit_card', amount: '23408', currency: 'PYG', card_digits: '4321' }],
}

const WAITING: ImportDocument = {
  id: 'd1',
  name: 'factura.xml',
  size: 4096,
  origin: 'folder',
  state: 'waiting',
  receipt: RECEIPT,
  shop: 'Supermercado Sol',
  account_id: 'a1',
  needs: ['category'],
  received_at: '2026-10-19T10:00:00Z',
}

const BOOKED: ImportDocument = {
  id: 'd2',
  name: 'farmacia.xml',
  size: 2048,
  origin: 'upload',
  state: 'booked',
  receipt: { ...RECEIPT, issuer: { name: 'FARMACIA CENTRAL S.R.L.' }, total: '58000' },
  needs: [],
  entry_id: 'e9',
  received_at: '2026-10-19T09:00:00Z',
  booked_at: '2026-10-19T09:00:01Z',
}

const REFUSED: ImportDocument = {
  id: 'd3',
  name: 'pedido.xml',
  size: 120,
  origin: 'upload',
  state: 'refused',
  needs: [],
  reason: { code: 'document.not_a_receipt', params: {}, message: 'this XML document is not an electronic document issued through SIFEN' },
  received_at: '2026-10-19T08:00:00Z',
}

const ACCOUNTS: Account[] = [{ id: 'a1', kind: 'card', name: 'Visa Debit', currency: 'PYG', opening_balance: '0' }]
const CATEGORIES: Category[] = [{ id: 'c1', parent_id: null, flow: 'expense', name: 'Groceries' }]

const BOOKS = {
  '/api/v1/imports/documents': json([WAITING, BOOKED, REFUSED]),
  '/api/v1/ledger/accounts': json(ACCOUNTS),
  '/api/v1/ledger/categories': json(CATEGORIES),
}

const sent = (requests: Sent[], method: string, path: string) => requests.filter((each) => each.method === method && each.path.startsWith(path))

describe('Import', () => {
  it('lists what waits apart from what is done, and says why each one is where it is', async () => {
    signedIn(BOOKS)
    render(app(<Import />, '/import'))
    const waiting = await screen.findByRole('region', { name: 'Waiting' })
    // As the books will name it, not in the till's capitals.
    expect(within(waiting).getByText('Supermercado Sol')).toBeTruthy()
    expect(within(waiting).getByText(/2 items · from the folder/)).toBeTruthy()
    expect(within(waiting).getByText(/23,408/)).toBeTruthy()
    // Only what it waits for is asked - and it is.
    expect(within(waiting).getByText('What was it for?')).toBeTruthy()

    const recent = screen.getByRole('region', { name: 'Recent' })
    expect(within(recent).getByText('FARMACIA CENTRAL S.R.L.')).toBeTruthy()
    expect(within(recent).getByText('In the books')).toBeTruthy()
    // The reason is said in the interface's words, not the server's.
    expect(within(recent).getByText('This XML file is not a SIFEN receipt.')).toBeTruthy()
  })

  it('books a waiting receipt with what it waited for, and keeps what was said before', async () => {
    const requests = signedIn({
      ...BOOKS,
      'POST /api/v1/imports/documents/d1/book': json({ ...WAITING, state: 'booked', needs: [], entry_id: 'e1' }),
    })
    render(app(<Import />, '/import'))
    const book = await screen.findByRole('button', { name: 'Book Supermercado Sol' })
    // Nothing is booked before what it waits for is said.
    expect((book as HTMLButtonElement).disabled).toBe(true)

    await userEvent.type(screen.getByRole('combobox', { name: /What was it for/ }), 'Groc')
    await userEvent.click(await screen.findByRole('option', { name: 'Groceries' }))
    await userEvent.click(book)
    await screen.findByText('In the books: Supermercado Sol.')
    expect(sent(requests, 'POST', '/api/v1/imports/documents/d1/book')[0]?.body).toEqual({ account_id: 'a1', category_id: 'c1' })
  })

  it('reads the chosen files and says what they came to', async () => {
    const requests = signedIn({
      ...BOOKS,
      'POST /api/v1/imports/documents': (_, url) =>
        Response.json(
          url.searchParams.get('name') === 'uno.xml'
            ? { already: false, document: { ...BOOKED, id: 'n1' } }
            : { already: false, document: { ...REFUSED, id: 'n2' } },
          { status: 201 },
        ),
    })
    render(app(<Import />, '/import'))
    await screen.findByRole('region', { name: 'Waiting' })
    await userEvent.upload(screen.getByTestId('import-files'), [
      new File(['<?xml version="1.0"?><rDE/>'], 'uno.xml', { type: 'text/xml' }),
      new File(['<?xml version="1.0"?><order/>'], 'dos.xml', { type: 'text/xml' }),
    ])
    await screen.findByText('1 booked, 0 waiting, 1 not read.')
    const uploads = sent(requests, 'POST', '/api/v1/imports/documents?')
    expect(uploads.map((each) => each.path)).toEqual(['/api/v1/imports/documents?name=uno.xml', '/api/v1/imports/documents?name=dos.xml'])
  })

  it('dismisses a document without touching the entry it became', async () => {
    const requests = signedIn({ ...BOOKS, 'DELETE /api/v1/imports/documents/d2': () => new Response(null, { status: 204 }) })
    render(app(<Import />, '/import'))
    await userEvent.click(await screen.findByRole('button', { name: 'Dismiss FARMACIA CENTRAL S.R.L.' }))
    await screen.findByText('Dismissed.')
    await waitFor(() => expect(sent(requests, 'DELETE', '/api/v1/imports/documents/d2')).toHaveLength(1))
    expect(requests.some((each) => each.path.startsWith('/api/v1/ledger/entries'))).toBe(false)
  })

  it('says what to do when nothing has been imported yet', async () => {
    signedIn({ ...BOOKS, '/api/v1/imports/documents': json([]) })
    render(app(<Import />, '/import'))
    expect(await screen.findByText('Nothing imported yet')).toBeTruthy()
  })
})
