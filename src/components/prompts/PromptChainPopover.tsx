import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { createPortal } from 'react-dom'
import {
  AlertTriangle,
  ChevronRight,
  ListTree,
  LoaderCircle,
  X,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  PromptInstructionChainDto,
  PromptSourceDto,
} from '../skills/types'

type PromptChainPopoverProps = {
  chain: PromptInstructionChainDto | null
  loading: boolean
  disabled: boolean
  contextLabel: string
  onOpen: () => void
  onSelectSource: (source: PromptSourceDto) => void
  t: TFunction
}

const PromptChainPopover = ({
  chain,
  loading,
  disabled,
  contextLabel,
  onOpen,
  onSelectSource,
  t,
}: PromptChainPopoverProps) => {
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const popoverRef = useRef<HTMLDivElement | null>(null)
  const [open, setOpen] = useState(false)
  const [popoverStyle, setPopoverStyle] = useState<CSSProperties | null>(null)

  const closePopover = useCallback((restoreFocus = false) => {
    setOpen(false)
    setPopoverStyle(null)
    if (restoreFocus) {
      window.requestAnimationFrame(() => triggerRef.current?.focus())
    }
  }, [])

  const togglePopover = () => {
    if (open) {
      closePopover()
      return
    }
    if (disabled) return
    setOpen(true)
    onOpen()
  }

  useLayoutEffect(() => {
    if (!open) return
    const updatePosition = () => {
      const trigger = triggerRef.current
      if (!trigger) return
      const rect = trigger.getBoundingClientRect()
      const viewportMargin = 8
      const gap = 6
      const availableWidth = Math.max(0, window.innerWidth - viewportMargin * 2)
      const width = Math.min(460, availableWidth)
      const left = Math.min(
        Math.max(rect.left, viewportMargin),
        Math.max(viewportMargin, window.innerWidth - width - viewportMargin),
      )
      const spaceBelow = window.innerHeight - rect.bottom - viewportMargin - gap
      const spaceAbove = rect.top - viewportMargin - gap
      const placeAbove = spaceBelow < 260 && spaceAbove > spaceBelow
      const availableHeight = Math.max(160, placeAbove ? spaceAbove : spaceBelow)
      setPopoverStyle({
        left,
        width,
        maxHeight: Math.min(520, availableHeight),
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
  }, [open])

  useEffect(() => {
    if (!open) return
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (
        !triggerRef.current?.contains(target) &&
        !popoverRef.current?.contains(target)
      ) {
        closePopover()
      }
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePopover(true)
    }
    document.addEventListener('mousedown', handlePointerDown)
    window.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [closePopover, open])

  const startupSources = chain?.startup_sources ?? []

  return (
    <div className="prompt-chain-launcher">
      <button
        className="prompt-chain-trigger"
        ref={triggerRef}
        type="button"
        onClick={togglePopover}
        disabled={disabled}
        aria-expanded={open}
        aria-haspopup="dialog"
      >
        <span className="prompt-chain-trigger-icon" aria-hidden="true">
          <ListTree size={16} />
        </span>
        <span className="prompt-chain-trigger-copy">
          <strong>{t('prompts.sessionChain')}</strong>
          <span>{contextLabel}</span>
        </span>
        <span className="prompt-chain-trigger-tail">
          {chain ? startupSources.length : null}
          <ChevronRight size={15} aria-hidden="true" />
        </span>
      </button>

      {open && popoverStyle
        ? createPortal(
            <div
              className="prompt-chain-popover"
              ref={popoverRef}
              role="dialog"
              aria-label={t('prompts.sessionChain')}
              style={popoverStyle}
            >
              <div className="prompt-chain-popover-header">
                <div>
                  <strong>{t('prompts.sessionChain')}</strong>
                  <span>{contextLabel}</span>
                </div>
                <button
                  className="icon-btn"
                  type="button"
                  onClick={() => closePopover(true)}
                  aria-label={t('close')}
                >
                  <X size={16} />
                </button>
              </div>
              <div className="prompt-chain-popover-body">
                {loading ? (
                  <div className="prompt-chain-empty">
                    <LoaderCircle size={18} className="spin" />
                    <span>{t('prompts.resolving')}</span>
                  </div>
                ) : chain ? (
                  <>
                    <div className="prompt-chain-popover-summary">
                      {t('prompts.startupSources', {
                        count: startupSources.length,
                      })}
                    </div>
                    {startupSources.length > 0 ? (
                      <div className="prompt-chain-popover-list">
                        {startupSources.map((item) => (
                          <button
                            className="prompt-chain-popover-item"
                            type="button"
                            key={`${item.order}-${item.source.path}`}
                            onClick={() => {
                              onSelectSource(item.source)
                              closePopover()
                            }}
                          >
                            <span className="prompt-chain-order">
                              {item.order + 1}
                            </span>
                            <span className="prompt-chain-main">
                              <span className="prompt-chain-title-row">
                                <strong title={item.source.path}>
                                  {item.source.display_path}
                                </strong>
                                <span>
                                  {t(`prompts.kinds.${item.source.kind}`)}
                                </span>
                              </span>
                              <code title={item.source.path}>
                                {item.source.path}
                              </code>
                              <span className="prompt-chain-meta">
                                <span>
                                  {t(`prompts.scopes.${item.source.scope}`)}
                                </span>
                                <span>
                                  {t('prompts.bytesLoaded', {
                                    count: item.loaded_bytes,
                                  })}
                                </span>
                                {item.imported_from ? (
                                  <span>{t('prompts.imported')}</span>
                                ) : null}
                                {item.truncated ? (
                                  <span className="warning">
                                    {t('prompts.truncated')}
                                  </span>
                                ) : null}
                              </span>
                            </span>
                          </button>
                        ))}
                      </div>
                    ) : (
                      <div className="prompt-chain-empty">
                        {t('prompts.noEffectiveSources')}
                      </div>
                    )}
                    {chain.warnings.length > 0 ? (
                      <div className="prompt-chain-warnings">
                        {chain.warnings.map((warning) => (
                          <span key={warning}>
                            <AlertTriangle size={14} />
                            {warning}
                          </span>
                        ))}
                      </div>
                    ) : null}
                  </>
                ) : (
                  <div className="prompt-chain-empty">
                    {t('prompts.resolveHint')}
                  </div>
                )}
              </div>
            </div>,
            document.body,
          )
        : null}
    </div>
  )
}

export default memo(PromptChainPopover)
