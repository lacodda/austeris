import type { HTMLAttributes, MouseEvent, ReactNode } from 'react'
import { Toggle } from '@base-ui/react/toggle'
import { ToggleGroup } from '@base-ui/react/toggle-group'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'dowel-ui'

/*
 * Chip.
 *
 * A badge you can act on - taken away, or switched - and a row of them that
 * switch together.
 *
 * A filter that can be removed, a tag with a count, a selected value in a
 * field - and a filter that is on or off, one of a row of kinds. The difference
 * from a Badge is entirely about whether something happens when you click it,
 * and if something does, that part is a real `<button>` with a real name, not a
 * decorative cross or a `<span>` with a click handler.
 *
 * `onRemove` is the reason this existed separately. Every product wrote the
 * same removable tag and every one of them made the cross a `<span>`, which
 * the keyboard cannot reach and a screen reader does not announce.
 *
 * `pressed` is the reason it grew. kilna alone drew twenty-three switchable
 * chips by hand, in three sizes and five recipes, because the set's Chip had
 * no state - and not one of them told a screen reader whether it was on. A
 * pressed chip is Base UI's Toggle: a `<button>` with `aria-pressed`, and the
 * fill that says "on" is drawn from that attribute, so the look and the
 * announcement cannot disagree.
 *
 * The two are exclusive, and the type says so. A chip that switches when
 * pressed and also holds a cross is a button inside a button: the cross is a
 * nested interactive element no reader can reach on its own, and a press on it
 * would switch the chip as well.
 */
export const chipVariants = cva(
  'inline-flex items-center gap-1 rounded-full border py-0.5 pl-2.5 text-xs whitespace-nowrap transition-colors',
  {
    variants: {
      /*
       * What the chip says before anyone presses it.
       *
       * `outline` is the plain chip. The five tones are the status vocabulary
       * and the accent, in the same fill-and-ink pairs Badge uses, so a chip
       * and a badge side by side in one row do not disagree about what "warn"
       * looks like. `dashed` is the outline with a broken border: a chip that
       * stands for something not there yet - "+ tag", "any kind" - which a
       * solid border would present as one more value.
       */
      variant: {
        outline: 'border-line text-dim',
        dashed: 'border-dashed border-line-2 text-dim',
        soft: 'border-transparent bg-soft text-dim',
        accent: 'border-transparent bg-accent-soft font-medium text-accent',
        good: 'border-transparent bg-good-soft font-medium text-good',
        warn: 'border-transparent bg-warn-soft font-medium text-warn',
        bad: 'border-transparent bg-bad-soft font-medium text-bad',
        info: 'border-transparent bg-info-soft font-medium text-info',
      },
      /** Padding on the right depends on whether a remove button sits there. */
      removable: {
        true: 'pr-1',
        false: 'pr-2.5',
      },
    },
    defaultVariants: { variant: 'outline', removable: false },
  },
)

/*
 * The clothes a chip wears when it is a switch.
 *
 * "On" is the accent fill in semibold - the mockup's `.chip.on` - and it wins
 * over whatever tone the chip wears while off, because the question a row of
 * filters answers is "which of these is on", and it has to read the same for
 * every chip in the row. Drawn from `data-pressed`, which Base UI sets from the
 * same state that sets `aria-pressed`.
 *
 * `target-min`, because a chip is 21 pixels tall by design and a switch is a
 * pointer target: the hit area grows to the floor, the chip does not.
 */
const pressableClasses = cn(
  'target-min cursor-pointer',
  'hover:border-line-2 hover:text-text',
  'data-[pressed]:border-transparent data-[pressed]:bg-accent-soft data-[pressed]:font-semibold data-[pressed]:text-accent',
  'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
  'data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50',
)

interface ChipOwnProps extends Omit<VariantProps<typeof chipVariants>, 'removable'> {
  /** Shown after the label, for a count. */
  count?: number
  className?: string
  children?: ReactNode
}

/*
 * Removable, switchable, or neither - and never both.
 *
 * `onRemove` travels with `removeLabel` as a union rather than as two
 * optionals, so a remove button exists only when there is a word for it.
 * There is no default word on purpose: a string the component invents is a
 * string the product cannot translate.
 *
 * A switch is a chip given `pressed`, `defaultPressed` or `onPressedChange` -
 * or a `value`, which is how a chip says which member of a ChipGroup it is.
 */
