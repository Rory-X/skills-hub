import { memo, useMemo, useState } from 'react'
import { LockKeyhole, Search, X } from 'lucide-react'
import type { TFunction } from 'i18next'
import type { ManagedSkill, SkillTemplateDto, TemplateMode } from '../skills/types'
import {
  createTemplateDraft,
  normalizeTemplateSkillIds,
  sortTemplateSkillsBySelection,
  templateDraftCanSave,
  type TemplateDraft,
} from './templateState'

type TemplateEditorModalProps = {
  open: boolean
  template?: SkillTemplateDto | null
  initialSkillIds?: string[]
  skills: ManagedSkill[]
  saving: boolean
  onClose: () => void
  onSave: (draft: TemplateDraft) => void
  t: TFunction
}

const TemplateEditorModal = ({
  open,
  template,
  initialSkillIds = [],
  skills,
  saving,
  onClose,
  onSave,
  t,
}: TemplateEditorModalProps) => {
  const [draft, setDraft] = useState<TemplateDraft>(() =>
    createTemplateDraft(template, initialSkillIds),
  )
  const [query, setQuery] = useState('')

  const existingSkillIds = useMemo(
    () => template?.skills.map((skill) => skill.id) ?? [],
    [template],
  )
  const existing = useMemo(() => new Set(existingSkillIds), [existingSkillIds])
  const selected = useMemo(() => new Set(draft.skillIds), [draft.skillIds])
  const filteredSkills = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    return sortTemplateSkillsBySelection(
      skills.filter(
        (skill) =>
          !normalized ||
          skill.name.toLowerCase().includes(normalized) ||
          skill.description?.toLowerCase().includes(normalized),
      ),
      draft.skillIds,
    )
  }, [draft.skillIds, query, skills])
  const canSave = templateDraftCanSave(draft, skills, existingSkillIds)

  if (!open) return null

  const setMode = (mode: TemplateMode) => {
    if (template) return
    setDraft((current) => ({ ...current, mode }))
  }

  const toggleSkill = (skillId: string, checked: boolean) => {
    setDraft((current) => ({
      ...current,
      skillIds: normalizeTemplateSkillIds(
        checked
          ? [...current.skillIds, skillId]
          : current.skillIds.filter((id) => id !== skillId),
      ),
    }))
  }

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <div
        className="modal template-editor-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="template-editor-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <h2 id="template-editor-title">
            {template ? t('templates.edit') : t('templates.create')}
          </h2>
          <button
            className="icon-btn"
            type="button"
            onClick={onClose}
            disabled={saving}
            aria-label={t('close')}
          >
            <X size={18} />
          </button>
        </div>

        <div className="modal-body template-editor-body">
          <label className="template-field">
            <span>{t('templates.fields.name')}</span>
            <input
              value={draft.name}
              onChange={(event) =>
                setDraft((current) => ({ ...current, name: event.target.value }))
              }
              disabled={saving}
              autoFocus
            />
          </label>
          <label className="template-field">
            <span>{t('templates.fields.description')}</span>
            <textarea
              value={draft.description}
              onChange={(event) =>
                setDraft((current) => ({
                  ...current,
                  description: event.target.value,
                }))
              }
              disabled={saving}
              rows={3}
            />
          </label>

          <div className="template-field">
            <span>{t('templates.fields.mode')}</span>
            <div className="template-mode-control">
              {(['live', 'snapshot'] as const).map((mode) => (
                <button
                  key={mode}
                  className={draft.mode === mode ? 'active' : ''}
                  type="button"
                  onClick={() => setMode(mode)}
                  disabled={saving || Boolean(template)}
                  title={t(`templates.modes.${mode}.title`)}
                >
                  {t(`templates.modes.${mode}.label`)}
                </button>
              ))}
              {template ? (
                <LockKeyhole
                  size={15}
                  aria-label={t('templates.modeLocked')}
                />
              ) : null}
            </div>
          </div>

          <div className="template-skill-picker">
            <div className="template-skill-picker-head">
              <strong>{t('templates.fields.skills')}</strong>
              <span>{draft.skillIds.length}</span>
            </div>
            <label className="template-skill-search">
              <Search size={16} />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t('templates.searchSkills')}
              />
            </label>
            <div className="template-skill-options">
              {filteredSkills.map((skill) => {
                const checked = selected.has(skill.id)
                const unavailable = !skill.enabled && !existing.has(skill.id)
                return (
                  <label
                    className={unavailable ? 'unavailable' : ''}
                    key={skill.id}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={saving || unavailable}
                      onChange={(event) =>
                        toggleSkill(skill.id, event.target.checked)
                      }
                    />
                    <span>
                      <strong>{skill.name}</strong>
                      {skill.description ? <small>{skill.description}</small> : null}
                    </span>
                  </label>
                )
              })}
            </div>
          </div>
        </div>

        <div className="modal-actions">
          <button
            className="btn btn-secondary"
            type="button"
            onClick={onClose}
            disabled={saving}
          >
            {t('cancel')}
          </button>
          <button
            className="btn btn-primary"
            type="button"
            onClick={() => onSave(draft)}
            disabled={saving || !canSave}
          >
            {t('templates.save')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(TemplateEditorModal)
