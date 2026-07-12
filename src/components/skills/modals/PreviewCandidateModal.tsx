import { memo } from 'react'
import type { TFunction } from 'i18next'
import type { GitSkillCandidate } from '../types'

type PreviewCandidateModalProps = {
  open: boolean
  candidates: GitSkillCandidate[]
  onRequestClose: () => void
  onSelect: (candidate: GitSkillCandidate) => void
  t: TFunction
}

const PreviewCandidateModal = ({
  open,
  candidates,
  onRequestClose,
  onSelect,
  t,
}: PreviewCandidateModalProps) => {
  if (!open) return null

  return (
    <div className="modal-backdrop" onClick={onRequestClose}>
      <div
        className="modal preview-candidate-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title">{t('skillPreview.chooseCandidate')}</div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            aria-label={t('close')}
          >
            ✕
          </button>
        </div>
        <div className="modal-body">
          <p className="label">{t('skillPreview.chooseCandidateBody')}</p>
          <div className="preview-candidate-list">
            {candidates.map((candidate) => (
              <button
                className="preview-candidate-item"
                type="button"
                key={candidate.subpath}
                onClick={() => onSelect(candidate)}
              >
                <strong>{candidate.name}</strong>
                {candidate.description ? <span>{candidate.description}</span> : null}
                <small>{candidate.subpath}</small>
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

export default memo(PreviewCandidateModal)
