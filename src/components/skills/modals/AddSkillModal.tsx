import { memo } from 'react'
import { Check, FolderSearch } from 'lucide-react'
import type { TFunction } from 'i18next'
import type { TagWithCountDto } from '../types'

type AddSkillModalProps = {
  open: boolean
  loading: boolean
  canClose: boolean
  addModalTab: 'local' | 'git'
  localPath: string
  localName: string
  gitUrl: string
  gitName: string
  tags: TagWithCountDto[]
  selectedTagIds: number[]
  reimportSkillName?: string | null
  onRequestClose: () => void
  onTabChange: (tab: 'local' | 'git') => void
  onLocalPathChange: (value: string) => void
  onPickLocalPath: () => void
  onLocalNameChange: (value: string) => void
  onGitUrlChange: (value: string) => void
  onGitNameChange: (value: string) => void
  onToggleTag: (tagId: number) => void
  onSubmit: () => void
  t: TFunction
}

const AddSkillModal = ({
  open,
  loading,
  canClose,
  addModalTab,
  localPath,
  localName,
  gitUrl,
  gitName,
  tags,
  selectedTagIds,
  reimportSkillName,
  onRequestClose,
  onTabChange,
  onLocalPathChange,
  onPickLocalPath,
  onLocalNameChange,
  onGitUrlChange,
  onGitNameChange,
  onToggleTag,
  onSubmit,
  t,
}: AddSkillModalProps) => {
  if (!open) return null

  return (
    <div
      className="modal-backdrop"
      onClick={() => (canClose ? onRequestClose() : null)}
    >
      <div className="modal add-skill-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title">
            {reimportSkillName
              ? t('assignment.reimportTitle', { name: reimportSkillName })
              : t('addSkillTitle')}
          </div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            aria-label={t('close')}
            disabled={!canClose}
          >
            ✕
          </button>
        </div>
        <div className="modal-body">
          <div className="tabs">
            <button
              className={`tab-item${addModalTab === 'local' ? ' active' : ''}`}
              type="button"
              onClick={() => onTabChange('local')}
            >
              {t('localTab')}
            </button>
            <button
              className={`tab-item${addModalTab === 'git' ? ' active' : ''}`}
              type="button"
              onClick={() => onTabChange('git')}
            >
              {t('gitTab')}
            </button>
          </div>

          {addModalTab === 'local' ? (
            <>
              {!reimportSkillName ? (
                <div className="local-scan-hint">
                  <FolderSearch size={18} />
                  <div>
                    <strong>{t('localScanTitle')}</strong>
                    <span>{t('localScanBody')}</span>
                  </div>
                </div>
              ) : null}
              <div className="form-group form-row-inline">
                <div className="form-field">
                  <label className="label">{t('localFolder')}</label>
                  <div className="input-row">
                    <input
                      className="input"
                      placeholder={t('localPathPlaceholder')}
                      value={localPath}
                      onChange={(event) => onLocalPathChange(event.target.value)}
                    />
                    <button
                      className="btn btn-secondary input-action"
                      type="button"
                      onClick={onPickLocalPath}
                      disabled={!canClose}
                    >
                      {t('browse')}
                    </button>
                  </div>
                </div>
                {!reimportSkillName ? (
                  <div className="form-field">
                    <label className="label">{t('localCustomName')}</label>
                    <input
                      className="input"
                      placeholder={t('localCustomNamePlaceholder')}
                      value={localName}
                      onChange={(event) => onLocalNameChange(event.target.value)}
                    />
                  </div>
                ) : null}
              </div>
            </>
          ) : (
            <>
              <div className="form-group form-row-inline">
                <div className="form-field">
                  <label className="label">{t('repositoryUrl')}</label>
                  <input
                    className="input"
                    placeholder={t('gitUrlPlaceholder')}
                    value={gitUrl}
                    onChange={(event) => onGitUrlChange(event.target.value)}
                  />
                </div>
                {!reimportSkillName ? (
                  <div className="form-field">
                    <label className="label">{t('optionalNamePlaceholder')}</label>
                    <input
                      className="input"
                      placeholder={t('optionalNamePlaceholder')}
                      value={gitName}
                      onChange={(event) => onGitNameChange(event.target.value)}
                    />
                  </div>
                ) : null}
              </div>
            </>
          )}

          {!reimportSkillName ? (
            <div className="form-group tag-field-inline">
              <label className="label">{t('addTags')}</label>
              {tags.length > 0 ? (
                <div className="add-tags-list">
                  {tags.map((tag) => {
                    const selected = selectedTagIds.includes(tag.id)
                    return (
                      <button
                        key={tag.id}
                        className={`add-tag-pill${selected ? ' selected' : ''}`}
                        type="button"
                        onClick={() => onToggleTag(tag.id)}
                      >
                        <span className="add-tag-check">
                          {selected ? <Check size={12} /> : null}
                        </span>
                        <span>#{tag.name}</span>
                      </button>
                    )
                  })}
                </div>
              ) : (
                <div className="helper-text">{t('noTagsYet')}</div>
              )}
            </div>
          ) : null}

        </div>
        <div className="modal-footer">
          <button
            className="btn btn-secondary"
            onClick={onRequestClose}
            disabled={!canClose}
          >
            {t('cancel')}
          </button>
          <button
            className="btn btn-primary"
            onClick={onSubmit}
            disabled={loading}
          >
            {reimportSkillName
              ? t('assignment.reimport')
              : addModalTab === 'local'
                ? t('scanAndImport')
                : t('install')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(AddSkillModal)
