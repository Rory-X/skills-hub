import {
  memo,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'

type TooltipProps = {
  content: string
  children: ReactNode
  className?: string
}

const Tooltip = ({ content, children, className = '' }: TooltipProps) => {
  const tooltipId = useId()
  const anchorRef = useRef<HTMLSpanElement | null>(null)
  const tooltipRef = useRef<HTMLSpanElement | null>(null)
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<CSSProperties | null>(null)

  const showTooltip = () => {
    if (content.trim()) setOpen(true)
  }

  const hideTooltip = () => {
    setOpen(false)
    setPosition(null)
  }

  const handleBlur = (event: FocusEvent<HTMLSpanElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      hideTooltip()
    }
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLSpanElement>) => {
    if (event.key !== 'Escape') return
    event.stopPropagation()
    hideTooltip()
  }

  useLayoutEffect(() => {
    if (!open) return
    const updatePosition = () => {
      const anchor = anchorRef.current
      const tooltip = tooltipRef.current
      if (!anchor || !tooltip) return
      const anchorRect = anchor.getBoundingClientRect()
      const tooltipRect = tooltip.getBoundingClientRect()
      const viewportMargin = 8
      const gap = 6
      const idealLeft =
        anchorRect.left + (anchorRect.width - tooltipRect.width) / 2
      const left = Math.min(
        Math.max(idealLeft, viewportMargin),
        Math.max(
          viewportMargin,
          window.innerWidth - tooltipRect.width - viewportMargin,
        ),
      )
      const spaceAbove = anchorRect.top - viewportMargin
      const spaceBelow = window.innerHeight - anchorRect.bottom - viewportMargin
      const placeAbove =
        spaceAbove >= tooltipRect.height + gap || spaceAbove >= spaceBelow
      const top = placeAbove
        ? Math.max(viewportMargin, anchorRect.top - tooltipRect.height - gap)
        : Math.min(
            window.innerHeight - tooltipRect.height - viewportMargin,
            anchorRect.bottom + gap,
          )
      setPosition({ left, top })
    }

    updatePosition()
    window.addEventListener('resize', updatePosition)
    document.addEventListener('scroll', updatePosition, true)
    return () => {
      window.removeEventListener('resize', updatePosition)
      document.removeEventListener('scroll', updatePosition, true)
    }
  }, [content, open])

  return (
    <span
      className={`tooltip-anchor${className ? ` ${className}` : ''}`}
      ref={anchorRef}
      onMouseEnter={showTooltip}
      onMouseLeave={hideTooltip}
      onFocusCapture={showTooltip}
      onBlurCapture={handleBlur}
      onKeyDown={handleKeyDown}
    >
      {children}
      {open
        ? createPortal(
            <span
              className="tooltip-popover"
              id={tooltipId}
              ref={tooltipRef}
              role="tooltip"
              style={position ?? { left: 0, top: 0, visibility: 'hidden' }}
            >
              {content}
            </span>,
            document.body,
          )
        : null}
    </span>
  )
}

export default memo(Tooltip)
