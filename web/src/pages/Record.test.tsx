import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { addDays } from '@/components/ui/calendar-math'
import { today } from '@/lib/dates'
import type { Entry } from '@/lib/types'
import { Record } from '@/pages/Record'
import { app, failure, json, signedIn, type Sent } from '@/test/server'

const plain = (text: string | null) => (text ?? '').replace(/\s/g, ' ')

const NOW = today()
const YESTERDAY = addDays(NOW, -1)

const entry = (id: string, description: string, on: string, amount = '15000'): Entry => ({
  id,
  occurred_on: on,
  cleared_on: on,
  status: 'cleared',
  description,
  tags: [],
  lines: [
    { id: `${id}a`, side: 'account', account_id: 'a1', amount: `-${amount}`, currency: 'PYG', note: '' },
    { id: `${id}c`, side: 'category', category_id: 'c1', amount, currency: 'PYG', note: '' },
  ],
  attachments: [],
})

const BOOKS = {
  '/api/v1/ledger/accounts': json([
    { id: 'a1', kind: 'cash', name: 'Wallet', currency: 'PYG', opening_balance: '0' },
    { id: 'a2', kind: 'card', name: 'Card', currency: 'PYG', opening_balance: '0' },
  ]),
  '/api/v1/ledger/categories': json([{ id: 'c1', flow: 'expense', name: 'Food' }]),
  '/api/v1/ledger/templates': json([
    { id: 't1', name: 'Coffee', account_id: 'a1', category_id: 'c1', flow: 'expense', amount: '15000', currency: 'PYG', description: 'coffee', tags: [], pending: false },
    { id: 't2', name: 'Superseis', account_id: 'a2', category_id: 'c1', flow: 'expense', currency: 'PYG', description: '', tags: [], pending: false },
  ]),
  // Today's list and the day before's are one path asked two ways.
  '/api/v1/ledger/entries': (_: unknown, url: URL) =>
    Response.json(url.searchParams.get('from') === NOW ? [entry('e1', 'Lunch', NOW)] : [entry('e2', 'Bus', YESTERDAY, '4000')]),
  'POST /api/v1/ledger/entries/quick/preview': failure(400, 'not yet'),
}

const RECORDED = (description: string) => json({ entry: entry('new', description, NOW) }, 201)

const posted = (requests: Sent[], path: string) => requests.filter((sent) => sent.method === 'POST' && sent.path === path)

