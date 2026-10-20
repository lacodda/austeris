import { describe, expect, it } from 'vitest'
import { fillFromSlip } from '@/lib/imports'
import type { SlipReading } from '@/lib/types'

const reading = (amounts: [string, string][], rate?: SlipReading['rate']): SlipReading => ({
  amounts: amounts.map(([amount, currency]) => ({ amount, currency })),
  rate: rate ?? null,
  text: '',
})

describe('fillFromSlip', () => {
  it('puts each amount read on the side whose currency it is in', () => {
    const slip = reading([['100', 'USD'], ['735000', 'PYG']])
    // Selling dollars: given in dollars, got in guaranies - and the other way.
    expect(fillFromSlip(slip, 'USD', 'PYG')).toEqual({ given: '100', got: '735000' })
    expect(fillFromSlip(slip, 'PYG', 'USD')).toEqual({ given: '735000', got: '100' })
  })

  it('works the missing side out from the rate, in that currency’s places', () => {
    const rate = { base: 'USD', quote: 'PYG', rate: '7350', stated: true }
    expect(fillFromSlip(reading([['100', 'USD']], rate), 'PYG', 'USD')).toEqual({ given: '735000', got: '100' })
    expect(fillFromSlip(reading([['735000', 'PYG']], rate), 'PYG', 'USD')).toEqual({ given: '735000', got: '100.00' })
  })

  it('places a rate by whichever of its currencies is known', () => {
    // Only guaranies on the slip, and a rate of something in them: the
    // other account's currency is what was priced.
    const rate = { quote: 'PYG', rate: '7350', stated: true }
    expect(fillFromSlip(reading([['1470000', 'PYG']], rate), 'PYG', 'USD')).toEqual({ given: '1470000', got: '200.00' })
  })

  it('leaves alone what it cannot place rather than guessing', () => {
    // A rate naming neither of the form's currencies.
    const rate = { base: 'EUR', quote: 'BRL', rate: '6.1', stated: true }
    expect(fillFromSlip(reading([['100', 'USD']], rate), 'PYG', 'USD')).toEqual({ given: undefined, got: '100' })
    expect(fillFromSlip(reading([]), 'PYG', 'USD')).toEqual({ given: undefined, got: undefined })
  })
})
