import { useCallback, useState, type FocusEvent, type RefObject } from 'react'
import { useCommands, type Command } from '@/components/ui/commands'

/*
 * A keyboard cursor over the rows of a table.
 *
 * J and K - or the arrows - walk it down and up, as in a mail client; X
 * chooses the row it is on for what is done to many at once. The cursor is
 * the focus rather than a highlight of its own: moving it focuses the row's
 * own control - the button that opens it - so a screen reader hears the row,
 * Enter presses the button, and a click or a Tab puts the cursor where the
 * focus went. Two cursors, one drawn and one focused, would part the moment
 * someone used both the keyboard and the mouse.
 *
 * The keys are declared as commands (dowel's `commands`), so they are listed
 * in the sheet `?` opens, leave a field being typed in alone, and give way to
 * a dialog or a menu that owns its letters.
 *
 * A row says where it is with `data-row` - its index - and which of its
 * controls the cursor lands on with `data-row-control`.
 */

/** What the keys are called in the sheet of shortcuts. */
export interface CursorLabels {
  /** The heading they are listed under. */
  group: string
  next: string
  previous: string
  open: string
  /** Absent when the table has nothing to choose rows for. */
  choose?: string
}

interface RowCursorOptions {
  /** The table the rows are in. */
  table: RefObject<HTMLElement | null>
  /** How many rows there are. */
  count: number
  labels: CursorLabels
  /** Chooses or unchooses the row at an index; absent when rows are not
   * chosen. */
  onChoose?: (index: number) => void
  /** Off while the screen is covered: a drawer open over the table. */
  enabled?: boolean
}

export interface RowCursor {
  /** The row the cursor is on, or `null` before it has been anywhere. */
  index: number | null
  /** Hand to the table's `onFocus`: the cursor follows the focus. */
  onFocus: (event: FocusEvent<HTMLElement>) => void
}

export function useRowCursor({ table, count, labels, onChoose, enabled = true }: RowCursorOptions): RowCursor {
  const [index, setIndex] = useState<number | null>(null)
  // A row that has gone - the list was narrowed - takes the cursor to the
  // last one there is, rather than to nowhere.
  const at = index === null ? null : Math.min(index, count - 1)

  const go = useCallback(
    (next: number) => {
      const row = table.current?.querySelector<HTMLElement>(`[data-row="${next}"]`)
      const control = row?.querySelector<HTMLElement>('[data-row-control]')
      if (!row || !control) return
      setIndex(next)
      control.focus({ preventScroll: true })
      row.scrollIntoView({ block: 'nearest' })
    },
    [table],
  )

  const step = (by: 1 | -1) => {
    if (count === 0) return
    // The first step goes to the first row, whichever way it is.
    const next = at === null ? (by === 1 ? 0 : count - 1) : Math.max(0, Math.min(count - 1, at + by))
    go(next)
  }

  const commands: Command[] = [
    { id: 'rows.next', label: labels.next, group: labels.group, keys: ['J', 'ArrowDown'], run: () => step(1), enabled },
    { id: 'rows.previous', label: labels.previous, group: labels.group, keys: ['K', 'ArrowUp'], run: () => step(-1), enabled },
    // Answered by the row's own button, which has the focus: listed here so
    // the sheet says so.
    { id: 'rows.open', label: labels.open, group: labels.group, keys: 'Enter' },
  ]
  if (onChoose && labels.choose) {
    commands.push({
      id: 'rows.choose',
      label: labels.choose,
      group: labels.group,
      keys: 'X',
      run: () => {
        if (at !== null) onChoose(at)
      },
      enabled: enabled && at !== null,
    })
  }
  useCommands(commands)

  const onFocus = (event: FocusEvent<HTMLElement>) => {
    const row = (event.target as HTMLElement).closest<HTMLElement>('[data-row]')
    const found = row?.dataset.row
    if (found !== undefined) setIndex(Number(found))
  }

  return { index: at, onFocus }
}
