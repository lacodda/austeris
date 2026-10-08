import { Input, type InputProps } from '@/components/ui/input'
import { cn } from '@/lib/utils'

/**
 * Where an amount is typed: a text field, never `type="number"`.
 *
 * A number input hands back a double and rejects `1 234,50` in a language that
 * writes it that way; this one keeps what was typed, and the form reads it
 * with `parseAmount` in the interface's language. `inputMode="decimal"` still
 * brings up the phone's keypad with a separator on it.
 */
export function AmountInput({ className, ...props }: Omit<InputProps, 'type' | 'inputMode'>) {
  return (
    <Input
      type="text"
      inputMode="decimal"
      autoComplete="off"
      className={cn('text-right tabular-nums', className)}
      {...props}
    />
  )
}
