import { Tooltip as Base } from '@base-ui/react/tooltip'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'dowel-ui'
import { usePopupContainer } from './layer'

/*
 * Tooltip.
 *
 * A label for a control that has no room for one - an icon button, a truncated
 * cell, a symbol whose meaning is not obvious. A phrase, not a panel.
 *
 * The line between this and Popover is not size, it is whether anything inside
 * can be reached. A tooltip holds no links, no buttons and no fields, because
 * there is no way to get into it: it is tied to a trigger that is hovered or
 * focused, and it disappears the moment attention moves. Content that has to
 * be clicked belongs in a Popover, whatever its length.
 *
 * The half everyone forgets is the keyboard. A tooltip that only opens on
 * hover is invisible to anyone who tabs, which is precisely the person reading
 * an unlabelled icon button. Base UI opens it on focus as well, and `Escape`
 * dismisses it - so it is tested here rather than assumed.
 *
 * The thing to know before using it: this is a VISUAL label, and nothing else.
 * Base UI puts no `role="tooltip"` on the popup and no `aria-describedby` on
 * the trigger, and that is deliberate rather than an oversight - a tooltip is
 * unreachable on a touch screen and unreliable for a screen reader, so
 * pretending otherwise would be worse than not trying. What follows from that
 * is a rule, not a suggestion: THE TRIGGER MUST CARRY ITS OWN `aria-label`,
 * saying roughly what the tooltip says. The tooltip helps a sighted mouse or
 * keyboard user; the `aria-label` is what everybody else gets.
 *
 * So if the words are load-bearing - if not reading them means not
 * understanding the control - this is the wrong component. Put them inline, or
 * in a Popover with `openOnHover` on the trigger, which touch and screen
 * readers can actually reach.
 *
 * `Provider` is optional and shared: once one tooltip in a group has opened,
 * the next opens instantly instead of waiting out its delay again. A toolbar
 * of icon buttons without it feels broken in a way nobody can name.
 */

export const tooltipPopupVariants = cva(
  [
    'rounded-md border border-line bg-raise px-2 py-1 text-xs text-text shadow-raise',
    'select-none',
    // The enter and the leave. `duration-*` reads the token directly because
    // Tailwind's own utility takes a literal number.
    '[transition:opacity_var(--duration-quick)_var(--ease-out),transform_var(--duration-quick)_var(--ease-out)]',
    'data-[closed]:scale-[0.96] data-[closed]:opacity-0',
    'data-[starting-style]:scale-[0.96] data-[starting-style]:opacity-0',
    // Grow out of the edge it is anchored to rather than out of its own
    // middle, so the motion points back at the trigger.
    'origin-[var(--transform-origin)]',
  ],
  {
    variants: {
      /*
       * Two, and no more. A tooltip is a phrase; the only real decision is
       * whether it is allowed to wrap.
       *
       * `wide` exists because the alternative people reach for is a Popover
       * that nothing can be clicked in, which loses the hover and focus
       * behaviour to gain a width.
       */
      size: {
        sm: 'max-w-64',
        wide: 'max-w-96',
      },
    },
    defaultVariants: { size: 'sm' },
  },
)

/** A shared delay for a group of tooltips. Wrap a toolbar in it and the second
 * icon button explains itself instantly rather than making the reader wait
 * again. Optional - a lone tooltip works without one. */
export const TooltipProvider = Base.Provider

/** The root. Takes `disabled`, and the controlled `open`/`onOpenChange`.
 *
 * Note that `delay` is NOT here - it is a prop of the trigger, the same as on
 * PreviewCard. Passing it to the root is silently ignored at runtime, which is
 * exactly the kind of mistake only the type checker catches. */
export const Tooltip = Base.Root

/** What it labels. Give it `render` to use your own button - and give that
 * button an `aria-label` saying what the tooltip says, because the tooltip
 * itself reaches nobody using a screen reader. */
export const TooltipTrigger = Base.Trigger

export interface TooltipPopupProps
  extends Base.Popup.Props,
    VariantProps<typeof tooltipPopupVariants> {
  /** Preferred side of the trigger. Base UI flips it when it does not fit,
   * and defaults it to the top. */
  side?: Base.Positioner.Props['side']
  /** Alignment along that side. Base UI centres it by default. */
  align?: Base.Positioner.Props['align']
  /** Distance from the trigger, in pixels. */
  sideOffset?: Base.Positioner.Props['sideOffset']
  /** Whether to draw the arrow pointing back at the trigger. */
  arrow?: boolean
  /** Where to portal to. Defaults to the raised host of the overlay this is
   * opened inside (`layer.tsx`), and to the document body when there is none -
   * either way not the element it was opened from, whose `overflow` would clip
   * it. Pass an element to put it somewhere else, such as a container being
   * screenshotted. */
  container?: Base.Portal.Props['container']
}

/** The label itself. Portalled and positioned, so it is not clipped by an
 * ancestor with `overflow: hidden`.
 *
 * `--z-popup` rather than `--z-floating`: a tooltip labels whatever is on top
 * of it, including the contents of a popover or a dialog, so it has to be able
 * to sit above them. */
export function TooltipPopup({
  size,
  side,
  align,
  sideOffset = 6,
  arrow = true,
  container,
  className,
  children,
  ...props
}: TooltipPopupProps) {
  // Inside an overlay, the overlay's raised host rather than the body - or
  // this popup draws under the dialog, drawer or popover that opened it. See
  // `layer.tsx`. Outside every overlay the hook gives `undefined`: the body.
  const host = usePopupContainer()

  return (
    <Base.Portal container={container ?? host}>
      <Base.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        className="[z-index:var(--z-popup)]"
      >
        <Base.Popup className={cn(tooltipPopupVariants({ size }), className)} {...props}>
          {arrow ? <TooltipArrow /> : null}
          {children}
        </Base.Popup>
      </Base.Positioner>
    </Base.Portal>
  )
}

/** The notch pointing back at the trigger. Base UI rotates it to whatever side
 * the tooltip landed on, which is why the placement is keyed off `data-side`
 * rather than off the `side` that was asked for. */
export function TooltipArrow({ className, ...props }: Base.Arrow.Props) {
  return (
    <Base.Arrow
      className={cn(
        'h-1.5 w-1.5 rotate-45 border border-line bg-raise',
        /* Two of the four borders are dropped per side, which Popover has done
         * all along and this did not.
         *
         * A square rotated 45 degrees shows two of its edges outside the
         * popup: the pair facing the trigger, which is the notch, and the
         * pair behind it, which is a stray line hanging off the far side. It
         * reads as a second arrow pointing the wrong way - and it is small
         * enough that it looked like a rendering artefact rather than a rule
         * nobody wrote. */
        'data-[side=bottom]:-top-hair data-[side=bottom]:border-r-0 data-[side=bottom]:border-b-0',
        'data-[side=top]:-bottom-hair data-[side=top]:border-t-0 data-[side=top]:border-l-0',
        'data-[side=left]:-right-hair data-[side=left]:border-b-0 data-[side=left]:border-l-0',
        'data-[side=right]:-left-hair data-[side=right]:border-r-0 data-[side=right]:border-t-0',
        className,
      )}
      {...props}
    />
  )
}
