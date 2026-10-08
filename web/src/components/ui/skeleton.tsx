import type { HTMLAttributes } from 'react'
import { cn } from 'dowel-ui'

/*
 * A placeholder shaped like the thing that is still loading.
 *
 * The rule the component is built on, and the reason it takes a shape rather
 * than filling the space:
 *
 *   **A skeleton of the wrong shape is worse than no skeleton.**
 *
 * It promises something the content does not keep, and the promise is paid for
 * in a jump: the page settles, the scrollbar appears, and whatever the reader
 * was about to click has moved. Measured rather than assumed - the line's own
 * calendar showed a list of four short lines where a six-row month grid was
 * about to land, and the skeleton was itself the jump it existed to prevent.
 *
 * So the useful thing here is not `<Skeleton />` - that is four lines anyone
 * can write - but the shapes: a list, a card, a grid. They are what a product
 * reaches for at the call site, and what keeps the placeholder honest.
 *
 * A spinner is the right answer when the shape is *not* known. A skeleton
 * claims to know; if it does not, say less rather than more.
 *
 * Everything here is `aria-hidden`. A screen reader is told the region is busy
 * by whatever owns the loading state - `QueryState` does it with `aria-busy` -
 * and announcing a dozen empty boxes as content would be noise on top of a
 * fact the reader already has.
 */

export type SkeletonProps = HTMLAttributes<HTMLDivElement>

/** One block. Size it with `className` - a skeleton is a shape, and the shape
 * belongs to whatever it stands in for. */
export function Skeleton({ className, ...props }: SkeletonProps) {
  return (
    <div
      aria-hidden
      className={cn('animate-pulse rounded-md bg-soft', className)}
      {...props}
    />
  )
}

export interface SkeletonTextProps extends HTMLAttributes<HTMLDivElement> {
  /** How many lines of text stand here. */
  lines?: number
}

/** A paragraph's worth of lines.
 *
 * The widths vary, and that is the whole point: a stack of equal bars reads as
 * a loading indicator, while ragged ones read as text. The last line is short,
 * because the last line of a paragraph is. */
export function SkeletonText({ lines = 3, className, ...props }: SkeletonTextProps) {
  const widths = ['w-full', 'w-11/12', 'w-4/5', 'w-full', 'w-3/4']
  return (
    <div aria-hidden className={cn('flex flex-col gap-2', className)} {...props}>
      {Array.from({ length: Math.max(1, lines) }, (_, line) => (
        <Skeleton
          key={line}
          className={cn(
            'h-3',
            line === lines - 1 ? 'w-1/2' : widths[line % widths.length],
          )}
        />
      ))}
    </div>
  )
}

export interface SkeletonListProps extends HTMLAttributes<HTMLDivElement> {
  rows?: number
  /** Draw a second, shorter line under each row - for a list whose rows carry
   * a title and something beneath it. */
  secondary?: boolean
}

/** Rows of a list, which is the shape most screens are waiting for. */
export function SkeletonList({
  rows = 5,
  secondary = true,
  className,
  ...props
}: SkeletonListProps) {
  // Three widths in rotation rather than random: a placeholder that differs
  // between renders is a placeholder that flickers when anything re-renders.
  const widths = ['w-2/3', 'w-4/5', 'w-1/2']
  return (
    <div aria-hidden className={cn('flex flex-col gap-1', className)} {...props}>
      {Array.from({ length: Math.max(1, rows) }, (_, row) => (
        <div key={row} className="flex flex-col gap-1.5 px-3 py-2">
          <Skeleton className={cn('h-3.5', widths[row % widths.length])} />
          {secondary && <Skeleton className="h-2.5 w-1/3" />}
        </div>
      ))}
    </div>
  )
}

export interface SkeletonGridProps extends HTMLAttributes<HTMLDivElement> {
  /** How many cells. */
  cells?: number
  /** How many per row. */
  columns?: number
  /** The aspect of one cell, as a Tailwind class - a gallery of covers is not
   * shaped like a grid of tiles. */
  cellClassName?: string
}

/** A grid of cells: a gallery, a board, a month.
 *
 * Given a cell count and a column count rather than a shape of its own,
 * because the grids a product waits for differ in both and agree in neither. */
export function SkeletonGrid({
  cells = 12,
  columns = 4,
  cellClassName = 'aspect-square',
  className,
  ...props
}: SkeletonGridProps) {
  return (
    <div
      aria-hidden
      // The column count is a style rather than a class, because a class would
      // have to be one of a fixed set - and Tailwind cannot generate
      // `grid-cols-${n}` from a value it never sees.
      style={{ gridTemplateColumns: `repeat(${Math.max(1, columns)}, minmax(0, 1fr))` }}
      className={cn('grid gap-2', className)}
      {...props}
    >
      {Array.from({ length: Math.max(1, cells) }, (_, cell) => (
        <Skeleton key={cell} className={cellClassName} />
      ))}
    </div>
  )
}
