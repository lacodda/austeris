import { Fragment, type HTMLAttributes, type ReactNode, type Ref } from 'react'
import { mergeProps } from '@base-ui/react/merge-props'
import { useRender } from '@base-ui/react/use-render'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'dowel-ui'
import { Tooltip, TooltipPopup, TooltipTrigger } from './tooltip'

/*
 * NavRail.
 *
 * The product's own destinations: the screens it has, drawn as a column down
 * the left, a row of tabs in a header, or a bar along the bottom of a phone.
 * One list, three shapes, because they are the same list - and when they were
 * three components in two products they drew the current entry three
 * different ways.
 *
 * The entries are the product's links, not this component's. `render` takes
 * an item and answers with the element to draw it as - a router's `NavLink`,
 * a plain `<a>`, whatever the product navigates with - and the rail puts the
 * clothes, the icon and the `aria-current` on it. That keeps the router out
 * of the registry entirely: a set that imported `react-router` would install
 * one in a product that had chosen another, and there is no version of that
 * which is the design system's business.
 *
 * `NavGroup` is the small uppercase caption between runs of entries, and
 * `NavSpacer` pushes what follows to the far end - the settings, the theme
 * switch, the profile, which every rail in the line keeps at its foot.
 *
 * **`collapsed` is the column with only its icons.** A work open on screen
 * wants the width, and the destinations are still needed - so the rail keeps
 * them as a column of icons at `--spacing-rail-compact`, and each one's name
 * moves into a tooltip beside it. The name does not leave the entry: it stays
 * in the accessibility tree, off the screen, so a reader hears "Catalogue,
 * current page" whether the rail is wide or narrow. The product passes the
 * same `collapsed` to every NavRail and NavGroup it assembles its rail from,
 * and sets the shell's rail width to the compact token.
 */

export const navRailVariants = cva('flex', {
  variants: {
    /*
     * Which of the three shapes this is.
     *
     * Not a guess from the width: a rail that changed shape when its
     * container narrowed would change shape in the middle of a drag.
     */
    layout: {
      /* Down the left, the desktop shape. `h-full` so the rail runs the
       * height of the window and its foot sits at the bottom because the nav
       * reaches it, not because the content does. */
      column: 'h-full flex-col gap-0.5 overflow-y-auto border-r border-line px-2.5 pt-2.5 pb-3',
      /* A row of tabs, in a header beside the product's name. */
      row: 'flex-row items-center gap-1',
      /* Along the bottom, where a thumb already is. The safe-area inset is
       * the phone's home indicator: without it the last row of entries sits
       * under it with no way to scroll out. */
      bar: 'flex-row border-t border-line bg-raise pb-[env(safe-area-inset-bottom)]',
    },
  },
  defaultVariants: { layout: 'column' },
})

export interface NavRailItem {
  id: string
  label: ReactNode
  icon?: ReactNode
  /** A count, a dot, a version chip - whatever sits at the end of the row.
   * Drawn only in `column`, where there is width for it. */
  end?: ReactNode
  /** A destination that exists on the roadmap but not in the build. It is
   * drawn greyed and is not a link: the point is to say the screen is coming,
   * which an entry that navigates nowhere would say by doing nothing. */
  soon?: boolean
}

export interface NavRailProps
  extends Omit<HTMLAttributes<HTMLElement>, 'children' | 'onSelect'>,
    VariantProps<typeof navRailVariants> {
  /** What this set of destinations is called. Not drawn - it names the
   * landmark, so a reader meeting two navs on one page can tell which is
   * which. A product with a rail AND a bottom bar has exactly that. */
  label: string
  items: readonly NavRailItem[]
  /** Which destination is the open screen. */
  activeId?: string
  /** The element an entry is drawn as - `render={(item) => <NavLink to={…} />}`. */
  render?: (item: NavRailItem) => useRender.RenderProp
  /** Pressed, whatever the entry is drawn as. */
  onSelect?: (id: string) => void
  /** Icons only, with each name in a tooltip. The column shape only - a row
   * of tabs and a bottom bar have no narrow form, because their names are
   * what they are made of. */
  collapsed?: boolean
}

export function NavRail({
  layout,
  label,
  items,
  activeId,
  render,
  onSelect,
  collapsed = false,
  className,
  ...props
}: NavRailProps) {
  const shape = layout ?? 'column'
  const narrow = collapsed && shape === 'column'

  return (
    <nav
      aria-label={label}
      className={cn(navRailVariants({ layout }), narrow && 'items-center px-2', className)}
      {...props}
    >
      {items.map((item) => {
        const entry = (
          <NavRailEntry
            item={item}
            layout={shape}
            collapsed={narrow}
            active={item.id === activeId}
            render={item.soon === true ? undefined : render?.(item)}
            onSelect={onSelect}
          />
        )
        if (!narrow) return <Fragment key={item.id}>{entry}</Fragment>
        // The name, beside the icon it no longer sits next to. The trigger is
        // the entry itself, so the tooltip opens on hover and on keyboard
        // focus alike - a name only the pointer can find is a name half the
        // rail's readers never get.
        return (
          <Tooltip key={item.id}>
            <TooltipTrigger render={entry} />
            <TooltipPopup side="right">{item.label}</TooltipPopup>
          </Tooltip>
        )
      })}
    </nav>
  )
}

/** The clothes of one entry, by shape.
 *
 * `target-min` on the two narrow shapes rather than on all three: a column
 * entry is already taller than the floor, and the utility establishes a
 * containing block, which is not free on every row of a list. */
