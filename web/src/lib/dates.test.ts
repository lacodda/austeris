import { describe, expect, it } from 'vitest'
import { formatDay, formatListDay, today } from '@/lib/dates'

describe('formatDay', () => {
  it('writes the day it was given, in the language asked', () => {
    expect(formatDay('2026-10-08', 'en')).toBe('Oct 8, 2026')
    expect(formatDay('2026-10-08', 'ru')).toMatch(/^8 окт\.? 2026/)
  })

  it('does not slide a day west of Greenwich', () => {
    // Midnight UTC is the evening before in Asunción; a day is not an instant.
    expect(formatDay('2026-01-01', 'en', { month: 'long' })).toBe('January 1, 2026')
  })

  it('leaves what is not a day as it was', () => {
    expect(formatDay('soon', 'en')).toBe('soon')
  })
})

describe('formatListDay', () => {
  it('leaves this year out and writes any other', () => {
    expect(formatListDay('2026-10-08', 'en', '2026-12-31')).toBe('Oct 8')
    expect(formatListDay('2025-12-31', 'en', '2026-01-02')).toBe('Dec 31, 2025')
  })
})

describe('today', () => {
  it('is a day the ledger reads', () => {
    expect(today()).toMatch(/^\d{4}-\d{2}-\d{2}$/)
  })
})
