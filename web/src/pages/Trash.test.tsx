import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import type { Entry, Trash as TrashBody } from '@/lib/types'
import { Trash } from '@/pages/Trash'
import { app, json, signedIn, type Sent } from '@/test/server'

const lunch: Entry = {
  id: 'e1',
  occurred_on: '2026-10-01',
  cleared_on: '2026-10-01',
  status: 'cleared',
  kept: false,
  description: 'Weekly shop',
  tags: [],
  lines: [
    { id: 'l1', side: 'account', account_id: 'a1', amount: '-45000', currency: 'PYG', note: '' },
    { id: 'l2', side: 'category', category_id: 'c1', amount: '45000', currency: 'PYG', note: '' },
  ],
  attachments: [{ id: 'f1', name: 'receipt.jpg', media_type: 'image/jpeg', size: 2048, created_at: '2026-10-01T12:00:00Z' }],
}

const TRASH: TrashBody = {
  keep_days: 30,
  entries: [{ entry: lunch, deleted_at: '2026-10-14T12:00:00Z', who: { kind: 'person', person_id: 'u1' }, erased_after: '2026-11-13T12:00:00Z' }],
  attachments: [
    {
      attachment: { id: 'f2', name: 'scan.pdf', media_type: 'application/pdf', size: 340 * 1024, created_at: '2026-10-02T12:00:00Z' },
      entry_id: 'e2',
      entry_title: 'Pharmacy',
      entry_occurred_on: '2026-10-02',
      removed_at: '2026-10-14T13:00:00Z',
      erased_after: '2026-11-13T13:00:00Z',
    },
  ],
}

const sent = (requests: Sent[], method: string, path: string) => requests.filter((each) => each.method === method && each.path === path)

describe('Trash', () => {
  it('lists what was deleted, how long it waits, and the money it moved', async () => {
    signedIn({ '/api/v1/ledger/trash': json(TRASH) })
    render(app(<Trash />, '/entries/trash'))
    expect(await screen.findByText('Weekly shop')).toBeTruthy()
    // The server says how long it keeps things; the page does not promise its own.
    expect(screen.getByText('Deleted entries and files wait here for 30 days, then they are erased for good.')).toBeTruthy()
    const row = screen.getByText('Weekly shop').closest('li')!
    expect(within(row).getByText(/1 file/)).toBeTruthy()
    expect(within(row).getByText(/erased Nov 13, 2026/)).toBeTruthy()
    expect(within(row).getByText(/45,000/)).toBeTruthy()

    const file = screen.getByRole('link', { name: 'scan.pdf' })
    expect(file.getAttribute('href')).toBe('/api/v1/ledger/attachments/f2/content')
    expect(screen.getByText(/From Pharmacy, Oct 2, 2026 · 340 kB/)).toBeTruthy()
  })

  it('restores an entry with one press', async () => {
    const requests = signedIn({
      '/api/v1/ledger/trash': json(TRASH),
      'POST /api/v1/ledger/trash/entries/e1/restore': json(lunch),
    })
    render(app(<Trash />, '/entries/trash'))
    await userEvent.click(await screen.findByRole('button', { name: 'Restore Weekly shop' }))
    await screen.findByText('Restored.')
    expect(sent(requests, 'POST', '/api/v1/ledger/trash/entries/e1/restore')).toHaveLength(1)
  })

  it('asks before erasing, and erases only what was asked about', async () => {
    const requests = signedIn({
      '/api/v1/ledger/trash': json(TRASH),
      'DELETE /api/v1/ledger/trash/attachments/f2': () => new Response(null, { status: 204 }),
    })
    render(app(<Trash />, '/entries/trash'))
    await userEvent.click(await screen.findByRole('button', { name: 'Erase scan.pdf' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('Nothing brings the file back afterwards.')).toBeTruthy()
    expect(sent(requests, 'DELETE', '/api/v1/ledger/trash/attachments/f2')).toHaveLength(0)
    await userEvent.click(within(dialog).getByRole('button', { name: 'Erase' }))
    await screen.findByText('Erased.')
    expect(sent(requests, 'DELETE', '/api/v1/ledger/trash/attachments/f2')).toHaveLength(1)
    expect(sent(requests, 'DELETE', '/api/v1/ledger/trash/entries/e1')).toHaveLength(0)
  })

  it('empties the trash after asking', async () => {
    const requests = signedIn({
      '/api/v1/ledger/trash': json(TRASH),
      'DELETE /api/v1/ledger/trash': () => new Response(null, { status: 204 }),
    })
    render(app(<Trash />, '/entries/trash'))
    await userEvent.click(await screen.findByRole('button', { name: 'Empty the trash' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Empty the trash' }))
    await waitFor(() => expect(sent(requests, 'DELETE', '/api/v1/ledger/trash')).toHaveLength(1))
  })

  it('says so when there is nothing in it, and has nothing to empty', async () => {
    signedIn({ '/api/v1/ledger/trash': json({ keep_days: 30, entries: [], attachments: [] }) })
    render(app(<Trash />, '/entries/trash'))
    expect(await screen.findByText('The trash is empty')).toBeTruthy()
    expect(screen.getByText('A deleted entry or file waits here for 30 days before it is erased.')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Empty the trash' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
