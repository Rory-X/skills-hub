import { memo } from 'react'
import { FileClock, RotateCcw, X } from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  PromptRevisionDocumentDto,
  PromptRevisionSummaryDto,
} from '../skills/types'

type PromptHistoryModalProps = {
  open: boolean
  revisions: PromptRevisionSummaryDto[]
  selectedRevisionId: string
  revisionDocument: PromptRevisionDocumentDto | null
  loading: boolean
  onSelect: (revisionId: string) => void
  onRestore: () => void
  onRequestClose: () => void
  formatTimestamp: (value?: number | null) => string
  t: TFunction
}

const PromptHistoryModal = ({
  open,
  revisions,
  selectedRevisionId,
  revisionDocument,
  loading,
  onSelect,
  onRestore,
  onRequestClose,
  formatTimestamp,
  t,
}: PromptHistoryModalProps) => {
  if (!open) return null
  return (
    <div className="modal-backdrop" onClick={loading ? undefined : onRequestClose}>
      <div className="modal prompt-history-modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal-header">
          <div>
            <div className="modal-title">{t('prompts.historyModal.title')}</div>
            <p>{t('prompts.historyModal.help')}</p>
          </div>
          <button className="modal-close" type="button" onClick={onRequestClose} disabled={loading} aria-label={t('close')}>
            <X size={18} />
          </button>
        </div>
        <div className="prompt-history-layout">
          <div className="prompt-history-list">
            {revisions.length === 0 ? (
              <div className="prompt-profile-empty">{t('prompts.historyModal.empty')}</div>
            ) : (
              revisions.map((revision) => (
                <button
                  className={`prompt-history-item${selectedRevisionId === revision.id ? ' active' : ''}`}
                  type="button"
                  key={revision.id}
                  onClick={() => onSelect(revision.id)}
                >
                  <FileClock size={15} />
                  <span>
                    <strong>{t(`prompts.operations.${revision.operation}`, { defaultValue: revision.operation })}</strong>
                    <small>{formatTimestamp(revision.created_at)}</small>
                  </span>
                  <code>{t('prompts.bytes', { count: revision.byte_len })}</code>
                </button>
              ))
            )}
          </div>
          <div className="prompt-history-preview">
            {revisionDocument ? (
              <>
                <div className="prompt-history-preview-head">
                  <code>{revisionDocument.summary.content_hash.slice(0, 12)}</code>
                  <span>{revisionDocument.line_ending.toUpperCase()} · {revisionDocument.encoding}</span>
                </div>
                <pre>{revisionDocument.content}</pre>
              </>
            ) : (
              <div className="prompt-editor-empty">{loading ? t('prompts.loading') : t('prompts.historyModal.select')}</div>
            )}
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" type="button" onClick={onRequestClose} disabled={loading}>
            {t('close')}
          </button>
          <button className="btn btn-primary" type="button" onClick={onRestore} disabled={loading || !revisionDocument}>
            <RotateCcw size={15} />
            {t('prompts.historyModal.restore')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(PromptHistoryModal)