describe('Record', () => {
  beforeEach(() => localStorage.clear())

  it('records a typed line against the account and the day chosen beside it', async () => {
    const requests = signedIn({ ...BOOKS, 'POST /api/v1/ledger/entries/quick': RECORDED('lunch') })
    render(app(<Record />, '/record'))
    await userEvent.click(await screen.findByRole('button', { name: 'Card' }))
    await userEvent.click(screen.getByRole('button', { name: 'Yesterday' }))
    await userEvent.type(screen.getByLabelText('Record an entry in one line'), '45000 food lunch{Enter}')
    await screen.findByText('Recorded: lunch')
    expect(posted(requests, '/api/v1/ledger/entries/quick')[0]?.body).toEqual({
      text: '45000 food lunch',
      occurred_on: YESTERDAY,
      account_id: 'a2',
      place: null,
    })
    // The account is remembered for the next visit from this browser.
    expect(localStorage.getItem('austeris.record.account')).toBe('a2')
  })

  it('chooses no account of several until one is chosen, rather than guessing', async () => {
    const requests = signedIn({ ...BOOKS, 'POST /api/v1/ledger/entries/quick': RECORDED('lunch') })
    render(app(<Record />, '/record'))
    await userEvent.type(await screen.findByLabelText('Record an entry in one line'), '45000 food{Enter}')
    await screen.findByText('Recorded: lunch')
    expect(posted(requests, '/api/v1/ledger/entries/quick')[0]?.body).toMatchObject({ account_id: null })
    for (const name of ['Wallet', 'Card']) {
      expect(screen.getByRole('button', { name }).getAttribute('aria-pressed')).toBe('false')
    }
  })

  it('shows what a line would record before it is recorded', async () => {
    signedIn({
      ...BOOKS,
      'POST /api/v1/ledger/entries/quick/preview': json({
        occurred_on: NOW,
        flow: 'expense',
        account: { id: 'a1', kind: 'cash', name: 'Wallet', currency: 'PYG', opening_balance: '0' },
        category: { id: 'c1', flow: 'expense', name: 'Food' },
        amount: { amount: '45000', currency: 'PYG' },
        moves: '-45000',
        description: 'lunch',
        counterparty: { name: 'Casa Rica' },
        tags: ['trip'],
        new_tags: ['trip'],
        pending: false,
      }),
    })
    render(app(<Record />, '/record'))
    await userEvent.type(await screen.findByLabelText('Record an entry in one line'), '45000 food lunch @casarica #trip')
    const preview = await screen.findByRole('status', { name: 'What this line records' })
    expect(plain(preview.textContent)).toContain('-₲45,000')
    expect(preview.textContent).toContain('from Wallet')
    expect(preview.textContent).toContain('@Casa Rica')
    // Both the counterparty and the tag would be created: each says so.
    expect(within(preview).getAllByText('new')).toHaveLength(2)
  })

  it('says why a line half typed cannot be recorded yet, in the interface language', async () => {
    signedIn({
      ...BOOKS,
      'POST /api/v1/ledger/entries/quick/preview': failure(400, 'server words', 'line.unknown_category', { name: 'yachts' }),
    })
    render(app(<Record />, '/record'))
    await userEvent.type(await screen.findByLabelText('Record an entry in one line'), '45000 yachts')
    expect(await screen.findByText('You have no category called “yachts”; create it first.')).toBeTruthy()
  })

  it('records a button that keeps its amount with one press, and takes it back with Undo', async () => {
    const requests = signedIn({
      ...BOOKS,
      'POST /api/v1/ledger/templates/t1/use': RECORDED('coffee'),
      'DELETE /api/v1/ledger/entries/new': () => new Response(null, { status: 204 }),
    })
    render(app(<Record />, '/record'))
    await userEvent.click(await screen.findByRole('button', { name: /^Coffee/ }))
    await screen.findByText('Recorded: Coffee')
    expect(posted(requests, '/api/v1/ledger/templates/t1/use')[0]?.body).toEqual({ occurred_on: NOW, amount: null, allow_duplicate: false })

    await userEvent.click(screen.getByRole('button', { name: 'Undo' }))
    await screen.findByText('Taken back.')
    expect(requests.some((sent) => sent.method === 'DELETE' && sent.path === '/api/v1/ledger/entries/new')).toBe(true)
    // The toast that offered it is gone with it: a second Undo would delete
    // what is no longer there.
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()
  })

  it('asks for the amount a button does not keep, and sends what was typed', async () => {
    const requests = signedIn({ ...BOOKS, 'POST /api/v1/ledger/templates/t2/use': RECORDED('') })
    render(app(<Record />, '/record'))
    await userEvent.click(await screen.findByRole('button', { name: /^Superseis/ }))
    await userEvent.type(await screen.findByLabelText('How much, in PYG'), '231.500{Enter}')
    await screen.findByText('Recorded: Superseis')
    expect(posted(requests, '/api/v1/ledger/templates/t2/use')[0]?.body).toMatchObject({ amount: '231500' })
  })

  it('asks about a second press, and records a second one only when told to', async () => {
    let presses = 0
    const requests = signedIn({
      ...BOOKS,
      'POST /api/v1/ledger/templates/t1/use': (body) => {
        presses += 1
        return (body as { allow_duplicate: boolean }).allow_duplicate
          ? Response.json({ entry: entry('second', 'coffee', NOW) }, { status: 201 })
          : Response.json(
              { status: 409, error: 'Conflict', message: 'the same entry', code: 'entry.possible_duplicate', params: { entry_id: 'e1', seconds: 4 } },
              { status: 409 },
            )
      },
    })
    render(app(<Record />, '/record'))
    await userEvent.click(await screen.findByRole('button', { name: /^Coffee/ }))
    const question = await screen.findByRole('alertdialog')
    expect(question.textContent).toContain('The same entry was recorded 4 s ago.')
    await userEvent.click(within(question).getByRole('button', { name: 'Record a second' }))
    await screen.findByText('Recorded: Coffee')
    expect(presses).toBe(2)
    expect(posted(requests, '/api/v1/ledger/templates/t1/use')[1]?.body).toMatchObject({ allow_duplicate: true })
  })

  it('lists today and the day before, and records the day before again on the day chosen', async () => {
    const requests = signedIn({ ...BOOKS, 'POST /api/v1/ledger/entries/e2/repeat': RECORDED('Bus') })
    render(app(<Record />, '/record'))
    expect(await screen.findByRole('region', { name: 'Today' })).toBeTruthy()
    const yesterday = await screen.findByRole('region', { name: 'Yesterday' })
    await userEvent.click(within(yesterday).getByRole('button', { name: 'Record “Bus” again' }))
    await screen.findByText('Recorded again: Bus')
    expect(posted(requests, '/api/v1/ledger/entries/e2/repeat')[0]?.body).toEqual({ occurred_on: NOW, allow_duplicate: false })
  })

  it('keeps the buttons in the order chosen', async () => {
    const requests = signedIn({ ...BOOKS, 'PUT /api/v1/ledger/templates/order': () => new Response(null, { status: 204 }) })
    render(app(<Record />, '/record'))
    await userEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Move “Superseis” up' }))
    await waitFor(() => expect(requests.find((sent) => sent.method === 'PUT')?.body).toEqual({ ids: ['t2', 't1'] }))
  })
})
