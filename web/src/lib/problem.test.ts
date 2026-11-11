import { afterEach, describe, expect, it } from 'vitest'
import i18n from '@/i18n'
import { ApiError } from '@/lib/api'
import { problem } from '@/lib/problem'

describe('a refusal in the reader`s language', () => {
  afterEach(async () => {
    await i18n.changeLanguage('en')
  })

  it('writes the units it names the way the language writes numbers', async () => {
    const refused = new ApiError(409, 'that leaves 0.955 BTC fewer', 'trade.oversold', { symbol: 'BTC', short: '0.955', at: '2026-10-20T13:00:00Z' })
    expect(problem(refused, i18n.t)).toBe('That would leave 0.955 BTC fewer than were held at the time.')
    await i18n.changeLanguage('ru')
    expect(problem(refused, i18n.t)).toBe('Тогда на тот момент не хватило бы 0,955 BTC.')
  })

  it('falls back on the server`s sentence for a code it has none for', () => {
    expect(problem(new ApiError(400, 'the server says so', 'nobody.knows'), i18n.t)).toBe('the server says so')
  })
})
