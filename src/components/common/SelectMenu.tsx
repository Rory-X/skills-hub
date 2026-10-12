import {
  memo,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { Check, ChevronDown } from 'lucide-react'

export type SelectMenuOption = {
  value: string
  label: string
  disabled?: boolean
}

type SelectMenuProps = {
  value: string
  options: readonly SelectMenuOption[]
  onChange: (value: string) => void
  ariaLabel: string
  className?: string
  disabled?: boolean
  id?: string
  leadingIcon?: ReactNode
  menuAlign?: 'start' | 'end'
  menuMinWidth?: number
  placeholder?: string
}

const SelectMenu = ({
  value,
  options,
  onChange,
  ariaLabel,
  className = '',
  disabled = false,
  id,
  leadingIcon,
  menuAlign = 'start',
  menuMinWidth = 180,
  placeholder = '',
}: SelectMenuProps) => {
  const generatedId = useId()
  const listboxId = `${generatedId}-listbox`
  const rootRef = useRef<HTMLDivElement | null>(null)
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([])
  const [open, setOpen] = useState(false)
  const [activeIndex, setActiveIndex] = useState(-1)
  const [menuStyle, setMenuStyle] = useState<CSSProperties | null>(null)

  const selectedIndex = options.findIndex((option) => option.value === value)
  const selectedOption = selectedIndex >= 0 ? options[selectedIndex] : null
  const enabledIndexes = useMemo(
    () =>
      options.flatMap((option, index) =>
        option.disabled ? [] : [index],
      ),
    [options],
  )

  const closeMenu = useCallback((restoreFocus = false) => {
    setOpen(false)
    setMenuStyle(null)
    if (restoreFocus) {
      window.requestAnimationFrame(() => triggerRef.current?.focus())
    }
  }, [])

  const openMenu = (preferredIndex?: number) => {
    if (disabled || enabledIndexes.length === 0) return
    const nextIndex =
      preferredIndex !== undefined && enabledIndexes.includes(preferredIndex)
        ? preferredIndex
        : enabledIndexes.includes(selectedIndex)
          ? selectedIndex
          : enabledIndexes[0]
    setActiveIndex(nextIndex)
    setOpen(true)
  }

  const moveActiveOption = (offset: number) => {
    if (enabledIndexes.length === 0) return
    const currentPosition = enabledIndexes.indexOf(activeIndex)
    const basePosition = currentPosition >= 0 ? currentPosition : 0
    const nextPosition =
      (basePosition + offset + enabledIndexes.length) % enabledIndexes.length
    setActiveIndex(enabledIndexes[nextPosition])
  }

  const chooseOption = (index: number) => {
    const option = options[index]
    if (!option || option.disabled) return
    if (option.value !== value) onChange(option.value)
    closeMenu(true)
  }

  const handleTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      openMenu()
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      openMenu(enabledIndexes.at(-1))
    } else if (event.key === 'Home') {
      event.preventDefault()
      openMenu(enabledIndexes[0])
    } else if (event.key === 'End') {
      event.preventDefault()
      openMenu(enabledIndexes.at(-1))
    } else if (event.key === 'Escape' && open) {
      event.preventDefault()
      closeMenu()
    }
  }

  const handleOptionKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveActiveOption(1)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveActiveOption(-1)
    } else if (event.key === 'Home') {
      event.preventDefault()
      setActiveIndex(enabledIndexes[0] ?? index)
    } else if (event.key === 'End') {
      event.preventDefault()
      setActiveIndex(enabledIndexes.at(-1) ?? index)
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      chooseOption(index)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      closeMenu(true)
    } else if (event.key === 'Tab') {
      closeMenu()
    }
  }

  useLayoutEffect(() => {
    if (!open) return
    const updatePosition = () => {
      const trigger = triggerRef.current
      if (!trigger) return
      const rect = trigger.getBoundingClientRect()
      const viewportMargin = 8
      const gap = 4
      const availableWidth = Math.max(0, window.innerWidth - viewportMargin * 2)
      const width = Math.min(Math.max(rect.width, menuMinWidth), availableWidth)
      const alignedLeft = menuAlign === 'end' ? rect.right - width : rect.left
      const left = Math.min(
        Math.max(alignedLeft, viewportMargin),
        Math.max(viewportMargin, window.innerWidth - width - viewportMargin),
      )
      const spaceBelow = window.innerHeight - rect.bottom - viewportMargin - gap
      const spaceAbove = rect.top - viewportMargin - gap
      const placeAbove = spaceBelow < 160 && spaceAbove > spaceBelow
      const availableHeight = Math.max(88, placeAbove ? spaceAbove : spaceBelow)
      const maxHeight = Math.min(280, availableHeight)
      setMenuStyle({
        left,
        width,
        maxHeight,
        ...(placeAbove
          ? { bottom: window.innerHeight - rect.top + gap }
          : { top: rect.bottom + gap }),
      })
    }

    updatePosition()
    window.addEventListener('resize', updatePosition)
    document.addEventListener('scroll', updatePosition, true)
    return () => {
      window.removeEventListener('resize', updatePosition)
      document.removeEventListener('scroll', updatePosition, true)
    }
  }, [menuAlign, menuMinWidth, open])

  useEffect(() => {
    if (!open || !menuStyle || activeIndex < 0) return
    optionRefs.current[activeIndex]?.focus()
  }, [activeIndex, menuStyle, open])

  useEffect(() => {
    if (!open) return
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (
        !rootRef.current?.contains(target) &&
        !menuRef.current?.contains(target)
      ) {
        closeMenu()
      }
    }
    document.addEventListener('mousedown', handlePointerDown)
    return () => document.removeEventListener('mousedown', handlePointerDown)
  }, [closeMenu, open])

  useEffect(() => {
    if (!disabled || !open) return
    const frame = window.requestAnimationFrame(() => closeMenu())
    return () => window.cancelAnimationFrame(frame)
  }, [closeMenu, disabled, open])

  return (
    <div
      className={`select-menu${open ? ' open' : ''}${className ? ` ${className}` : ''}`}
      ref={rootRef}
    >
      <button
        className="select-menu-trigger"
        id={id}
        ref={triggerRef}
        type="button"
        role="combobox"
        aria-label={ariaLabel}
        aria-controls={listboxId}
        aria-expanded={open}
        aria-haspopup="listbox"
        disabled={disabled}
        onClick={() => (open ? closeMenu() : openMenu())}
        onKeyDown={handleTriggerKeyDown}
      >
        {leadingIcon ? (
          <span className="select-menu-leading" aria-hidden="true">
            {leadingIcon}
          </span>
        ) : null}
        <span className="select-menu-value" title={selectedOption?.label}>
          {selectedOption?.label ?? placeholder}
        </span>
        <ChevronDown className="select-menu-chevron" size={15} aria-hidden="true" />
      </button>

      {open && menuStyle
        ? createPortal(
            <div
              className="select-menu-popover"
              id={listboxId}
              ref={menuRef}
              role="listbox"
              aria-label={ariaLabel}
              style={menuStyle}
            >
              {options.map((option, index) => (
                <button
                  className={`select-menu-option${option.value === value ? ' selected' : ''}`}
                  key={option.value}
                  ref={(element) => {
                    optionRefs.current[index] = element
                  }}
                  type="button"
                  role="option"
                  aria-selected={option.value === value}
                  tabIndex={index === activeIndex ? 0 : -1}
                  disabled={option.disabled}
                  title={option.label}
                  onClick={() => chooseOption(index)}
                  onFocus={() => setActiveIndex(index)}
                  onMouseEnter={() => {
                    if (!option.disabled) setActiveIndex(index)
                  }}
                  onKeyDown={(event) => handleOptionKeyDown(event, index)}
                >
                  <span className="select-menu-check" aria-hidden="true">
                    {option.value === value ? <Check size={14} /> : null}
                  </span>
                  <span>{option.label}</span>
                </button>
              ))}
            </div>,
            document.body,
          )
        : null}
    </div>
  )
}

export default memo(SelectMenu)
