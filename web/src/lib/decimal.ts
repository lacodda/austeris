import type { Decimal } from '@/lib/types'

/*
 * Decimal arithmetic on the strings the API speaks, without a binary float in
 * between (ADR 0004).
 *
 * A form adds up the lines of a split and negates the sum for the account's
 * side, and `0.1 + 0.2` in a double is `0.30000000000000004` - an entry that
 * does not balance and a server that rightly refuses it. Scaled BigInts do
 * the sum exactly. It is the handful of operations a form needs, not a
 * decimal library: twenty lines of our own beat a dependency for one job.
 */

const CANONICAL = /^-?\d+(\.\d+)?$/

/** U+2212, the typographic minus some keyboards write; built from its code so
 * the source stays ASCII and no editor turns it back into a hyphen. */
const MINUS_SIGN = new RegExp(`^${String.fromCharCode(0x2212)}`)

interface Scaled {
  units: bigint
  scale: number
}

function scaled(value: Decimal): Scaled {
  if (!CANONICAL.test(value)) throw new Error(`not a decimal: "${value}"`)
  const negative = value.startsWith('-')
  const [whole = '0', fraction = ''] = (negative ? value.slice(1) : value).split('.')
  const units = BigInt(whole + fraction)
  return { units: negative ? -units : units, scale: fraction.length }
}

function write({ units, scale }: Scaled): Decimal {
  const negative = units < 0n
  const digits = (negative ? -units : units).toString().padStart(scale + 1, '0')
  const whole = digits.slice(0, digits.length - scale)
  const fraction = scale > 0 ? `.${digits.slice(digits.length - scale)}` : ''
  return `${negative ? '-' : ''}${whole}${fraction}`
}

function widen(value: Scaled, scale: number): bigint {
  return value.units * 10n ** BigInt(scale - value.scale)
}

/** Whether a string is a decimal this module (and the API) accepts. */
export function isDecimal(value: string): boolean {
  return CANONICAL.test(value)
}

/** The exact sum, at the widest scale among the terms. Zero for none. */
export function sum(values: readonly Decimal[]): Decimal {
  const terms = values.map(scaled)
  const scale = Math.max(0, ...terms.map((term) => term.scale))
  const units = terms.reduce((total, term) => total + widen(term, scale), 0n)
  return write({ units, scale })
}

export function negate(value: Decimal): Decimal {
  const { units, scale } = scaled(value)
  return write({ units: -units, scale })
}

/** -1, 0 or 1. */
export function sign(value: Decimal): -1 | 0 | 1 {
  const { units } = scaled(value)
  return units < 0n ? -1 : units > 0n ? 1 : 0
}

/** The decimal separator of a language: `.` in English, `,` in Russian. */
export function decimalSeparator(locale: string): string {
  const part = new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === 'decimal')
  return part?.value ?? '.'
}

/**
 * What a person typed into an amount field, as a decimal - or `null` when it
 * is not an amount.
 *
 * Typed in the interface's language: `1 234,50` in Russian, `1,234.50` in
 * English. The language's decimal separator is the decimal point; spaces, the
 * other separator and the apostrophe some write thousands with are grouping and
 * dropped. A `.` is also accepted as the point where the language uses `,` and
 * the text has no comma - a keypad has only a dot, and `10.50` means one thing.
 */
export function parseAmount(text: string, locale: string): Decimal | null {
  const point = decimalSeparator(locale)
  let cleaned = text.trim().replace(/[\s']/g, '').replace(MINUS_SIGN, '-')
  if (cleaned === '') return null

  if (point === ',' && !cleaned.includes(',')) {
    // A dot is the point only when it is clearly not grouping: one of them.
    if ((cleaned.match(/\./g) ?? []).length === 1) cleaned = cleaned.replace('.', ',')
  }
  const grouping = point === ',' ? '.' : ','
  cleaned = cleaned.split(grouping).join('')
  if (point !== '.') cleaned = cleaned.replace(point, '.')

  if (!CANONICAL.test(cleaned)) return null
  return cleaned
}
