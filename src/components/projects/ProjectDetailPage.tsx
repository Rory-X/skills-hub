import { memo, useMemo, useState } from 'react'
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FolderGit2,
  Library,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Wrench,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  ProjectDetailDto,
  ProjectIssueDto,
  ProjectSkillDto,
} from '../skills/types'

type ProjectTab = 'current' | 'library' | 'attention'

type ProjectDetailPageProps = {
  detail: ProjectDetailDto
  loading: boolean
  onBack: () => void
  onRefresh: () => void
  onToolsChange: (toolKeys: string[]) => void
  onGitExcludeChange: (enabled: boolean) => void
  onInjectSkill: (skillId: string) => void
  onRemoveSkill: (skillId: string) => void
  onCollectSkill: (discoveredSkillId: string, name: string) => void
  onRepairIssue: (issueId: string) => void
  t: TFunction
}

const ProjectDetailPage = ({
  detail,
  loading,
  onBack,
  onRefresh,
  onToolsChange,
  onGitExcludeChange,
  onInjectSkill,
  onRemoveSkill,
  onCollectSkill,
  onRepairIssue,
  t,
}: ProjectDetailPageProps) => {
  const [activeTab, setActiveTab] = useState<ProjectTab>('current')
  const [librarySearch, setLibrarySearch] = useState('')
  const [expandedSkills, setExpandedSkills] = useState<Record<string, boolean>>({})
  const toolLabels = useMemo(
    () =>
      Object.fromEntries(
        detail.tools.map((tool) => [
          tool.key,
          t(`tools.${tool.key}`, { defaultValue: tool.label }),
        ]),
      ),
    [detail.tools, t],
  )
  const filteredLibrary = useMemo(() => {
    const query = librarySearch.trim().toLowerCase()
    if (!query) return detail.library_skills
    return detail.library_skills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(query) ||
        skill.description?.toLowerCase().includes(query),
    )
  }, [detail.library_skills, librarySearch])

  const toggleTool = (toolKey: string, enabled: boolean) => {
    const next = new Set(detail.project.enabled_tool_keys)
    if (enabled) next.add(toolKey)
    else next.delete(toolKey)
    onToolsChange([...next])
  }

  const renderSkillRow = (skill: ProjectSkillDto) => {
    const expanded = Boolean(expandedSkills[skill.name])
    const collectiblePath = skill.paths.find(
      (path) => path.status !== 'invalid_missing_skill_md',
    )
    return (
      <div className="project-skill-row" key={skill.name}>
        <button
          className="project-skill-expand"
          type="button"
          onClick={() =>
            setExpandedSkills((current) => ({
              ...current,
              [skill.name]: !expanded,
            }))
          }
          aria-label={t('projects.details')}
        >
          {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </button>
        <div className="project-skill-main">
          <div className="project-skill-title-line">
            <strong>{skill.name}</strong>
            <span className={`project-source-badge source-${skill.source}`}>
              {t(`projects.sources.${skill.source}`)}
            </span>
            {skill.status === 'needs_attention' ? (
              <span className="project-skill-warning">
                <AlertTriangle size={13} />
                {t('projects.needsAttention')}
              </span>
            ) : null}
          </div>
          {skill.description ? (
            <p className="project-skill-description">{skill.description}</p>
          ) : null}
          <div className="project-skill-tools">
            {skill.tool_keys.map((toolKey) => (
              <span key={toolKey}>{toolLabels[toolKey] ?? toolKey}</span>
            ))}
          </div>
          {expanded ? (
            <div className="project-skill-paths">
              {skill.paths.length === 0 ? (
                <span>{t('projects.pendingTarget')}</span>
              ) : (
                skill.paths.map((path) => (
                  <div key={path.absolute_path}>
                    <code>{path.relative_path}</code>
                    <span>
                      {path.tool_keys
                        .map((toolKey) => toolLabels[toolKey] ?? toolKey)
                        .join(', ')}
                    </span>
                  </div>
                ))
              )}
            </div>
          ) : null}
        </div>
        <div className="project-skill-actions">
          {skill.source === 'library' && skill.linked_skill_id ? (
            <button
              className="btn btn-quiet danger"
              type="button"
              onClick={() => onRemoveSkill(skill.linked_skill_id as string)}
              disabled={loading}
            >
              <Trash2 size={15} />
              {t('projects.removeFromProject')}
            </button>
          ) : skill.source === 'project' && collectiblePath ? (
            <button
              className="btn btn-quiet"
              type="button"
              onClick={() =>
                onCollectSkill(collectiblePath.discovered_skill_id, skill.name)
              }
              disabled={loading}
            >
              <Archive size={15} />
              {t('projects.collectToLibrary')}
            </button>
          ) : null}
        </div>
      </div>
    )
  }

  const renderIssue = (issue: ProjectIssueDto) => {
    const variables = {
      skill: issue.skill_name ?? t('unknown'),
      tool: issue.tool_key
        ? toolLabels[issue.tool_key] ?? issue.tool_key
        : t('unknown'),
    }
    return (
      <div className="project-issue-row" key={issue.id}>
        <span className="project-issue-icon" aria-hidden="true">
          <AlertTriangle size={17} />
        </span>
        <div className="project-issue-main">
          <strong>{t(`projects.issues.${issue.kind}.title`, variables)}</strong>
          <p>{t(`projects.issues.${issue.kind}.impact`, variables)}</p>
          <span>{t(`projects.issues.${issue.kind}.recommendation`, variables)}</span>
          {issue.path ? <code>{issue.path}</code> : null}
        </div>
        {issue.fixable ? (
          <button
            className="btn btn-secondary"
            type="button"
            onClick={() => onRepairIssue(issue.id)}
            disabled={loading}
          >
            <Wrench size={15} />
            {t('projects.repair')}
          </button>
        ) : null}
      </div>
    )
  }

  return (
    <div className="project-detail-page">
      <div className="project-detail-head">
        <button
          className="icon-btn"
          type="button"
          onClick={onBack}
          aria-label={t('back')}
        >
          <ArrowLeft size={18} />
        </button>
        <div className="project-detail-identity">
          <h1>{detail.project.name}</h1>
          <code title={detail.project.root_path}>{detail.project.root_path}</code>
        </div>
        <button
          className="icon-btn"
          type="button"
          onClick={onRefresh}
          disabled={loading}
          title={t('projects.refresh')}
          aria-label={t('projects.refresh')}
        >
          <RefreshCw size={17} />
        </button>
      </div>

      <div className="project-controls-band">
        <details className="project-tools-control">
          <summary>
            <FolderGit2 size={16} />
            <span>{t('projects.projectTools')}</span>
            <strong>
              {t('projects.toolCount', {
                count: detail.project.enabled_tool_keys.length,
              })}
            </strong>
          </summary>
          <div className="project-tool-options">
            {detail.tools.map((tool) => (
              <label key={tool.key} title={tool.project_skills_dir}>
                <input
                  type="checkbox"
                  checked={tool.enabled}
                  onChange={(event) => toggleTool(tool.key, event.target.checked)}
                  disabled={loading}
                />
                <span>{toolLabels[tool.key] ?? tool.label}</span>
                <code>{tool.project_skills_dir}</code>
              </label>
            ))}
          </div>
        </details>
        <div className="project-git-exclude-control">
          <div>
            <span>{t('projects.localGitExclude')}</span>
            <code>.git/info/exclude</code>
          </div>
          <button
            type="button"
            className={`settings-toggle${
              detail.project.local_git_exclude_enabled ? ' checked' : ''
            }`}
            aria-pressed={detail.project.local_git_exclude_enabled}
            onClick={() =>
              onGitExcludeChange(!detail.project.local_git_exclude_enabled)
            }
            disabled={loading}
          >
            <span className="settings-toggle-knob" />
          </button>
        </div>
      </div>

      <div className="project-view-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === 'current'}
          className={activeTab === 'current' ? 'active' : ''}
          onClick={() => setActiveTab('current')}
        >
          <CheckCircle2 size={16} />
          {t('projects.tabs.current')}
          <span>{detail.current_skills.length}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === 'library'}
          className={activeTab === 'library' ? 'active' : ''}
          onClick={() => setActiveTab('library')}
        >
          <Library size={16} />
          {t('projects.tabs.library')}
          <span>{detail.library_skills.length}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === 'attention'}
          className={activeTab === 'attention' ? 'active' : ''}
          onClick={() => setActiveTab('attention')}
        >
          <AlertTriangle size={16} />
          {t('projects.tabs.attention')}
          <span>{detail.issues.length}</span>
        </button>
      </div>

      <div className="project-view-content">
        {activeTab === 'current' ? (
          detail.current_skills.length === 0 ? (
            <div className="project-section-empty">{t('projects.currentEmpty')}</div>
          ) : (
            <div className="project-skills-list">
              {detail.current_skills.map(renderSkillRow)}
            </div>
          )
        ) : activeTab === 'library' ? (
          <div className="project-library-view">
            <label className="project-library-search">
              <Search size={16} />
              <input
                value={librarySearch}
                onChange={(event) => setLibrarySearch(event.target.value)}
                placeholder={t('projects.searchLibrary')}
              />
            </label>
            {filteredLibrary.length === 0 ? (
              <div className="project-section-empty">{t('projects.libraryEmpty')}</div>
            ) : (
              <div className="project-library-list">
                {filteredLibrary.map((skill) => (
                  <div className="project-library-row" key={skill.skill_id}>
                    <div>
                      <strong>{skill.name}</strong>
                      {skill.description ? <p>{skill.description}</p> : null}
                    </div>
                    <span className={`project-availability ${skill.availability}`}>
                      {t(`projects.availability.${skill.availability}`)}
                    </span>
                    {skill.availability === 'addable' ? (
                      <button
                        className="btn btn-primary"
                        type="button"
                        onClick={() => onInjectSkill(skill.skill_id)}
                        disabled={loading}
                      >
                        <Plus size={15} />
                        {t('projects.add')}
                      </button>
                    ) : skill.availability === 'needs_repair' ? (
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={() => setActiveTab('attention')}
                      >
                        <Wrench size={15} />
                        {t('projects.review')}
                      </button>
                    ) : null}
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : detail.issues.length === 0 ? (
          <div className="project-section-empty healthy">
            <CheckCircle2 size={22} />
            {t('projects.attentionEmpty')}
          </div>
        ) : (
          <div className="project-issues-list">
            {detail.issues.map(renderIssue)}
          </div>
        )}
      </div>
    </div>
  )
}

export default memo(ProjectDetailPage)
