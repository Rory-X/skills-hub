import { memo } from 'react'
import {
  AlertTriangle,
  FolderKanban,
  FolderOpen,
  FolderPlus,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type { ProjectSummaryDto } from '../skills/types'

type ProjectsPageProps = {
  projects: ProjectSummaryDto[]
  importPath: string
  loading: boolean
  onImportPathChange: (value: string) => void
  onPickProject: () => void
  onImportProject: () => void
  onOpenProject: (project: ProjectSummaryDto) => void
  formatRelative: (value?: number | null) => string
  t: TFunction
}

const ProjectsPage = ({
  projects,
  importPath,
  loading,
  onImportPathChange,
  onPickProject,
  onImportProject,
  onOpenProject,
  formatRelative,
  t,
}: ProjectsPageProps) => {
  return (
    <div className="projects-page">
      <div className="projects-page-head">
        <div>
          <h1>{t('projects.title')}</h1>
          <span className="projects-count">
            {t('projects.count', { count: projects.length })}
          </span>
        </div>
        <div className="project-import-control">
          <input
            value={importPath}
            onChange={(event) => onImportPathChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') onImportProject()
            }}
            placeholder={t('projects.absolutePathPlaceholder')}
            aria-label={t('projects.absolutePathPlaceholder')}
            disabled={loading}
          />
          <button
            className="icon-btn"
            type="button"
            onClick={onPickProject}
            title={t('projects.chooseFolder')}
            aria-label={t('projects.chooseFolder')}
            disabled={loading}
          >
            <FolderOpen size={17} />
          </button>
          <button
            className="btn btn-primary"
            type="button"
            onClick={onImportProject}
            disabled={loading || !importPath.trim()}
          >
            <FolderPlus size={16} />
            {t('projects.import')}
          </button>
        </div>
      </div>

      {projects.length === 0 ? (
        <div className="projects-empty">
          <FolderKanban size={32} />
          <strong>{t('projects.emptyTitle')}</strong>
          <span>{t('projects.emptyBody')}</span>
        </div>
      ) : (
        <div className="projects-grid">
          {projects.map((project) => (
            <button
              className="project-card"
              type="button"
              key={project.id}
              onClick={() => onOpenProject(project)}
            >
              <span className="project-card-icon" aria-hidden="true">
                <FolderKanban size={20} />
              </span>
              <span className="project-card-main">
                <strong>{project.name}</strong>
                <span className="project-card-path mono" title={project.root_path}>
                  {project.root_path}
                </span>
                <span className="project-card-meta">
                  {t('projects.skillCount', { count: project.skill_count })}
                  <span aria-hidden="true">·</span>
                  {t('projects.toolCount', {
                    count: project.enabled_tool_keys.length,
                  })}
                  <span aria-hidden="true">·</span>
                  {formatRelative(project.last_scanned_at)}
                </span>
              </span>
              {project.attention_count > 0 ? (
                <span className="project-attention-count">
                  <AlertTriangle size={14} />
                  {project.attention_count}
                </span>
              ) : null}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

export default memo(ProjectsPage)
