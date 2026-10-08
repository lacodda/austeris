import { describe, expect, it } from 'vitest'
import { isFiltered, readFilter, writeFilter } from '@/lib/filters'

describe('the filter in the address', () => {
  it('reads what an address says', () => {
    const filter = readFilter(new URLSearchParams('account=a1&tag=trip&from=2026-09-01&status=pending'))
    expect(filter).toEqual({ account: 'a1', tag: 'trip', from: '2026-09-01', status: 'pending' })
  })

  it('leaves out what the server would refuse', () => {
    const filter = readFilter(new URLSearchParams('from=yesterday&to=2026-13&status=maybe&tag='))
    expect(filter).toEqual({})
  })

  it('writes only what is set, the same way every time', () => {
    expect(writeFilter({ tag: 'trip', account: 'a1' }).toString()).toBe('account=a1&tag=trip')
    expect(writeFilter({}).toString()).toBe('')
  })

  it('reads back what it wrote', () => {
    const filter = { from: '2026-09-01', to: '2026-09-30', counterparty: 'c1', status: 'cleared' as const }
    expect(readFilter(writeFilter(filter))).toEqual(filter)
  })

  it('says whether anything narrows the list', () => {
    expect(isFiltered({})).toBe(false)
    expect(isFiltered({ tag: 'trip' })).toBe(true)
  })
})
