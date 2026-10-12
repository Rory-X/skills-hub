import { memo, useMemo, useState } from 'react'
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FolderGit2,
  EyeOff,
  Eye,
  FileText,
  Library,
  ListChecks,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Wrench,
  Workflow,
  X,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  ProjectDetailDto,
  ProjectIssueActionDto,
  ProjectIssueDto,
  ProjectSkillDto,
} from '../skills/types'
import Tooltip from '../common/Tooltip'
import type { SkillPreviewRequest } from '../skills/SkillPreviewDrawer'
import {
  getAvailableProjectToolKeys,
} from '../skills/toolAvailability'
import { getDirectProjectIssueAction } from './projectIssueResolution'
import {
  getManageableProjectSkillIds,
  getProjectLibraryAddAction,
} from './projectBulkActions'

type ProjectTab = 'current' | 'library' | 'attention'

type ProjectDetailPageProps = {
  detail: ProjectDetailDto
  loading: boolean
  onBack: () => void
  onRefresh: () => void
  onOpenPrompts: () => void
  onManageTemplate: () => void
  onOpenSkillDetail: (request: SkillPreviewRequest) => void
  onToolsChange: (toolKeys: string[]) => void
  onGitExcludeChange: (enabled: boolean) => void
  onAddLibrarySkill: (skillId: string) => void
  onAddLibrarySkills: (skillIds: string[]) => void
  onHandleLibraryConflict: (skillId: string) => void
  onAssignSkills: (skillIds: string[]) => void
  onRemoveSkill: (skillId: string) => void
  onRemoveSkills: (skillIds: string[]) => void
  onCollectSkill: (discoveredSkillId: string, name: string) => void
  onResolveIssue: (
    issueId: string,
    action: ProjectIssueActionDto,
  ) => void
  onIgnoreIssue: (issueId: string, fingerprint: string) => void
  onIgnoreAllIssues: () => void
  t: TFunction
}

