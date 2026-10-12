import { memo, useEffect, useMemo, useState } from 'react'
import { AlertTriangle, ArrowRightLeft, Download, Unlink, X } from 'lucide-react'
import type { TFunction } from 'i18next'
import SelectMenu from '../common/SelectMenu'
import type {
  ProjectTemplateBindingDto,
  SkillTemplateDto,
  TemplateDiffDto,
} from '../skills/types'
import { templateDiffCount } from './templateState'

type ProjectTemplateModalProps = {
  open: boolean
  projectName: string
  binding?: ProjectTemplateBindingDto | null
  templates: SkillTemplateDto[]
  loading: boolean
  importFlow?: boolean
  onPreview: (templateId: string) => Promise<TemplateDiffDto | null>
  onApply: (templateId: string) => void
  onDetach: (retainAsManual: boolean) => void
  onUpdateSnapshot: () => void
  onClose: () => void
  t: TFunction
}

const ProjectTemplateModal = ({
  open,
  projectName,
  binding,
  templates,
  loading,
  importFlow = false,
  onPreview,
  onApply,
  onDetach,
  onUpdateSnapshot,
  onClose,
  t,
}: ProjectTemplateModalProps) => {
  const availableTemplates = useMemo(
    () =>
      templates.filter(
        (template) => !template.archived || template.id === binding?.template_id,
      ),
    [binding?.template_id, templates],
  )
  const initialTemplateId =
    binding?.template_id ?? availableTemplates[0]?.id ?? ''
  const [selectedTemplateId, setSelectedTemplateId] =
    useState(initialTemplateId)
  const [diff, setDiff] = useState<TemplateDiffDto | null>(null)
  const [previewing, setPreviewing] = useState(Boolean(initialTemplateId))
  const [showDetach, setShowDetach] = useState(false)

  useEffect(() => {
    if (!open || !selectedTemplateId) return
    let active = true
    void onPreview(selectedTemplateId)
      .then((result) => {
        if (active) setDiff(result)
      })
      .finally(() => {
        if (active) setPreviewing(false)
      })
    return () => {
      active = false
    }
  }, [onPreview, open, selectedTemplateId])

  if (!open) return null

  const selectedTemplate = availableTemplates.find(
    (template) => template.id === selectedTemplateId,
  )
  const counts = templateDiffCount(diff)
  const switching = Boolean(
    binding && selectedTemplateId && binding.template_id !== selectedTemplateId,
  )
  const sameTemplate = binding?.template_id === selectedTemplateId

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <div
        className="modal project-template-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-template-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <h2 id="project-template-title">{t('templates.project.title')}</h2>
            <span>{projectName}</span>
          </div>
          <button
            className="icon-btn"
            type="button"
            onClick={onClose}
            disabled={loading}
            aria-label={t('close')}
          >
            <X size={18} />
          </button>
        </div>

        <div className="modal-body project-template-body">
          {binding ? (
            <div className="project-template-current">
              <span>{t('templates.project.current')}</span>
              <strong>{binding.template_name}</strong>
              <small>{t(`templates.modes.${binding.mode}.label`)}</small>
              {binding.sync_error ? (
                <AlertTriangle size={16} aria-label={t('projects.needsAttention')} />
              ) : null}
            </div>
          ) : null}

          <label className="template-field">
            <span>{t('templates.project.template')}</span>
            <SelectMenu
              className="template-select-menu"
              value={selectedTemplateId}
              options={availableTemplates.map((template) => ({
                value: template.id,
                label: template.name,
              }))}
              onChange={(templateId) => {
                setSelectedTemplateId(templateId)
                setDiff(null)
                setPreviewing(Boolean(templateId))
              }}
              ariaLabel={t('templates.project.template')}
              disabled={loading || availableTemplates.length === 0}
              menuMinWidth={240}
            />
          </label>

          {selectedTemplate ? (
            <>
              <div className="project-template-selection">
                <div>
                  <strong>{selectedTemplate.name}</strong>
                  {selectedTemplate.description ? (
                    <span>{selectedTemplate.description}</span>
                  ) : null}
                </div>
                <small>
                  {t('templates.skillCount', {
                    count: selectedTemplate.skills.length,
                  })}
                </small>
              </div>

              <section
                className="project-template-skills"
                aria-labelledby="project-template-skills-title"
              >
                <div className="project-template-skills-head">
                  <strong id="project-template-skills-title">
                    {t('templates.project.includedSkills')}
                  </strong>
                </div>
                <div className="project-template-skill-list">
                  {selectedTemplate.skills.map((skill) => (
                    <div
                      className={`project-template-skill-row${
                        skill.enabled ? '' : ' disabled'
                      }`}
                      key={skill.id}
                    >
                      <div>
                        <strong>{skill.name}</strong>
                        {skill.description ? <small>{skill.description}</small> : null}
                      </div>
                      {!skill.enabled ? (
                        <span className="project-template-skill-status">
                          {t('disabled')}
                        </span>
                      ) : null}
                    </div>
                  ))}
                </div>
              </section>
            </>
          ) : (
            <div className="template-section-empty">{t('templates.empty')}</div>
          )}

          {diff && !previewing ? (
            <div className="project-template-diff">
              <div>
                <strong>+{counts.added}</strong>
                <span>{t('templates.project.added')}</span>
              </div>
              <div>
                <strong>-{counts.removed}</strong>
                <span>{t('templates.project.removed')}</span>
              </div>
              <div>
                <strong>{counts.retained}</strong>
                <span>{t('templates.project.retained')}</span>
              </div>
            </div>
          ) : null}

          {binding?.mode === 'snapshot' &&
          binding.snapshot_outdated &&
          sameTemplate ? (
            <button
              className="btn btn-secondary project-template-update"
              type="button"
              onClick={onUpdateSnapshot}
              disabled={loading}
            >
              <Download size={15} />
              {t('templates.project.updateSnapshot')}
            </button>
          ) : null}

          {binding && showDetach ? (
            <div className="project-template-detach-options">
              <button
                className="btn btn-primary"
                type="button"
                onClick={() => onDetach(true)}
                disabled={loading}
              >
                {t('templates.project.detachRetain')}
              </button>
              <button
                className="btn btn-danger"
                type="button"
                onClick={() => onDetach(false)}
                disabled={loading}
              >
                {t('templates.project.detachRemove')}
              </button>
            </div>
          ) : null}
        </div>

        <div className="modal-actions project-template-actions">
          {binding ? (
            <button
              className="btn btn-secondary"
              type="button"
              onClick={() => setShowDetach((current) => !current)}
              disabled={loading}
            >
              <Unlink size={15} />
              {t('templates.project.detach')}
            </button>
          ) : null}
          <span className="modal-actions-spacer" />
          <button
            className="btn btn-secondary"
            type="button"
            onClick={onClose}
            disabled={loading}
          >
            {importFlow ? t('templates.project.skip') : t('cancel')}
          </button>
          <button
            className="btn btn-primary"
            type="button"
            onClick={() => onApply(selectedTemplateId)}
            disabled={
              loading || previewing || !selectedTemplateId || (sameTemplate && !switching)
            }
          >
            {switching ? <ArrowRightLeft size={15} /> : <Download size={15} />}
            {switching
              ? t('templates.project.switch')
              : t('templates.project.apply')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(ProjectTemplateModal)
