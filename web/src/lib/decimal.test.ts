import { describe, expect, it } from 'vitest'
import { isDecimal, negate, parseAmount, sign, sum } from '@/lib/decimal'

describe('sum', () => {
  it('adds exactly where a double would not', () => {
    // 0.1 + 0.2 is 0.30000000000000004 in a float; a split that adds up like
    // that does not balance, and the ledger refuses it.
    expect(sum(['0.1', '0.2'])).toBe('0.3')
    expect(sum(['0.1', '0.2', '-0.3'])).toBe('0.0')
  })

  it('keeps the widest scale and the sign', () => {
    expect(sum(['45000', '0.50'])).toBe('45000.50')
    expect(sum(['-10.5', '3'])).toBe('-7.5')
    expect(sum(['-0.05', '0.01'])).toBe('-0.04')
  })

  it('holds figures past what a double can carry', () => {
    expect(sum(['12345678901234567.89', '0.01'])).toBe('12345678901234567.90')
    expect(sum(['0.00000001', '0.00000002'])).toBe('0.00000003')
  })

  it('is zero for nothing', () => {
    expect(sum([])).toBe('0')
  })
})

describe('negate and sign', () => {
  it('turns a sum into the other side', () => {
    expect(negate('45000.50')).toBe('-45000.50')
    expect(negate('-1')).toBe('1')
    expect(sign('-0.01')).toBe(-1)
    expect(sign('0.00')).toBe(0)
    expect(sign('3')).toBe(1)
  })
})

describe('parseAmount', () => {
  it('reads an English amount', () => {
    expect(parseAmount('1,234.50', 'en')).toBe('1234.50')
    expect(parseAmount(' 45000 ', 'en')).toBe('45000')
  })

  it('reads a Russian amount, with the space it groups by', () => {
    expect(parseAmount('1 234,50', 'ru')).toBe('1234.50')
    expect(parseAmount(`1${String.fromCharCode(0xa0)}234,5`, 'ru')).toBe('1234.5')
  })

  it('takes a keypad dot as the point where the language writes a comma', () => {
    expect(parseAmount('10.50', 'ru')).toBe('10.50')
  })

  it('refuses what is not an amount', () => {
    expect(parseAmount('', 'en')).toBeNull()
    expect(parseAmount('12a', 'en')).toBeNull()
    expect(parseAmount('1.2.3', 'en')).toBeNull()
  })

  it('answers in the form the API takes', () => {
    for (const text of ['1,234.50', '7', '-3.25']) {
      expect(isDecimal(parseAmount(text, 'en') ?? '')).toBe(true)
    }
  })
})
