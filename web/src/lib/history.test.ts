import { describe, expect, it } from 'vitest'
import { formatSize, isPicture } from '@/lib/attachments'
import { changes } from '@/lib/history'
import type { Entry, Revision } from '@/lib/types'

const lunch: Entry = {
  id: 'e1',
  occurred_on: '2026-10-01',
  cleared_on: '2026-10-01',
  status: 'cleared',
  kept: false,
  description: 'lunch',
  tags: ['work'],
  lines: [
    { id: 'l1', side: 'account', account_id: 'a1', amount: '-45000', currency: 'PYG', note: '' },
    { id: 'l2', side: 'category', category_id: 'c1', amount: '45000', currency: 'PYG', note: '' },
  ],
  attachments: [],
}

const revision = (changed: Revision['changed'], before: Entry, after: Entry): Revision => ({
  id: 2,
  at: '2026-10-02T12:00:00Z',
  action: 'changed',
  who: { kind: 'person', person_id: 'u1' },
  changed,
  before,
  after,
})

describe('changes', () => {
  it('says which tags went on and which came off', () => {
    const after = { ...lunch, tags: ['trip', 'work-trip'] }
    expect(changes(revision(['tags'], { ...lunch, tags: ['work', 'trip'] }, after))).toEqual([
      { field: 'tags', added: ['work-trip'], removed: ['work'] },
    ])
  })

  it('says what the account moved by, when one account moved', () => {
    const after = {
      ...lunch,
      lines: [
        { ...lunch.lines[0]!, id: 'l3', amount: '-50000' },
        { ...lunch.lines[1]!, id: 'l4', amount: '50000' },
      ],
    }
    expect(changes(revision(['lines'], lunch, after))).toEqual([
      { field: 'amount', from: { amount: '-45000', currency: 'PYG' }, to: { amount: '-50000', currency: 'PYG' } },
    ])
  })

  it('says lines were written again when no one amount says what changed', () => {
    const transfer = {
      ...lunch,
      lines: [
        { id: 'l1', side: 'account' as const, account_id: 'a1', amount: '-100', currency: 'USD', note: '' },
        { id: 'l2', side: 'account' as const, account_id: 'a2', amount: '100', currency: 'USD', note: '' },
      ],
    }
    expect(changes(revision(['lines'], transfer, { ...transfer, lines: [...transfer.lines].reverse() }))).toEqual([{ field: 'lines' }])
  })

  it('reads a hold posted later as held, then posted on its day', () => {
    const held = { ...lunch, status: 'pending' as const, cleared_on: null }
    const posted = { ...lunch, cleared_on: '2026-10-03' }
    expect(changes(revision(['status'], held, posted))).toEqual([
      { field: 'status', from: { held: true }, to: { held: false, on: '2026-10-03' } },
    ])
    // Posted the day it happened is just posted: the day is the entry's own.
    expect(changes(revision(['status'], held, lunch))).toEqual([{ field: 'status', from: { held: true }, to: { held: false, on: null } }])
  })

  it('names the files put on and taken off', () => {
    const file = { id: 'f1', name: 'receipt.jpg', media_type: 'image/jpeg', size: 2048, created_at: '2026-10-02T12:00:00Z' }
    expect(changes(revision(['attachments'], lunch, { ...lunch, attachments: [file] }))).toEqual([
      { field: 'attachments', added: [file], removed: [] },
    ])
  })

  it('says nothing for what is about the whole entry', () => {
    expect(changes({ ...revision([], lunch, lunch), action: 'created', before: null })).toEqual([])
  })
})

describe('files', () => {
  it('draws only what a browser can draw', () => {
    expect(isPicture('image/jpeg')).toBe(true)
    expect(isPicture('image/webp')).toBe(true)
    // Kept and saved, but most browsers cannot show one.
    expect(isPicture('image/heic')).toBe(false)
    expect(isPicture('application/pdf')).toBe(false)
  })

  it('writes a size the way a person reads it', () => {
    expect(formatSize(400, 'en')).toBe('0.4 kB')
    expect(formatSize(340 * 1024, 'en')).toBe('340 kB')
    expect(formatSize(2.4 * 1024 * 1024, 'en')).toBe('2.4 MB')
    expect(formatSize(2.4 * 1024 * 1024, 'ru')).toMatch(/^2,4\sМБ$/)
  })
})
