import type { Decimal } from '@/lib/types'

/*
 * Money, written the way the interface's language writes it.
 *
 * Three things a household with guaraníes, dollars and roubles needs, all of
 * them `Intl`'s and none of them ours to tabulate:
 *
 * - **the currency's own digits.** PYG has no minor unit and is written
 *   without kopecks - `₲45,000`, never `₲45,000.00`; USD and RUB have two.
 * - **the language's separators.** `1,234,500` in English, `1 234 500` in
 *   Russian.
 * - **a symbol, where it says which currency it is.** See `display`.
 *
 * The amount goes to `Intl` as the string the API sent. `NumberFormat.format`
 * takes a decimal string and formats it exactly; turning it into a number
 * first would round a crypto balance or a large total through a double before
 * it is drawn (ADR 0004).
 *
 * A code that is not ISO 4217 - a coin, `BTC` - has no minor unit to round to
 * (the ledger keeps it unrounded too), so it is written with every digit it
 * has and its code after it.
 */

const iso = new Set(Intl.supportedValuesOf('currency'))

/** Between a coin's figures and its code, so the two never part at a line
 * end. Built from its code: a literal one is invisible in the source and
 * fails the linter's irregular-whitespace rule. */
const NO_BREAK_SPACE = String.fromCharCode(0xa0)

/** Whether a code is an ISO 4217 currency `Intl` knows how to write. */
export function isIsoCurrency(code: string): boolean {
  return iso.has(code.toUpperCase())
}

const displays = new Map<string, 'narrowSymbol' | 'symbol'>()

/**
 * Which symbol a currency is written with in a language.
 *
 * The narrow symbol when it belongs to this currency alone - `₲`, `₽`, `€`.
 * Where several share one, the narrow form is kept only by the currency the
 * language itself writes that way, and the rest take their standard symbol:
 * in English USD is `$` and AUD is `A$`, not two different amounts both
 * written `$`. Asked of `Intl` rather than listed, so a language that writes
 * a symbol differently is right without anyone noticing.
 */
function display(code: string, locale: string): 'narrowSymbol' | 'symbol' {
  const key = `${locale}:${code}`
  const known = displays.get(key)
  if (known) return known

  const symbol = (style: 'narrowSymbol' | 'symbol') =>
    new Intl.NumberFormat(locale, { style: 'currency', currency: code, currencyDisplay: style })
      .formatToParts(0)
      .find((part) => part.type === 'currency')?.value ?? code
  const narrow = symbol('narrowSymbol')
  const standard = symbol('symbol')
  const shared = [...iso].some((other) => other !== code && symbolOf(other, locale) === narrow)
  const chosen = !shared || standard === narrow ? 'narrowSymbol' : 'symbol'
  displays.set(key, chosen)
  return chosen
}

const narrows = new Map<string, Map<string, string>>()

/** Every currency's narrow symbol in a language, worked out once. */
function symbolOf(code: string, locale: string): string {
  let table = narrows.get(locale)
  if (!table) {
    table = new Map()
    for (const other of iso) {
      const part = new Intl.NumberFormat(locale, { style: 'currency', currency: other, currencyDisplay: 'narrowSymbol' })
        .formatToParts(0)
        .find((p) => p.type === 'currency')
      table.set(other, part?.value ?? other)
    }
    narrows.set(locale, table)
  }
  return table.get(code) ?? code
}

export interface MoneyOptions {
  /** `exceptZero` puts a `+` on money coming in, for a column of movements. */
  signDisplay?: Intl.NumberFormatOptions['signDisplay']
}

/** An amount in a currency, as text in a language. */
export function formatMoney(amount: Decimal, currency: string, locale: string, options: MoneyOptions = {}): string {
  const code = currency.toUpperCase()
  const value = amount as Intl.StringNumericLiteral
  if (isIsoCurrency(code)) {
    return new Intl.NumberFormat(locale, {
      style: 'currency',
      currency: code,
      currencyDisplay: display(code, locale),
      signDisplay: options.signDisplay,
    }).format(value)
  }
  const figures = new Intl.NumberFormat(locale, {
    maximumFractionDigits: 20,
    signDisplay: options.signDisplay,
  }).format(value)
  return `${figures}${NO_BREAK_SPACE}${code}`
}

/**
 * A rate, to six significant digits: `5,965`, `1.06157`, `0.000168`.
 *
 * The ledger keeps a rate to the last digit and works a fee out from all of
 * them; a person comparing an exchange office's board with the day's rate
 * reads the first few, and ten decimals of `1.0615711253` only hide them.
 */
export function formatRate(rate: Decimal, locale: string): string {
  return new Intl.NumberFormat(locale, { maximumSignificantDigits: 6 }).format(rate as Intl.StringNumericLiteral)
}
