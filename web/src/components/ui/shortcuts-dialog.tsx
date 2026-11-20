import { useId, useRef, useState, type HTMLAttributes, type ReactNode } from 'react'
import { cn, type HeadingLevel } from 'dowel-ui'
import { Button } from './button'
import { groupCommands, useCommand, useCommandList, type ListedCommand } from './commands'
import { Dialog, DialogBody, DialogClose, DialogDescription, DialogHeader, DialogPopup, DialogTitle } from './dialog'
import { Kbd } from './kbd'

/*
 * ShortcutsDialog.
 *
 * The sheet of every key the application answers to, opened by `?` - written
 * from the list of commands rather than by hand, so it cannot promise a key
 * that does not work.
 *
 * Every hand-written sheet of shortcuts is a version behind what the
 * application actually answers to. This one has no rows of its own: it lists
 * what is declared now, under the headings the commands give themselves, with
 * the keys they answer to after a rebinding and after a conflict. A command
 * declared by the open screen appears while that screen is open, and is gone
 * when it is not.
 *
 * It declares its own command, so `?` opens it with no wiring, the palette
 * offers it, and it lists itself - under the title the product gives it.
 *
 * It opens with the focus on the list rather than on the close button. A sheet
 * is read, not answered: with the focus on the cross, the Enter that someone
 * presses to see what happens closes it, and the arrows that should scroll
 * the list do nothing.
 */

/** The id of the command the dialog declares, for a rebinding or a menu item
 * that opens it with `runCommand`. */
export const SHORTCUTS_COMMAND = 'shortcuts'

/** One heading and its keys. A group rather than a section: a named section
 * is a landmark, and a sheet of six headings would put six landmarks into the
 * list a screen reader jumps between - on a settings page, beside the page's
 * own. */
function Group({ title, commands, level }: { title: string | undefined; commands: ListedCommand[]; level: HeadingLevel }) {
  const heading = useId()
  const Heading = `h${level}` as const
  return (
    <div
      role="group"
      aria-labelledby={title === undefined ? undefined : heading}
      className="mb-4 break-inside-avoid last:mb-0"
    >
      {title !== undefined && (
        <Heading id={heading} className="caption pb-1">
          {title}
        </Heading>
      )}
      <dl>
        {commands.map((command) => (
          <div key={command.id} className="flex items-baseline justify-between gap-3 py-1">
            <dt className="min-w-0 text-sm">{command.label}</dt>
            {/* Alternatives one under another: side by side, `G S` and `Ctrl ,`
                read as one shortcut of four keys. */}
            <dd className="flex shrink-0 flex-col items-end gap-1">
              {command.keys.map((keys) => (
                <Kbd key={keys} keys={keys} />
              ))}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

export interface ShortcutListProps extends HTMLAttributes<HTMLDivElement> {
  /** Where the group headings stand in the outline: `3` by default - under
   * the sheet's title, or a settings page's section. */
  level?: HeadingLevel
}

/** The list on its own, for a settings page that shows the keys in place. A
 * command with no keys has nothing to show and is left out. */
export function ShortcutList({ level = 3, className, ...props }: ShortcutListProps) {
  const groups = groupCommands(useCommandList().filter((command) => command.keys.length > 0))
  return (
    <div className={cn('gap-x-8 sm:columns-2', className)} {...props}>
      {groups.map(({ group, commands }) => (
        <Group key={group ?? ''} title={group} commands={commands} level={level} />
      ))}
    </div>
  )
}

export interface ShortcutsDialogProps {
  /** The heading, and the name of the command that opens it: in the palette
   * and in the list itself. */
  title: string
  /** What the close button is called, for a screen reader. No default: a word
   * the component invents is a word the product cannot translate. */
  closeLabel: string
  /** The line under the heading. */
  description?: ReactNode
  /** The keys that open it. */
  keys?: string
  /** The heading its own line is listed under. */
  group?: string
  /** Controlled, for a product that opens it from somewhere else as well. */
  open?: boolean
  /** Called with the new state whenever it opens or closes, whether by the
   * keys, `runCommand` or the close button. */
  onOpenChange?: (open: boolean) => void
  /** After the list: what keys cannot say - a gesture with the pointer, a
   * note that letters wait while someone is typing. */
  children?: ReactNode
}

export function ShortcutsDialog({
  title,
  closeLabel,
  description,
  keys = '?',
  group,
  open,
  onOpenChange,
  children,
}: ShortcutsDialogProps) {
  const [own, setOwn] = useState(false)
  const body = useRef<HTMLDivElement>(null)
  const shown = open ?? own
  const setShown = (next: boolean) => {
    if (open === undefined) setOwn(next)
    onOpenChange?.(next)
  }

  useCommand({ id: SHORTCUTS_COMMAND, label: title, group, keys, run: () => setShown(true) })

  return (
    <Dialog open={shown} onOpenChange={setShown}>
      <DialogPopup size="lg" initialFocus={body}>
        <DialogHeader
          action={
            <DialogClose render={<Button variant="icon" size="icon-sm" aria-label={closeLabel} />}>
              <CrossIcon />
            </DialogClose>
          }
        >
          <DialogTitle>{title}</DialogTitle>
          {description !== undefined && <DialogDescription>{description}</DialogDescription>}
        </DialogHeader>
        <DialogBody ref={body} tabIndex={-1}>
          {/* Clear of the ring the body draws inside its edge when it has the
              focus, which it has from the moment the sheet opens. */}
          <ShortcutList className="pt-1" />
          {children}
        </DialogBody>
      </DialogPopup>
    </Dialog>
  )
}

function CrossIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.75" aria-hidden>
      <path d="M4 4l8 8M12 4l-8 8" strokeLinecap="round" />
    </svg>
  )
}
