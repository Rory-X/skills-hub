import { memo, useState } from 'react'
import {
  AlertTriangle,
  Archive,
  ArchiveRestore,
  Clock3,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Workflow,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  SkillTemplateDto,
  SkillTemplateRevisionDto,
} from '../skills/types'

type TemplatesPageProps = {
  templates: SkillTemplateDto[]
  selectedTemplateId: string
  revisions: SkillTemplateRevisionDto[]
  showArchived: boolean
  loading: boolean
  onSelect: (templateId: string) => void
  onCreate: () => void
  onEdit: (template: SkillTemplateDto) => void
  onArchive: (template: SkillTemplateDto, archived: boolean) => void
  onDelete: (template: SkillTemplateDto) => void
  onToggleArchived: (show: boolean) => void
  onLoadHistory: (templateId: string) => void
  onRollback: (template: SkillTemplateDto, revisionId: string) => void
  onOpenProject: (projectId: string) => void
  onRetryProject: (projectId: string) => void
  formatRelative: (value?: number | null) => string
  t: TFunction
}

const TemplatesPage = ({
  templates,
  selectedTemplateId,
  revisions,
  showArchived,
  loading,
  onSelect,
  onCreate,
  onEdit,
  onArchive,
  onDelete,
  onToggleArchived,
  onLoadHistory,
  onRollback,
  onOpenProject,
  onRetryProject,
  formatRelative,
  t,
}: TemplatesPageProps) => {
  const [tab, setTab] = useState<'details' | 'history'>('details')
  const selected =
    templates.find((template) => template.id === selectedTemplateId) ??
    templates[0] ??
    null

  const openHistory = () => {
    if (!selected) return
    setTab('history')
    onLoadHistory(selected.id)
  }

  return (
    <div className="templates-page">
      <div className="templates-page-head">
        <div>
          <h1>{t('templates.title')}</h1>
          <span>{t('templates.count', { count: templates.length })}</span>
        </div>
        <div className="templates-page-actions">
          <label className="templates-archive-toggle">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(event) => onToggleArchived(event.target.checked)}
            />
            <span>{t('templates.showArchived')}</span>
          </label>
          <button
            className="btn btn-primary"
            type="button"
            onClick={onCreate}
            disabled={loading}
          >
            <Plus size={16} />
            {t('templates.create')}
          </button>
        </div>
      </div>

      {templates.length === 0 ? (
        <div className="templates-empty">
          <Workflow size={34} />
          <strong>{t('templates.empty')}</strong>
          <button className="btn btn-primary" type="button" onClick={onCreate}>
            <Plus size={16} />
            {t('templates.create')}
          </button>
        </div>
      ) : (
        <div className="templates-workspace">
          <aside className="templates-list" aria-label={t('templates.title')}>
            {templates.map((template) => (
              <button
                className={`template-list-item${
                  template.id === selected?.id ? ' active' : ''
                }${template.archived ? ' archived' : ''}`}
                type="button"
                key={template.id}
                onClick={() => onSelect(template.id)}
              >
                <span className="template-list-icon">
                  <Workflow size={17} />
                </span>
                <span className="template-list-main">
                  <strong>{template.name}</strong>
                  <small>
                    {t('templates.skillCount', { count: template.skills.length })}
                    <span aria-hidden="true"> · </span>
                    {t(`templates.modes.${template.mode}.label`)}
                  </small>
                </span>
                {template.failure_count > 0 ? (
                  <span className="template-failure-count">
                    <AlertTriangle size={13} />
                    {template.failure_count}
                  </span>
                ) : null}
              </button>
            ))}
          </aside>

          {selected ? (
            <section className="template-detail">
              <div className="template-detail-head">
                <div>
                  <div className="template-title-line">
                    <h2>{selected.name}</h2>
                    {selected.archived ? (
                      <span>{t('templates.archived')}</span>
                    ) : null}
                  </div>
                  {selected.description ? <p>{selected.description}</p> : null}
                </div>
                <div className="template-detail-actions">
                  <button
                    className="icon-btn"
                    type="button"
                    onClick={() => onEdit(selected)}
                    disabled={loading}
                    title={t('templates.edit')}
                    aria-label={t('templates.edit')}
                  >
                    <Pencil size={16} />
                  </button>
                  <button
                    className="icon-btn"
                    type="button"
                    onClick={() => onArchive(selected, !selected.archived)}
                    disabled={loading}
                    title={
                      selected.archived
                        ? t('templates.restore')
                        : t('templates.archive')
                    }
                    aria-label={
                      selected.archived
                        ? t('templates.restore')
                        : t('templates.archive')
                    }
                  >
                    {selected.archived ? (
                      <ArchiveRestore size={16} />
                    ) : (
                      <Archive size={16} />
                    )}
                  </button>
                  <button
                    className="icon-btn danger"
                    type="button"
                    onClick={() => onDelete(selected)}
                    disabled={loading || selected.linked_projects.length > 0}
                    title={
                      selected.linked_projects.length > 0
                        ? t('templates.deleteBlocked')
                        : t('templates.delete')
                    }
                    aria-label={t('templates.delete')}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>

              <div className="template-detail-tabs" role="tablist">
                <button
                  type="button"
                  className={tab === 'details' ? 'active' : ''}
                  onClick={() => setTab('details')}
                >
                  <Workflow size={15} />
                  {t('templates.details')}
                </button>
                <button
                  type="button"
                  className={tab === 'history' ? 'active' : ''}
                  onClick={openHistory}
                >
                  <Clock3 size={15} />
                  {t('templates.history')}
                </button>
              </div>

              {tab === 'details' ? (
                <div className="template-detail-content">
                  <div className="template-meta-row">
                    <span>{t('templates.fields.mode')}</span>
                    <strong>{t(`templates.modes.${selected.mode}.label`)}</strong>
                    <span>{t('templates.updated')}</span>
                    <strong>{formatRelative(selected.updated_at)}</strong>
                  </div>

                  <div className="template-section">
                    <div className="template-section-head">
                      <h3>{t('templates.fields.skills')}</h3>
                      <span>{selected.skills.length}</span>
                    </div>
                    <div className="template-members">
                      {selected.skills.map((skill) => (
                        <div className="template-member-row" key={skill.id}>
                          <div>
                            <strong>{skill.name}</strong>
                            {skill.description ? <small>{skill.description}</small> : null}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>

                  <div className="template-section">
                    <div className="template-section-head">
                      <h3>{t('templates.linkedProjects')}</h3>
                      <span>{selected.linked_projects.length}</span>
                    </div>
                    {selected.linked_projects.length === 0 ? (
                      <div className="template-section-empty">
                        {t('templates.noLinkedProjects')}
                      </div>
                    ) : (
                      <div className="template-projects-list">
                        {selected.linked_projects.map((project) => (
                          <div className="template-project-row" key={project.project_id}>
                            <button
                              type="button"
                              onClick={() => onOpenProject(project.project_id)}
                            >
                              <strong>{project.project_name}</strong>
                              <code>{project.project_root}</code>
                            </button>
                            {project.sync_error ? (
                              <button
                                className="btn btn-secondary"
                                type="button"
                                onClick={() => onRetryProject(project.project_id)}
                                disabled={loading}
                                title={project.sync_error}
                              >
                                <RefreshCw size={14} />
                                {t('templates.retry')}
                              </button>
                            ) : null}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <div className="template-history-list">
                  {revisions.map((revision) => (
                    <div className="template-revision-row" key={revision.id}>
                      <div>
                        <strong>
                          {t('templates.revision', {
                            number: revision.revision_number,
                          })}
                        </strong>
                        <span>{formatRelative(revision.created_at)}</span>
                        <small>
                          {revision.skills.map((skill) => skill.skill_name).join(', ')}
                        </small>
                      </div>
                      {revision.id !== selected.current_revision_id ? (
                        <button
                          className="btn btn-secondary"
                          type="button"
                          onClick={() => onRollback(selected, revision.id)}
                          disabled={loading}
                        >
                          {t('templates.rollback')}
                        </button>
                      ) : null}
                    </div>
                  ))}
                </div>
              )}
            </section>
          ) : null}
        </div>
      )}
    </div>
  )
}

export default memo(TemplatesPage)
