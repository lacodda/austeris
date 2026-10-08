import { useCallback, useState, type KeyboardEvent, type ReactNode } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'dowel-ui'
import { fieldClasses } from './input'
import { Chip } from './chip'

/*
 * TagInput.
 *
 * Free text turned into a list: type a word, press Enter, it becomes a chip.
 * This is the field for labels, keywords, recipients - anywhere the set of
 * values is not known in advance, which is exactly what separates it from
 * `Combobox multiple`. Choosing from a list is that component's job; this one
 * is for values that do not exist until someone types them.
 *
 * Base UI has no part for this, and the reason is worth stating: its Combobox
 * chips are for values chosen from a collection, and "create the thing I just
 * typed" is a decision about the product's data, not about the widget. So the
 * behaviour here is written, and written narrowly.
 *
 * What it does, and why each one is not optional:
 *
 * - **Enter commits.** The obvious one, and the reason it is a field at all.
 * - **Backspace on an empty input removes the last tag.** Without it the only
 *   way back is the mouse, and a list built by typing should be unbuildable
 *   by typing too.
 * - **Blur commits what was typed.** A word left in the box when the reader
 *   clicks Save is a word they believe they entered. Losing it silently is
 *   the single commonest complaint about fields of this shape.
 * - **Duplicates are refused, not stacked.** A tag list is a set.
 * - **Whitespace is trimmed and empties dropped**, so Enter on a blank input
 *   does nothing rather than adding a tag nobody can see.
 *
 * What it deliberately does not do: validate. What makes a tag acceptable -
 * length, charset, whether it must already exist - is the product's rule, and
 * a component that guessed would be wrong in both directions. Reject a value
 * by not putting it in `value`.
 */

export const tagInputVariants = cva(
  cn(
    fieldClasses,
    'flex flex-wrap items-center gap-1 py-1',
    'focus-within:outline-2 focus-within:outline-offset-0 focus-within:outline-accent',
  ),
  {
    variants: {
      size: {
        sm: 'min-h-8 text-xs',
        md: 'min-h-9',
        lg: 'min-h-10 text-base',
      },
    },
    defaultVariants: { size: 'md' },
  },
)

/* The box inside the box.
 *
 * `min-h-*` on the container sets a floor; the input inside it has a height of
 * its own, and that is what a reader actually sees. A size that moved only the
 * container moved nothing - measured, `sm` and `md` both came out 38px tall,
 * because the input was `h-7` in all three and its padding decided the rest.
 *
 * `tailwind-merge` does not rescue this either: `h-7` and `min-h-8` are not
 * the same property, so both survive and the fixed height wins. The two have
 * to be declared together.
 */
const tagInputBoxVariants = cva(
  'w-auto min-w-24 flex-1 bg-transparent px-1 text-text outline-none placeholder:text-faint',
  {
    variants: {
      size: {
        sm: 'h-6 text-xs',
        md: 'h-7 text-sm',
        lg: 'h-8 text-base',
      },
    },
    defaultVariants: { size: 'md' },
  },
)

export interface TagInputProps extends VariantProps<typeof tagInputVariants> {
  /** The tags. Controlled: the product owns the list. */
  value: string[]
  /** Called with the next list. */
  onValueChange: (value: string[]) => void
  /** Placeholder for the text box, shown when there is room for it. */
  placeholder?: string
  /** Names the field. Required - a box of chips with no label names nothing. */
  'aria-label'?: string
  /** Or points at a label element, when there is a visible one. */
  'aria-labelledby'?: string
  /** How a tag's remove button is named, for a screen reader. Takes the tag,
   * so the product can say "Remove documentation" in its own language.
   *
   * Required, like `Chip`'s own: a remove button exists only when there is a
   * word for it, and a word this component invented would ship in English to
   * every reader who does not read English. */
  removeLabel: (tag: string) => string
  /** Cap on how many tags may be added. */
  max?: number
  disabled?: boolean
  /** Drawn after the input, inside the box - a counter, a hint. */
  children?: ReactNode
  className?: string
  id?: string
}

export function TagInput({
  value,
  onValueChange,
  placeholder,
  removeLabel,
  max,
  disabled,
  size,
  children,
  className,
  id,
  ...aria
}: TagInputProps) {
  const [draft, setDraft] = useState('')

  const full = max !== undefined && value.length >= max

  const commit = useCallback(
    (raw: string) => {
      const tag = raw.trim()
      // Empty, duplicate, or past the cap: nothing to add. The draft is
      // cleared either way, so Enter always leaves the box ready for the next
      // word rather than leaving a rejected one sitting there.
      if (tag && !value.includes(tag) && !full) onValueChange([...value, tag])
      setDraft('')
    },
    [full, onValueChange, value],
  )

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      // Only when there is something to commit: otherwise Enter belongs to the
      // form around this, and swallowing it would break submitting by keyboard.
      if (draft.trim()) {
        event.preventDefault()
        commit(draft)
      }
      return
    }
    if (event.key === 'Backspace' && draft === '' && value.length > 0) {
      onValueChange(value.slice(0, -1))
    }
  }

  return (
    <div
      className={cn(tagInputVariants({ size }), disabled && 'pointer-events-none opacity-50', className)}
    >
      {value.map((tag) => (
        <Chip
          key={tag}
          removeLabel={removeLabel(tag)}
          onRemove={() => onValueChange(value.filter((entry) => entry !== tag))}
        >
          {tag}
        </Chip>
      ))}
      <input
        id={id}
        value={draft}
        disabled={disabled}
        // The list is the value; the box is how you add to it. Announcing the
        // box as the field would name the wrong thing, so the label goes here
        // where a screen reader meets it.
        {...aria}
        placeholder={full ? undefined : placeholder}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={onKeyDown}
        // A word typed and not committed is a word the reader thinks they
        // entered. Blur is where that belief meets the form's Save button.
        onBlur={() => commit(draft)}
        className={tagInputBoxVariants({ size })}
      />
      {children}
    </div>
  )
}