const ProjectDetailPage = ({
  detail,
  loading,
  onBack,
  onRefresh,
  onOpenPrompts,
  onManageTemplate,
  onOpenSkillDetail,
  onToolsChange,
  onGitExcludeChange,
  onAddLibrarySkill,
  onAddLibrarySkills,
  onHandleLibraryConflict,
  onAssignSkills,
  onRemoveSkill,
  onRemoveSkills,
  onCollectSkill,
  onResolveIssue,
  onIgnoreIssue,
  onIgnoreAllIssues,
  t,
}: ProjectDetailPageProps) => {
  const [activeTab, setActiveTab] = useState<ProjectTab>('current')
  const [librarySearch, setLibrarySearch] = useState('')
  const [librarySkillSelection, setSelectedLibrarySkillIds] = useState<string[]>([])
  const [currentBulkMode, setCurrentBulkMode] = useState(false)
  const [currentSkillSelection, setSelectedCurrentSkillIds] = useState<string[]>([])
  const [pendingBulkRemoveIds, setPendingBulkRemoveIds] = useState<string[]>([])
  const [expandedSkills, setExpandedSkills] = useState<Record<string, boolean>>({})
  const [resolutionIssue, setResolutionIssue] = useState<ProjectIssueDto | null>(null)
  const [selectedActionIndex, setSelectedActionIndex] = useState(0)
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
  const visibleAddableLibraryIds = useMemo(
    () =>
      filteredLibrary
        .filter((skill) => skill.availability === 'addable')
        .map((skill) => skill.skill_id),
    [filteredLibrary],
  )
  const manageableCurrentSkillIds = useMemo(
    () => {
      const manualSkillIds = new Set(
        detail.current_skills
          .filter((skill) => skill.origin_types.includes('manual'))
          .map((skill) => skill.linked_skill_id)
          .filter((skillId): skillId is string => Boolean(skillId)),
      )
      return getManageableProjectSkillIds(detail.current_skills).filter((skillId) =>
        manualSkillIds.has(skillId),
      )
    },
    [detail.current_skills],
  )
  const selectedLibrarySkillIds = useMemo(() => {
    const addableIds = new Set(
      detail.library_skills
        .filter((skill) => skill.availability === 'addable')
        .map((skill) => skill.skill_id),
    )
    return librarySkillSelection.filter((skillId) => addableIds.has(skillId))
  }, [detail.library_skills, librarySkillSelection])
  const selectedCurrentSkillIds = useMemo(() => {
    const manageableIds = new Set(manageableCurrentSkillIds)
    return currentSkillSelection.filter((skillId) => manageableIds.has(skillId))
  }, [currentSkillSelection, manageableCurrentSkillIds])
  const hasEnabledProjectAgents =
    getAvailableProjectToolKeys(
      detail.tools,
      detail.project.enabled_tool_keys,
    ).length > 0
  const selectedCurrentSkillNames = useMemo(() => {
    const selectedIds = new Set(selectedCurrentSkillIds)
    return detail.current_skills
      .filter(
        (skill) =>
          skill.linked_skill_id && selectedIds.has(skill.linked_skill_id),
      )
      .map((skill) => skill.name)
  }, [detail.current_skills, selectedCurrentSkillIds])

  const toggleTool = (toolKey: string, enabled: boolean) => {
    const next = new Set(detail.project.enabled_tool_keys)
    if (enabled) next.add(toolKey)
    else next.delete(toolKey)
    onToolsChange([...next])
  }

  const toggleLibrarySkill = (skillId: string, checked: boolean) => {
    setSelectedLibrarySkillIds((current) =>
      checked
        ? Array.from(new Set([...current, skillId]))
        : current.filter((id) => id !== skillId),
    )
  }

  const toggleVisibleLibrarySkills = () => {
    const visibleIds = new Set(visibleAddableLibraryIds)
    const allVisibleSelected = visibleAddableLibraryIds.every((skillId) =>
      selectedLibrarySkillIds.includes(skillId),
    )
    setSelectedLibrarySkillIds((current) =>
      allVisibleSelected
        ? current.filter((skillId) => !visibleIds.has(skillId))
        : Array.from(new Set([...current, ...visibleAddableLibraryIds])),
    )
  }

  const addSelectedLibrarySkills = () => {
    const action = getProjectLibraryAddAction(selectedLibrarySkillIds)
    if (action.kind === 'none') return
    setSelectedLibrarySkillIds([])
    if (action.kind === 'direct') {
      onAddLibrarySkill(action.skillId)
    } else {
      onAddLibrarySkills(action.skillIds)
    }
  }

  const toggleCurrentSkill = (skillId: string, checked: boolean) => {
    setSelectedCurrentSkillIds((current) =>
      checked
        ? Array.from(new Set([...current, skillId]))
        : current.filter((id) => id !== skillId),
    )
  }

  const toggleAllCurrentSkills = () => {
    const allSelected = manageableCurrentSkillIds.every((skillId) =>
      selectedCurrentSkillIds.includes(skillId),
    )
    setSelectedCurrentSkillIds(
      allSelected ? [] : Array.from(manageableCurrentSkillIds),
    )
  }

  const closeCurrentBulkMode = () => {
    setCurrentBulkMode(false)
    setSelectedCurrentSkillIds([])
  }

  const confirmBulkRemove = () => {
    if (pendingBulkRemoveIds.length === 0) return
    const skillIds = pendingBulkRemoveIds
    setPendingBulkRemoveIds([])
    closeCurrentBulkMode()
    onRemoveSkills(skillIds)
  }

  const openIssueResolution = (issue: ProjectIssueDto) => {
    const directAction = getDirectProjectIssueAction(issue)
    if (directAction) {
      onResolveIssue(issue.id, directAction)
      return
    }
    setSelectedActionIndex(0)
    setResolutionIssue(issue)
  }

  const closeIssueResolution = () => {
    if (loading) return
    setResolutionIssue(null)
    setSelectedActionIndex(0)
  }

  const confirmIssueResolution = () => {
    const action = resolutionIssue?.actions[selectedActionIndex]
    if (!resolutionIssue || !action) return
    const issueId = resolutionIssue.id
    setResolutionIssue(null)
    setSelectedActionIndex(0)
    onResolveIssue(issueId, action)
  }

  const renderSkillRow = (skill: ProjectSkillDto) => {
    const expanded = Boolean(expandedSkills[skill.name])
    const manageableSkillId =
      skill.source === 'library' ? skill.linked_skill_id ?? null : null
    const bulkSelected = Boolean(
      manageableSkillId && selectedCurrentSkillIds.includes(manageableSkillId),
    )
    const collectiblePath = skill.paths.find(
      (path) => path.status !== 'invalid_missing_skill_md',
    )
    const additionallyVisibleTools = skill.additionally_visible_tool_keys.map(
      (toolKey) => ({ key: toolKey, label: toolLabels[toolKey] ?? toolKey }),
    )
    const additionalVisibilityTooltip = t('projects.alsoVisibleTo', {
      tools: additionallyVisibleTools.map((tool) => tool.label).join(', '),
    })
    const managedSkillId = skill.linked_skill_id ?? skill.matched_skill_id
    const previewSource = managedSkillId
      ? { kind: 'managed' as const, skillId: managedSkillId }
      : collectiblePath
        ? {
            kind: 'local' as const,
            basePath: collectiblePath.absolute_path,
            subpath: '.',
          }
        : null
    return (
      <div
        className={`project-skill-row${currentBulkMode ? ' bulk-mode' : ''}${bulkSelected ? ' bulk-selected' : ''}`}
        key={skill.name}
      >
        {currentBulkMode ? (
          <label
            className={`project-row-checkbox${manageableSkillId ? '' : ' unavailable'}`}
            title={
              manageableSkillId
                ? t('projects.selectSkill')
                : t('projects.onlyLibrarySkillsBatch')
            }
          >
            <input
              type="checkbox"
              checked={bulkSelected}
              disabled={loading || !manageableSkillId}
              onChange={(event) =>
                manageableSkillId
                  ? toggleCurrentSkill(manageableSkillId, event.target.checked)
                  : undefined
              }
            />
          </label>
        ) : null}
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
            {previewSource ? (
              <button
                className="project-skill-title"
                type="button"
                onClick={() =>
                  onOpenSkillDetail({
                    title: skill.name,
                    description: skill.description,
                    source: previewSource,
                  })
                }
              >
                {skill.name}
              </button>
            ) : (
              <strong>{skill.name}</strong>
            )}
            <span className={`project-source-badge source-${skill.source}`}>
              {t(`projects.sources.${skill.source}`)}
            </span>
            {skill.origin_types.includes('template') ? (
              <span className="project-source-badge source-template">
                {t('projects.sources.template')}
              </span>
            ) : null}
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
          {additionallyVisibleTools.length > 0 ? (
            <div className="project-skill-visible-tools">
              <Tooltip content={additionalVisibilityTooltip}>
                <button
                  className="project-skill-visibility-help"
                  type="button"
                  aria-label={additionalVisibilityTooltip}
                >
                  <Eye size={13} aria-hidden="true" />
                </button>
              </Tooltip>
              <span className="project-skill-visible-list">
                {additionallyVisibleTools.map((tool) => (
                  <span key={tool.key}>{tool.label}</span>
                ))}
              </span>
            </div>
          ) : null}
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
          {skill.source === 'library' &&
          skill.linked_skill_id &&
          skill.origin_types.includes('manual') ? (
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
        <div className="project-issue-actions">
          {issue.can_ignore ? (
            <Tooltip content={t('projects.ignoreIssue')}>
              <button
                className="icon-btn"
                type="button"
                onClick={() => onIgnoreIssue(issue.id, issue.fingerprint)}
                disabled={loading}
                aria-label={t('projects.ignoreIssue')}
              >
                <EyeOff size={16} />
              </button>
            </Tooltip>
          ) : null}
          {issue.actions.length > 0 ? (
            <button
              className="btn btn-secondary"
              type="button"
              onClick={() => openIssueResolution(issue)}
              disabled={loading}
            >
              <Wrench size={15} />
              {t('projects.handleIssue')}
            </button>
          ) : null}
        </div>
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
        <div className="project-detail-head-actions">
          <button
            className="btn btn-secondary"
            type="button"
            onClick={onManageTemplate}
            disabled={loading}
          >
            <Workflow size={16} />
            {detail.template_binding?.template_name ?? t('templates.project.apply')}
            {detail.template_binding?.sync_error ? (
              <AlertTriangle size={14} aria-label={t('projects.needsAttention')} />
            ) : null}
          </button>
          <button
            className="btn btn-secondary"
            type="button"
            onClick={onOpenPrompts}
            disabled={loading}
          >
            <FileText size={16} />
            {t('navPrompts')}
          </button>
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
                <label
                  className={tool.installed ? '' : 'unavailable'}
                  key={tool.key}
                  title={tool.project_skills_dir}
                >
                  <input
                    type="checkbox"
                    checked={tool.enabled}
                    onChange={(event) =>
                      toggleTool(tool.key, event.target.checked)
                    }
                    disabled={loading || (!tool.installed && !tool.enabled)}
                  />
                  <span>{toolLabels[tool.key] ?? tool.label}</span>
                  <code>{tool.project_skills_dir}</code>
                  {!tool.installed ? (
                    <small>{t('assignment.unavailable')}</small>
                  ) : null}
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
            <>
              <div className="project-bulk-toolbar">
                {currentBulkMode ? (
                  <>
                    <span>
                      {t('projects.selectedSkills', {
                        count: selectedCurrentSkillIds.length,
                      })}
                    </span>
                    <div>
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={toggleAllCurrentSkills}
                        disabled={loading || manageableCurrentSkillIds.length === 0}
                      >
                        {manageableCurrentSkillIds.length > 0 &&
                        manageableCurrentSkillIds.every((skillId) =>
                          selectedCurrentSkillIds.includes(skillId),
                        )
                          ? t('projects.clearSelection')
                          : t('projects.selectAll')}
                      </button>
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={() => {
                          onAssignSkills(selectedCurrentSkillIds)
                          closeCurrentBulkMode()
                        }}
                        disabled={
                          loading ||
                          selectedCurrentSkillIds.length === 0 ||
                          !hasEnabledProjectAgents
                        }
                        title={
                          hasEnabledProjectAgents
                            ? undefined
                            : t('projects.noEnabledAgents')
                        }
                      >
                        {t('projects.setAgents')}
                      </button>
                      <button
                        className="btn btn-danger"
                        type="button"
                        onClick={() =>
                          setPendingBulkRemoveIds(selectedCurrentSkillIds)
                        }
                        disabled={loading || selectedCurrentSkillIds.length === 0}
                      >
                        {t('projects.removeSelected')}
                      </button>
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={closeCurrentBulkMode}
                        disabled={loading}
                      >
                        {t('cancel')}
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <span>{t('projects.currentBatchHint')}</span>
                    <button
                      className="btn btn-secondary"
                      type="button"
                      onClick={() => setCurrentBulkMode(true)}
                      disabled={loading || manageableCurrentSkillIds.length === 0}
                    >
                      <ListChecks size={15} />
                      {t('projects.bulkManage')}
                    </button>
                  </>
                )}
              </div>
              <div className="project-skills-list">
                {detail.current_skills.map(renderSkillRow)}
              </div>
            </>
          )
        ) : activeTab === 'library' ? (
          <div className="project-library-view">
            <div className="project-library-toolbar">
              <label className="project-library-search">
                <Search size={16} />
                <input
                  value={librarySearch}
                  onChange={(event) => setLibrarySearch(event.target.value)}
                  placeholder={t('projects.searchLibrary')}
                />
              </label>
              <div className="project-library-bulk-actions">
                <span>
                  {t('projects.selectedSkills', {
                    count: selectedLibrarySkillIds.length,
                  })}
                </span>
                <button
                  className="btn btn-secondary"
                  type="button"
                  onClick={toggleVisibleLibrarySkills}
                  disabled={loading || visibleAddableLibraryIds.length === 0}
                >
                  {visibleAddableLibraryIds.length > 0 &&
                  visibleAddableLibraryIds.every((skillId) =>
                    selectedLibrarySkillIds.includes(skillId),
                  )
                    ? t('projects.clearVisible')
                    : t('projects.selectVisible')}
                </button>
                <button
                  className="btn btn-primary"
                  type="button"
                  onClick={addSelectedLibrarySkills}
                  disabled={
                    loading ||
                    selectedLibrarySkillIds.length === 0 ||
                    !hasEnabledProjectAgents
                  }
                  title={
                    hasEnabledProjectAgents
                      ? undefined
                      : t('projects.noEnabledAgents')
                  }
                >
                  <Plus size={15} />
                  {t('projects.addSelected', {
                    count: selectedLibrarySkillIds.length,
                  })}
                </button>
              </div>
            </div>
            {filteredLibrary.length === 0 ? (
              <div className="project-section-empty">{t('projects.libraryEmpty')}</div>
            ) : (
              <div className="project-library-list">
                {filteredLibrary.map((skill) => (
                  <div
                    className={`project-library-row${selectedLibrarySkillIds.includes(skill.skill_id) ? ' bulk-selected' : ''}`}
                    key={skill.skill_id}
                  >
                    <div className="project-library-row-main">
                      {skill.availability === 'addable' ? (
                        <label
                          className="project-row-checkbox"
                          title={t('projects.selectSkill')}
                        >
                          <input
                            type="checkbox"
                            checked={selectedLibrarySkillIds.includes(
                              skill.skill_id,
                            )}
                            disabled={loading}
                            onChange={(event) =>
                              toggleLibrarySkill(
                                skill.skill_id,
                                event.target.checked,
                              )
                            }
                          />
                        </label>
                      ) : null}
                      <div>
                        <button
                          className="project-skill-title"
                          type="button"
                          onClick={() =>
                            onOpenSkillDetail({
                              title: skill.name,
                              description: skill.description,
                              source: {
                                kind: 'managed',
                                skillId: skill.skill_id,
                              },
                            })
                          }
                        >
                          {skill.name}
                        </button>
                        {skill.description ? <p>{skill.description}</p> : null}
                      </div>
                    </div>
                    <span className={`project-availability ${skill.availability}`}>
                      {t(`projects.availability.${skill.availability}`)}
                    </span>
                    {skill.availability === 'addable' ? (
                      <button
                        className="btn btn-primary"
                        type="button"
                        onClick={() => onAddLibrarySkill(skill.skill_id)}
                        disabled={loading || !hasEnabledProjectAgents}
                        title={
                          hasEnabledProjectAgents
                            ? undefined
                            : t('projects.noEnabledAgents')
                        }
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
                    ) : skill.availability === 'conflict' ? (
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={() => {
                          setActiveTab('attention')
                          onHandleLibraryConflict(skill.skill_id)
                        }}
                        disabled={loading}
                      >
                        <Wrench size={15} />
                        {t('projects.handleIssue')}
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
          <>
            {detail.issues.some((issue) => issue.can_ignore) ? (
              <div className="project-issues-toolbar">
                <button
                  className="btn btn-secondary"
                  type="button"
                  onClick={onIgnoreAllIssues}
                  disabled={loading}
                >
                  <EyeOff size={15} />
                  {t('projects.ignoreAll')}
                </button>
              </div>
            ) : null}
            <div className="project-issues-list">
              {detail.issues.map(renderIssue)}
            </div>
          </>
        )}
      </div>

      {pendingBulkRemoveIds.length > 0 ? (
        <div
          className="modal-backdrop"
          onClick={() => (loading ? null : setPendingBulkRemoveIds([]))}
        >
          <div
            className="modal modal-delete project-bulk-remove-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="project-bulk-remove-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="modal-header">
              <div className="modal-title" id="project-bulk-remove-title">
                {t('projects.bulkRemoveTitle', {
                  count: pendingBulkRemoveIds.length,
                })}
              </div>
              <button
                className="modal-close"
                type="button"
                onClick={() => setPendingBulkRemoveIds([])}
                disabled={loading}
                aria-label={t('close')}
              >
                <X size={18} />
              </button>
            </div>
            <div className="modal-body">
              <p>{t('projects.bulkRemoveBody')}</p>
              <div className="project-bulk-remove-list">
                {selectedCurrentSkillNames.map((name) => (
                  <span key={name}>{name}</span>
                ))}
              </div>
            </div>
            <div className="modal-footer">
              <button
                className="btn btn-secondary"
                type="button"
                onClick={() => setPendingBulkRemoveIds([])}
                disabled={loading}
              >
                {t('cancel')}
              </button>
              <button
                className="btn btn-danger"
                type="button"
                onClick={confirmBulkRemove}
                disabled={loading}
              >
                {t('projects.bulkRemoveConfirm', {
                  count: pendingBulkRemoveIds.length,
                })}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {resolutionIssue ? (
        <div className="modal-backdrop" onClick={closeIssueResolution}>
          <div
            className="modal project-issue-resolution-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="project-issue-resolution-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="project-issue-resolution-head">
              <div>
                <h2 id="project-issue-resolution-title">
                  {t('projects.resolutionDialog.title')}
                </h2>
                <p>
                  {t(
                    `projects.issues.${resolutionIssue.kind}.title`,
                    {
                      skill: resolutionIssue.skill_name ?? t('unknown'),
                      tool: resolutionIssue.tool_key
                        ? toolLabels[resolutionIssue.tool_key] ??
                          resolutionIssue.tool_key
                        : t('unknown'),
                    },
                  )}
                </p>
              </div>
              <button
                className="icon-btn"
                type="button"
                onClick={closeIssueResolution}
                disabled={loading}
                aria-label={t('cancel')}
              >
                <X size={18} />
              </button>
            </div>

            <fieldset className="project-issue-resolution-options">
              <legend>{t('projects.resolutionDialog.choose')}</legend>
              {resolutionIssue.actions.map((action, index) => (
                <label
                  className={selectedActionIndex === index ? 'selected' : ''}
                  key={`${action.kind}:${action.value ?? index}`}
                >
                  <input
                    type="radio"
                    name="project-issue-resolution"
                    checked={selectedActionIndex === index}
                    onChange={() => setSelectedActionIndex(index)}
                    disabled={loading}
                  />
                  <span>
                    <strong>
                      {t(`projects.issueActions.${action.kind}`)}
                    </strong>
                    {action.label ? <code>{action.label}</code> : null}
                    {action.destructive ? (
                      <small>{t('projects.resolutionDialog.changesFiles')}</small>
                    ) : null}
                  </span>
                </label>
              ))}
            </fieldset>

            <div className="project-issue-resolution-actions">
              <button
                className="btn btn-secondary"
                type="button"
                onClick={closeIssueResolution}
                disabled={loading}
              >
                {t('cancel')}
              </button>
              <button
                className={`btn ${
                  resolutionIssue.actions[selectedActionIndex]?.destructive
                    ? 'btn-danger-solid'
                    : 'btn-primary'
                }`}
                type="button"
                onClick={confirmIssueResolution}
                disabled={loading || !resolutionIssue.actions[selectedActionIndex]}
              >
                {t('projects.resolutionDialog.confirm')}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

export default memo(ProjectDetailPage)
