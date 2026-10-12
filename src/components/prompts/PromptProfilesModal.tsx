import { memo } from 'react'
import { FolderOpen, Plus, Trash2, X } from 'lucide-react'
import type { TFunction } from 'i18next'
import SelectMenu from '../common/SelectMenu'
import type {
  PromptProfileInputDto,
  PromptTool,
} from '../skills/types'

type PromptProfilesModalProps = {
  open: boolean
  drafts: PromptProfileInputDto[]
  saving: boolean
  onChange: (index: number, draft: PromptProfileInputDto) => void
  onAdd: (tool: PromptTool) => void
  onRemove: (index: number) => void
  onPickRoot: (index: number) => void
  onSave: () => void
  onRequestClose: () => void
  t: TFunction
}

const PromptProfilesModal = ({
  open,
  drafts,
  saving,
  onChange,
  onAdd,
  onRemove,
  onPickRoot,
  onSave,
  onRequestClose,
  t,
}: PromptProfilesModalProps) => {
  if (!open) return null
  const valid = drafts.every(
    (draft) => draft.label.trim() && draft.config_root.trim(),
  )
  return (
    <div className="modal-backdrop" onClick={saving ? undefined : onRequestClose}>
      <div className="modal prompt-profiles-modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal-header">
          <div>
            <div className="modal-title">{t('prompts.profilesModal.title')}</div>
            <p>{t('prompts.profilesModal.help')}</p>
          </div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            disabled={saving}
            aria-label={t('close')}
          >
            <X size={18} />
          </button>
        </div>
        <div className="modal-body prompt-profile-drafts">
          {drafts.length === 0 ? (
            <div className="prompt-profile-empty">{t('prompts.profilesModal.empty')}</div>
          ) : (
            drafts.map((draft, index) => (
              <div className="prompt-profile-draft" key={draft.id || `${draft.tool}-${index}`}>
                <SelectMenu
                  className="prompt-profile-tool-select"
                  value={draft.tool}
                  options={[
                    { value: 'codex', label: t('prompts.tools.codex') },
                    {
                      value: 'claude_code',
                      label: t('prompts.tools.claude_code'),
                    },
                    { value: 'pi', label: t('prompts.tools.pi') },
                    {
                      value: 'ds_harness',
                      label: t('prompts.tools.ds_harness'),
                    },
                  ]}
                  onChange={(value) =>
                    onChange(index, {
                      ...draft,
                      tool: value as PromptTool,
                    })
                  }
                  disabled={saving}
                  ariaLabel={t('prompts.profilesModal.tool')}
                  menuMinWidth={160}
                />
                <input
                  value={draft.label}
                  onChange={(event) => onChange(index, { ...draft, label: event.target.value })}
                  placeholder={t('prompts.profilesModal.labelPlaceholder')}
                  disabled={saving}
                />
                <div className="prompt-profile-path-field">
                  <input
                    className="mono"
                    value={draft.config_root}
                    onChange={(event) =>
                      onChange(index, { ...draft, config_root: event.target.value })
                    }
                    placeholder={t('prompts.profilesModal.pathPlaceholder')}
                    disabled={saving}
                  />
                  <button
                    className="prompt-profile-path-picker"
                    type="button"
                    onClick={() => onPickRoot(index)}
                    disabled={saving}
                    title={t('prompts.profilesModal.chooseRoot')}
                    aria-label={t('prompts.profilesModal.chooseRoot')}
                  >
                    <FolderOpen size={16} />
                  </button>
                </div>
                <button
                  className="icon-btn danger"
                  type="button"
                  onClick={() => onRemove(index)}
                  disabled={saving}
                  aria-label={t('prompts.profilesModal.remove')}
                  title={t('prompts.profilesModal.remove')}
                >
                  <Trash2 size={16} />
                </button>
              </div>
            ))
          )}
          <div className="prompt-profile-add-actions">
            <button className="btn btn-secondary" type="button" onClick={() => onAdd('codex')} disabled={saving}>
              <Plus size={15} />
              {t('prompts.profilesModal.addCodex')}
            </button>
            <button className="btn btn-secondary" type="button" onClick={() => onAdd('claude_code')} disabled={saving}>
              <Plus size={15} />
              {t('prompts.profilesModal.addClaude')}
            </button>
            <button className="btn btn-secondary" type="button" onClick={() => onAdd('pi')} disabled={saving}>
              <Plus size={15} />
              {t('prompts.profilesModal.addPi')}
            </button>
            <button
              className="btn btn-secondary"
              type="button"
              onClick={() => onAdd('ds_harness')}
              disabled={saving}
            >
              <Plus size={15} />
              {t('prompts.profilesModal.addDsHarness')}
            </button>
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn btn-secondary" type="button" onClick={onRequestClose} disabled={saving}>
            {t('cancel')}
          </button>
          <button className="btn btn-primary" type="button" onClick={onSave} disabled={saving || !valid}>
            {saving ? t('prompts.saving') : t('prompts.save')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(PromptProfilesModal)
