import { useLocale } from 'dowel-ui'
import { cn } from '@/lib/utils'
import { formatMoney, type MoneyOptions } from '@/lib/money'
import type { Decimal } from '@/lib/types'

interface MoneyProps extends MoneyOptions {
  amount: Decimal
  currency: string
  className?: string
}

/**
 * An amount in its currency, in the interface's language.
 *
 * `tabular-nums` so a column of them lines its digits up - the eye compares
 * lengths, and `1` and `8` must take the same room for that to work.
 * `whitespace-nowrap` because a currency symbol on the line below its amount
 * reads as two values.
 */
export function Money({ amount, currency, signDisplay, className }: MoneyProps) {
  const locale = useLocale()
  return (
    <span className={cn('whitespace-nowrap tabular-nums', className)}>
      {formatMoney(amount, currency, locale, { signDisplay })}
    </span>
  )
}