type Removable = {
  onRemove: () => void
  removeLabel: string
  pressed?: never
  defaultPressed?: never
  onPressedChange?: never
  value?: never
  disabled?: never
}

type Pressable = {
  onRemove?: never
  removeLabel?: never
  /** Whether it is on. Controlled; pair it with `onPressedChange`. */
  pressed?: boolean
  /** Whether it starts on, for a chip nobody else needs to know about. */
  defaultPressed?: boolean
  onPressedChange?: (pressed: boolean) => void
  /** Which member of a ChipGroup this is. Inside a group the group owns the
   * state, and `pressed` is not read. */
  value?: string
  disabled?: boolean
}

type Plain = {
  onRemove?: never
  removeLabel?: never
  pressed?: never
  defaultPressed?: never
  onPressedChange?: never
  value?: never
  disabled?: never
}

export type ChipProps = ChipOwnProps &
  Omit<HTMLAttributes<HTMLElement>, 'onSelect' | 'defaultValue' | 'color'> &
  (Removable | Pressable | Plain)

export function Chip(props: ChipProps) {
  const {
    variant,
    count,
    onRemove,
    removeLabel,
    pressed,
    defaultPressed,
    onPressedChange,
    value,
    disabled,
    className,
    children,
    ...rest
  } = props

  const content = (
    <>
      {children}
      {count !== undefined && <span className="font-normal text-faint tabular-nums">{count}</span>}
    </>
  )

  const switchable =
    pressed !== undefined || defaultPressed !== undefined || onPressedChange !== undefined || value !== undefined

  if (switchable) {
    return (
      <Toggle
        pressed={pressed}
        defaultPressed={defaultPressed}
        onPressedChange={onPressedChange ? (next) => onPressedChange(next) : undefined}
        value={value}
        disabled={disabled}
        className={cn(chipVariants({ variant, removable: false }), pressableClasses, className)}
        {...rest}
      >
        {content}
      </Toggle>
    )
  }

  return (
    <span className={cn(chipVariants({ variant, removable: Boolean(onRemove) }), className)} {...rest}>
      {content}

      {onRemove && (
        <button
          type="button"
          aria-label={removeLabel}
          onClick={(event: MouseEvent<HTMLButtonElement>) => {
            // A chip is often inside something else that is also clickable.
            event.stopPropagation()
            onRemove()
          }}
          className={cn(
            // `target-min` keeps the cross the size it looks - a chip is a
            // small thing and a cross a third of its height reads as a button
            // with a chip around it - while the area a pointer has to find
            // grows to the floor the theme sets.
            'grid size-4 place-items-center rounded-full text-faint transition-colors target-min',
            'hover:bg-line hover:text-text',
            'focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent',
          )}
        >
          <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
            <path d="M4 4l8 8M12 4l-8 8" strokeLinecap="round" />
          </svg>
        </button>
      )}
    </span>
  )
}

export interface ChipGroupProps extends Omit<HTMLAttributes<HTMLDivElement>, 'defaultValue' | 'onChange'> {
  /** What the row of chips is: "Kind", "Status". A group without a name is
   * a row of buttons a reader meets one at a time with no idea what they
   * choose between. */
  'aria-label'?: string
  'aria-labelledby'?: string
  /** The values of the chips that are on. An array in both modes, because
   * that is Base UI's shape and a single mode is a group that holds at most
   * one. */
  value?: readonly string[]
  defaultValue?: readonly string[]
  onValueChange?: (value: string[]) => void
  /** Several on at once - filters - rather than one at a time - a kind. */
  multiple?: boolean
  disabled?: boolean
  children: ReactNode
}

/**
 * A row of chips that switch together.
 *
 * Base UI's ToggleGroup underneath, so the row is one stop on the Tab key and
 * the arrows walk the chips - a filter bar of eight kinds is not eight Tab
 * presses long - and `multiple` decides whether pressing one lets go of the
 * others. The chips inside are ordinary Chips with a `value`.
 */
export function ChipGroup({ value, defaultValue, onValueChange, multiple = false, className, children, ...props }: ChipGroupProps) {
  return (
    <ToggleGroup
      value={value}
      defaultValue={defaultValue}
      onValueChange={onValueChange ? (next) => onValueChange(next as string[]) : undefined}
      multiple={multiple}
      className={cn('flex flex-wrap items-center gap-1.5', className)}
      {...props}
    >
      {children}
    </ToggleGroup>
  )
}
