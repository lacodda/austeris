import { describe, expect, it } from 'vitest'
import { formatMoney, formatRate, isIsoCurrency } from '@/lib/money'

/** Intl's spaces are narrow or non-breaking depending on the language; the
 * tests read them as one space so they say what the reader sees. */
const plain = (text: string) => text.replace(/\s/g, ' ')

describe('formatMoney', () => {
  it('writes guaraníes without kopecks', () => {
    expect(plain(formatMoney('45000.00', 'PYG', 'en'))).toBe('₲45,000')
    expect(plain(formatMoney('1234500', 'PYG', 'ru'))).toBe('1 234 500 ₲')
  })

  it('writes dollars and roubles with their two digits', () => {
    expect(plain(formatMoney('12.5', 'USD', 'en'))).toBe('$12.50')
    expect(plain(formatMoney('-1500', 'RUB', 'ru'))).toBe('-1 500,00 ₽')
  })

  it('separates thousands the way the language does', () => {
    expect(plain(formatMoney('1234567.89', 'USD', 'en'))).toBe('$1,234,567.89')
    expect(plain(formatMoney('1234567.89', 'USD', 'ru'))).toBe('1 234 567,89 $')
  })

  it('keeps a symbol to the currency that owns it', () => {
    // Two currencies written `$` side by side would be two amounts nobody can
    // tell apart; the one the language writes as `$` keeps it.
    expect(plain(formatMoney('1', 'USD', 'en'))).toBe('$1.00')
    expect(plain(formatMoney('1', 'AUD', 'en'))).toBe('A$1.00')
  })

  it('formats the string it was given, not a double', () => {
    // Past 2^53 a number is already rounded before Intl sees it.
    expect(plain(formatMoney('12345678901234567.89', 'USD', 'en'))).toBe('$12,345,678,901,234,567.89')
  })

  it('writes a coin with every digit and its code', () => {
    expect(plain(formatMoney('0.00150000', 'BTC', 'en'))).toBe('0.0015 BTC')
    expect(plain(formatMoney('1234.123456789', 'USDT', 'ru'))).toBe('1 234,123456789 USDT')
  })

  it('marks money coming in when a column asks for it', () => {
    expect(plain(formatMoney('1200000', 'PYG', 'en', { signDisplay: 'exceptZero' }))).toBe('+₲1,200,000')
    expect(plain(formatMoney('-45000', 'PYG', 'en', { signDisplay: 'exceptZero' }))).toBe('-₲45,000')
  })
})

describe('isIsoCurrency', () => {
  it('knows a currency from a coin', () => {
    expect(isIsoCurrency('pyg')).toBe(true)
    expect(isIsoCurrency('BTC')).toBe(false)
  })
})

describe('formatRate', () => {
  it('writes a rate to the digits a person compares', () => {
    expect(plain(formatRate('5965', 'en'))).toBe('5,965')
    expect(plain(formatRate('1.061571125300000000', 'en'))).toBe('1.06157')
    expect(plain(formatRate('0.0001676', 'en'))).toBe('0.0001676')
    expect(plain(formatRate('6439.35', 'ru'))).toBe('6 439,35')
  })
})
