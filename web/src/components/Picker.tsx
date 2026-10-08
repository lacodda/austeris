import { useMemo, useState } from 'react'
import {
  Combobox,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxPopup,
} from '@/components/ui/combobox'

/** One row of a picker. */
export interface PickerOption {
  value: string
  label: string
}

/** What was picked: something that exists, or a name for something new. */
export type Picked = { id: string } | { name: string }

interface Row extends PickerOption {
  /** The row that offers to create what was typed. */
  isNew?: boolean
}

const NEW = 'new:'

interface PickerProps {
  options: readonly PickerOption[]
  value: Picked | null
  onValueChange: (value: Picked | null) => void
  /** Names the field for a screen reader when there is no visible label. */
  'aria-label'?: string
  id?: string
  placeholder?: string
  /** What the list says when nothing matches. */
  emptyLabel: string
  /** When given, a name that matches nothing is offered as a new row, in
   * these words - `Create “Casa Rica”`. Without it the list only chooses. */
  createLabel?: (name: string) => string
  disabled?: boolean
}

/**
 * Choose from a list, or name something that is not in it yet.
 *
 * A counterparty or a category is usually one that exists, and sometimes the
 * one being paid for the first time. Two controls for those - a picker and a
 * "new" button that opens a dialog - put a whole form between a person and the
 * receipt in their hand. Here the new one is just typed; what is created and
 * when is the caller's: a form creates it when it is saved, so a cancelled form
 * leaves nothing behind.
 */
export function Picker({
  options,
  value,
  onValueChange,
  id,
  placeholder,
  emptyLabel,
  createLabel,
  disabled,
  ...aria
}: PickerProps) {
  const [query, setQuery] = useState('')

  const rows = useMemo<Row[]>(() => {
    const typed = query.trim()
    const known = options.some((option) => option.label.localeCompare(typed, undefined, { sensitivity: 'base' }) === 0)
    if (!createLabel || typed === '' || known) return [...options]
    // Labelled with the bare name, so it passes the filter on what was typed
    // and fills the input with the name rather than the offer.
    return [...options, { value: `${NEW}${typed}`, label: typed, isNew: true }]
  }, [options, query, createLabel])

  const selected = useMemo<Row | null>(() => {
    if (value === null) return null
    if ('id' in value) return options.find((option) => option.value === value.id) ?? null
    return { value: `${NEW}${value.name}`, label: value.name, isNew: true }
  }, [options, value])

  return (
    <Combobox<Row>
      items={rows}
      value={selected}
      onValueChange={(row) => {
        if (row === null) onValueChange(null)
        else if (row.isNew) onValueChange({ name: row.label })
        else onValueChange({ id: row.value })
      }}
      // Uncontrolled: Base UI writes the chosen row's label into the input,
      // including when the value is set from outside (a counterparty filling
      // in its usual category). The query is only watched, for the offer.
      onInputValueChange={setQuery}
      itemToStringLabel={(row) => row.label}
      isItemEqualToValue={(row, other) => row.value === other.value}
      disabled={disabled}
    >
      <ComboboxInput id={id} placeholder={placeholder} {...aria} />
      <ComboboxPopup>
        {/* `empty:p-0`: the element stays mounted for its announcement, and
            with dowel 0.35 its padding stood as a blank band above every list
            that did match. */}
        <ComboboxEmpty className="empty:p-0">{emptyLabel}</ComboboxEmpty>
        <ComboboxList>
          {(row: Row) => (
            <ComboboxItem key={row.value} value={row}>
              {row.isNew && createLabel ? createLabel(row.label) : row.label}
            </ComboboxItem>
          )}
        </ComboboxList>
      </ComboboxPopup>
    </Combobox>
  )
}