const entryClasses = {
  column:
    'flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm no-underline transition-colors',
  /* `flex` here for the same reason as the other two, and it was missing: a
   * tab is an icon beside a word, and without it the entry is a block, the
   * icon and the label stack, and the header grows to twice its height. The
   * class-list tests did not see it - they read the string, not the layout -
   * and the stand did, in the first screenshot. */
  row: 'target-min flex items-center gap-2 rounded-md px-2.5 py-1 text-sm no-underline transition-colors',
  bar: 'target-min flex min-h-14 min-w-0 flex-1 flex-col items-center justify-center gap-1 px-1 py-2 text-2xs no-underline transition-colors',
} as const

/** A collapsed entry is a square around its icon, sized like an icon button
 * in the title bar so the two strips of chrome read as one set. */
const collapsedEntry = 'size-9 shrink-0 justify-center gap-0 p-0'

function NavRailEntry({
  item,
  layout,
  active,
  render,
  onSelect,
  collapsed = false,
  ref,
  ...trigger
}: {
  item: NavRailItem
  layout: NonNullable<NavRailProps['layout']>
  active: boolean
  render?: useRender.RenderProp
  onSelect?: (id: string) => void
  collapsed?: boolean
  ref?: Ref<HTMLElement>
} & Omit<HTMLAttributes<HTMLElement>, 'onSelect'>) {
  const soon = item.soon === true

  return useRender({
    render,
    ref,
    // A destination is a link when the product gives it one and a button when
    // it does not; one that is not built yet is neither, and `span` says so -
    // a disabled button is still in the tab order on some browsers, and
    // tabbing onto a screen that does not exist is a dead end.
    defaultTagName: soon ? 'span' : 'button',
    // What a tooltip trigger hands down when the entry is wrapped in one -
    // its hover and focus handlers - merged under the entry's own, so the
    // entry's click and clothes are never overridden by the wrapper's.
    props: mergeProps<'button'>(trigger, {
      ...(render === undefined && !soon ? { type: 'button' } : {}),
      // Named as the current page, which is how a reader learns which of six
      // identical links is where they stand. Colour alone says it to nobody
      // who cannot see it.
      'aria-current': active ? 'page' : undefined,
      ...(soon ? { 'aria-disabled': true } : {}),
      onClick: soon ? undefined : () => onSelect?.(item.id),
      className: cn(
        entryClasses[layout],
        collapsed && collapsedEntry,
        '[&_svg:not([class*=size-])]:size-4 [&_svg]:shrink-0',
        'focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent',
        soon
          ? 'cursor-default text-faint'
          : cn(
              'cursor-pointer text-dim hover:bg-soft hover:text-text',
              // The bottom bar tints the entry itself rather than filling the
              // cell: a filled cell in a four-across bar reads as a pressed
              // button, and it is a location, not an action.
              active &&
                (layout === 'bar'
                  ? 'text-accent-2 hover:bg-transparent'
                  : 'bg-accent-soft text-text [&_svg]:text-accent'),
            ),
      ),
      children: (
        <>
          {item.icon !== undefined ? (
            <span aria-hidden className="contents">
              {item.icon}
            </span>
          ) : collapsed ? (
            // Collapsed, an entry with no icon would be an empty square - a
            // button nobody can see. Its initial stands in, the way an avatar
            // stands in for a face; the name is still the entry's, off screen.
            <span aria-hidden className="text-xs font-semibold uppercase">
              {typeof item.label === 'string' ? item.label.charAt(0) : '•'}
            </span>
          ) : null}
          {/* Truncated, in one line. A rail is a fixed-width column, and a
              label that wraps onto a second line makes the row taller than
              every other - which in a two-language product happens to one
              entry and not the rest. `min-w-0` because a flex child refuses
              to shrink below its content without it. */}
          <span
            className={cn(
              'min-w-0 truncate',
              layout === 'bar' ? 'w-full text-center' : 'flex-1',
              // Off the screen, still the entry's name.
              collapsed && 'sr-only',
            )}
          >
            {item.label}
          </span>
          {item.end === undefined || layout !== 'column' || collapsed ? null : (
            <span className="ml-auto shrink-0 text-2xs text-faint">{item.end}</span>
          )}
        </>
      ),
    }),
  })
}

/** The caption over a run of entries: Library, Team, Admin.
 *
 * A `div` rather than a heading: a rail's groups are not the page's outline,
 * and six `h3`s inside a nav put six entries into a reader's document map
 * that lead nowhere.
 *
 * Collapsed, a caption has no room, and a word cut to three letters is worse
 * than none - so the run is marked by a short hairline instead, and the word
 * stays for a reader. */
export function NavGroup({
  collapsed = false,
  className,
  children,
  ...props
}: HTMLAttributes<HTMLDivElement> & { collapsed?: boolean }) {
  if (collapsed) {
    return (
      <div className={cn('mx-auto my-2 h-px w-5 shrink-0 bg-line', className)} {...props}>
        <span className="sr-only">{children}</span>
      </div>
    )
  }

  return (
    <div className={cn('caption px-2.5 pt-3 pb-1', className)} {...props}>
      {children}
    </div>
  )
}

/** Pushes everything after it to the far end of the rail. */
export function NavSpacer({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('mt-auto flex flex-col gap-0.5', className)} {...props} />
}
