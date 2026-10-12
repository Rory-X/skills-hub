import { memo } from 'react'
import { TriangleAlert } from 'lucide-react'
import type { TFunction } from 'i18next'

type DeleteModalProps = {
  open: boolean
  loading: boolean
  skillName: string | null
  title?: string
  description?: string
  warnings?: string[]
  confirmLabel?: string
  onRequestClose: () => void
  onConfirm: () => void
  t: TFunction
}

const DeleteModal = ({
  open,
  loading,
  skillName,
  title,
  description,
  warnings,
  confirmLabel,
  onRequestClose,
  onConfirm,
  t,
}: DeleteModalProps) => {
  if (!open) return null

  const warningItems = warnings ?? [
    t('delete.warningRemoveFromTools'),
    t('delete.warningDeleteFromHub'),
  ]

  return (
    <div
      className="modal-backdrop"
      onClick={loading ? undefined : onRequestClose}
    >
      <div
        className="modal modal-delete"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className="modal-body delete-body">
          <div className="delete-title">
            <TriangleAlert size={20} />
            {title ?? t('deleteTitle')}
          </div>
          <div className="delete-desc">
            {description ?? (skillName ? (
              <>
                {t('delete.confirmPrefix')}
                <strong>{skillName}</strong>
                {t('delete.confirmSuffix')}
              </>
            ) : (
              t('deleteBody')
            ))}
          </div>
          <div className="delete-warning">
            <ul>
              {warningItems.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </div>
        </div>
        <div className="modal-footer space-between">
          <button
            className="btn btn-secondary"
            onClick={onRequestClose}
            disabled={loading}
          >
            {t('cancel')}
          </button>
          <button
            className="btn btn-danger-solid"
            onClick={onConfirm}
            disabled={loading}
          >
            {confirmLabel ?? t('delete.confirmButton')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(DeleteModal)
