import { memo, useMemo, useState } from 'react'
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  EyeOff,
  FolderInput,
  FolderKanban,
  Globe2,
  RefreshCw,
  Save,
  Trash2,
  Wrench,
  X,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  AssignmentApplyResultDto,
  AssignmentDraftDto,
  AssignmentPreviewDto,
  ManagedSkill,
  ProjectSummaryDto,
  ToolInfoDto,
} from '../types'

type AssignmentModalProps = {
  open: boolean
  loading: boolean
  skills: ManagedSkill[]
  tools: ToolInfoDto[]
  projects: ProjectSummaryDto[]
  additive: boolean
  initialDraft?: AssignmentDraftDto | null
  onRequestClose: () => void
  onPreview: (draft: AssignmentDraftDto) => Promise<AssignmentPreviewDto | null>
  onApply: (
    draft: AssignmentDraftDto,
    replaceConflictKeys: string[],
  ) => Promise<AssignmentApplyResultDto | null>
  onIgnoreIssue: (
    skillId: string,
    issueKey: string,
    fingerprint: string,
  ) => Promise<void>
  onIgnoreAllIssues: (skillIds: string[]) => Promise<void>
  onRepairSkill: (skill: ManagedSkill) => Promise<void>
  onReimportSkill: (skill: ManagedSkill) => void
  onRemoveSkill: (skill: ManagedSkill) => void
  t: TFunction
}

const initialAssignmentState = (
  skills: ManagedSkill[],
  additive: boolean,
  initialDraft?: AssignmentDraftDto | null,
) => {
  const globalTools = new Set<string>()
  const projectTools: Record<string, string[]> = {}
  if (initialDraft) {
    for (const tool of initialDraft.global_tools) globalTools.add(tool)
    for (const project of initialDraft.projects) {
      projectTools[project.project_id] = Array.from(new Set(project.tool_keys))
    }
  } else if (skills.length === 1 && !additive) {
    for (const assignment of skills[0].assignments) {
      if (assignment.scope === 'global') {
        globalTools.add(assignment.tool)
      } else if (assignment.project_id) {
        projectTools[assignment.project_id] = Array.from(
          new Set([...(projectTools[assignment.project_id] ?? []), assignment.tool]),
        )
      }
    }
  }
  return {
    globalTools: Array.from(globalTools),
    projectTools,
    expandedProjects: Object.keys(projectTools),
  }
}

const AssignmentModal = ({
  open,
  loading,
  skills,
  tools,
  projects,
  additive,
  initialDraft,
  onRequestClose,
  onPreview,
  onApply,
  onIgnoreIssue,
  onIgnoreAllIssues,
  onRepairSkill,
  onReimportSkill,
  onRemoveSkill,
  t,
}: AssignmentModalProps) => {
  const initialState = initialAssignmentState(skills, additive, initialDraft)
  const [globalTools, setGlobalTools] = useState<string[]>(
    initialState.globalTools,
  )
  const [projectTools, setProjectTools] = useState<Record<string, string[]>>(
    initialState.projectTools,
  )
  const [expandedProjects, setExpandedProjects] = useState<string[]>(
    initialState.expandedProjects,
  )
  const [preview, setPreview] = useState<AssignmentPreviewDto | null>(null)
  const [replaceConflictKeys, setReplaceConflictKeys] = useState<string[]>([])

  const toolsByKey = useMemo(
    () => Object.fromEntries(tools.map((tool) => [tool.key, tool])),
    [tools],
  )
  const projectCapableTools = useMemo(
    () => tools.filter((tool) => tool.supports_project_scope),
    [tools],
  )
  const conflicts = useMemo(
    () => preview?.items.filter((item) => item.outcome === 'conflict') ?? [],
    [preview],
  )
  const assignmentIssues = useMemo(
    () =>
      skills.flatMap((skill) =>
        skill.assignments
          .filter(
            (assignment) =>
              assignment.state === 'error' &&
              !assignment.issue_ignored &&
              assignment.issue_kind !== 'source_missing' &&
              assignment.issue_kind !== 'source_invalid',
          )
          .map((assignment) => ({ skill, assignment })),
      ),
    [skills],
  )
  const sourceIssues = useMemo(
    () => skills.filter((skill) => Boolean(skill.source_issue)),
    [skills],
  )
  const existingIssueCount = assignmentIssues.length + sourceIssues.length
  const ignorableIssues = useMemo(
    () => assignmentIssues.filter(({ assignment }) => assignment.can_ignore),
    [assignmentIssues],
  )
  const draft = useMemo<AssignmentDraftDto>(
    () => ({
      global_tools: globalTools,
      projects: Object.entries(projectTools)
        .filter(([, toolKeys]) => toolKeys.length > 0)
        .map(([projectId, toolKeys]) => ({
          project_id: projectId,
          tool_keys: toolKeys,
        })),
    }),
    [globalTools, projectTools],
  )

  if (!open) return null

  const toggleGlobalTool = (toolKey: string, checked: boolean) => {
    setGlobalTools((current) =>
      checked
        ? Array.from(new Set([...current, toolKey]))
        : current.filter((key) => key !== toolKey),
    )
  }

  const toggleProject = (project: ProjectSummaryDto, checked: boolean) => {
    setProjectTools((current) => {
      if (!checked) {
        const next = { ...current }
        delete next[project.id]
        return next
      }
      const inherited = project.enabled_tool_keys.filter((toolKey) => {
        const tool = toolsByKey[toolKey]
        return Boolean(tool?.enabled && tool.supports_project_scope)
      })
      return { ...current, [project.id]: inherited }
    })
    setExpandedProjects((current) =>
      checked
        ? Array.from(new Set([...current, project.id]))
        : current.filter((id) => id !== project.id),
    )
  }

  const toggleProjectTool = (
    projectId: string,
    toolKey: string,
    checked: boolean,
  ) => {
    setProjectTools((current) => {
      const selected = current[projectId] ?? []
      return {
        ...current,
        [projectId]: checked
          ? Array.from(new Set([...selected, toolKey]))
          : selected.filter((key) => key !== toolKey),
      }
    })
  }

  const handlePreview = async () => {
    const result = await onPreview(draft)
    if (!result) return
    setPreview(result)
    setReplaceConflictKeys([])
  }

  const handleApply = async () => {
    await onApply(draft, replaceConflictKeys)
  }

  return (
    <div className="modal-backdrop" onClick={loading ? undefined : onRequestClose}>
      <div
        className="modal assignment-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="assignment-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <div className="modal-title" id="assignment-modal-title">
              {skills.length === 1
                ? t('assignment.title', { name: skills[0]?.name ?? '' })
                : t('assignment.batchTitle', { count: skills.length })}
            </div>
            {additive ? (
              <div className="assignment-mode-label">{t('assignment.additive')}</div>
            ) : null}
          </div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            disabled={loading}
            aria-label={t('close')}
            title={t('close')}
          >
            <X size={18} />
          </button>
        </div>

        <div className="modal-body assignment-body">
          {preview ? (
            <div className="assignment-conflicts">
              <div className="assignment-section-title">
                {conflicts.length > 0 ||
                preview.items.some((item) => item.outcome === 'remove') ? (
                  <AlertTriangle size={17} />
                ) : (
                  <Check size={17} />
                )}
                <span>
                  {t('assignment.previewTitle', { count: preview.items.length })}
                </span>
              </div>
              <div className="assignment-conflict-list">
                {preview.items.map((item) => {
                  const replace = replaceConflictKeys.includes(item.key)
                  const tool = toolsByKey[item.tool]
                  return (
                    <div className="assignment-conflict-row" key={item.key}>
                      <div className="assignment-conflict-main">
                        <strong>
                          {item.project_name
                            ? `${item.project_name} / ${tool?.label ?? item.tool}`
                            : tool?.label ?? item.tool}
                        </strong>
                        {item.target_path ? <code>{item.target_path}</code> : null}
                      </div>
                      {item.outcome === 'conflict' ? (
                        <div className="assignment-resolution" role="radiogroup">
                          <button
                            type="button"
                            className={!replace ? 'active' : ''}
                            onClick={() =>
                              setReplaceConflictKeys((current) =>
                                current.filter((key) => key !== item.key),
                              )
                            }
                            disabled={loading}
                          >
                            {!replace ? <Check size={14} /> : null}
                            {t('assignment.keepExisting')}
                          </button>
                          <button
                            type="button"
                            className={replace ? 'active danger' : ''}
                            onClick={() =>
                              setReplaceConflictKeys((current) =>
                                Array.from(new Set([...current, item.key])),
                              )
                            }
                            disabled={loading}
                          >
                            {replace ? <Check size={14} /> : null}
                            {t('assignment.replaceWithLibrary')}
                          </button>
                        </div>
                      ) : (
                        <span className={`assignment-preview-outcome ${item.outcome}`}>
                          {t(`assignment.outcomes.${item.outcome}`)}
                        </span>
                      )}
                    </div>
                  )
                })}
                {preview.items.length === 0 ? (
                  <div className="assignment-empty">{t('assignment.noChanges')}</div>
                ) : null}
              </div>
            </div>
          ) : (
            <>
              {existingIssueCount > 0 ? (
                <section className="assignment-section assignment-existing-issues">
                  <div className="assignment-section-title">
                    <AlertTriangle size={17} />
                    <span>
                      {t('assignment.existingIssues', {
                        count: existingIssueCount,
                      })}
                    </span>
                    <button
                      className="btn btn-secondary assignment-retry-btn"
                      type="button"
                      onClick={() => void handlePreview()}
                      disabled={loading}
                    >
                      {t('assignment.retryAll')}
                    </button>
                    {ignorableIssues.length > 0 ? (
                      <button
                        className="btn btn-secondary assignment-ignore-all-btn"
                        type="button"
                        onClick={() =>
                          void onIgnoreAllIssues(
                            Array.from(new Set(skills.map((skill) => skill.id))),
                          )
                        }
                        disabled={loading}
                      >
                        <EyeOff size={15} />
                        {t('assignment.ignoreAll')}
                      </button>
                    ) : null}
                  </div>
                  <div className="assignment-issue-list">
                    {sourceIssues.map((skill) => (
                      <div
                        className="assignment-issue-row"
                        key={`${skill.id}-${skill.source_issue}`}
                      >
                        <div className="assignment-issue-main">
                          <strong>{skill.name}</strong>
                          <span>
                            {t(`assignment.issues.${skill.source_issue}`, {
                              defaultValue: t('unknown'),
                            })}
                          </span>
                        </div>
                        <div className="assignment-issue-actions">
                          {skill.source_ref ? (
                            <button
                              className="btn btn-secondary"
                              type="button"
                              onClick={() => void onRepairSkill(skill)}
                              disabled={loading}
                            >
                              <RefreshCw size={14} />
                              {t('assignment.restoreFromSource')}
                            </button>
                          ) : null}
                          <button
                            className="btn btn-secondary"
                            type="button"
                            onClick={() => onReimportSkill(skill)}
                            disabled={loading}
                          >
                            <FolderInput size={14} />
                            {t('assignment.reimport')}
                          </button>
                          <button
                            className="btn btn-secondary danger"
                            type="button"
                            onClick={() => onRemoveSkill(skill)}
                            disabled={loading}
                          >
                            <Trash2 size={14} />
                            {t('remove')}
                          </button>
                        </div>
                      </div>
                    ))}
                    {assignmentIssues.map(({ skill, assignment }) => {
                      const canIgnore =
                        assignment.can_ignore &&
                        Boolean(assignment.issue_key) &&
                        Boolean(assignment.issue_fingerprint)
                      return (
                        <div
                          className="assignment-issue-row"
                          key={`${skill.id}-${assignment.scope}-${assignment.project_id ?? ''}-${assignment.tool}`}
                        >
                          <div className="assignment-issue-main">
                            <strong>
                              {skills.length > 1 ? `${skill.name} / ` : ''}
                              {assignment.project_name
                                ? `${assignment.project_name} / `
                                : ''}
                              {toolsByKey[assignment.tool]?.label ?? assignment.tool}
                            </strong>
                            <span>
                              {assignment.issue_kind
                                ? t(`assignment.issues.${assignment.issue_kind}`, {
                                    defaultValue:
                                      assignment.last_error ?? t('unknown'),
                                  })
                                : assignment.last_error ?? t('unknown')}
                            </span>
                          </div>
                          <div className="assignment-issue-actions">
                            <button
                              className="btn btn-secondary"
                              type="button"
                              onClick={() => void handlePreview()}
                              disabled={loading}
                            >
                              <Wrench size={14} />
                              {t('assignment.handleIssue')}
                            </button>
                            {canIgnore ? (
                              <button
                                className="btn btn-secondary"
                                type="button"
                                onClick={() =>
                                  void onIgnoreIssue(
                                    skill.id,
                                    assignment.issue_key as string,
                                    assignment.issue_fingerprint as string,
                                  )
                                }
                                disabled={loading}
                              >
                                <EyeOff size={14} />
                                {t('assignment.ignoreIssue')}
                              </button>
                            ) : null}
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </section>
              ) : null}
              <section className="assignment-section">
                <div className="assignment-section-title">
                  <Globe2 size={17} />
                  <span>{t('assignment.globalTitle')}</span>
                  <span className="assignment-count">{globalTools.length}</span>
                </div>
                <div className="assignment-tool-grid">
                  {tools.map((tool) => {
                    const checked = globalTools.includes(tool.key)
                    const available = tool.enabled && tool.installed
                    return (
                      <label
                        className={`assignment-tool-option${checked ? ' selected' : ''}${!available ? ' unavailable' : ''}`}
                        key={tool.key}
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          disabled={loading || (!available && !checked)}
                          onChange={(event) =>
                            toggleGlobalTool(tool.key, event.target.checked)
                          }
                        />
                        <span>{tool.label}</span>
                        {!available ? <small>{t('assignment.unavailable')}</small> : null}
                      </label>
                    )
                  })}
                </div>
              </section>

              <section className="assignment-section project-assignment-section">
                <div className="assignment-section-title">
                  <FolderKanban size={17} />
                  <span>{t('assignment.projectsTitle')}</span>
                  <span className="assignment-count">
                    {Object.values(projectTools).filter((keys) => keys.length > 0).length}
                  </span>
                </div>
                {projects.length === 0 ? (
                  <div className="assignment-empty">{t('assignment.noProjects')}</div>
                ) : (
                  <div className="assignment-project-list">
                    {projects.map((project) => {
                      const selected = Object.hasOwn(projectTools, project.id)
                      const expanded = expandedProjects.includes(project.id)
                      const selectedTools = projectTools[project.id] ?? []
                      return (
                        <div className="assignment-project-row" key={project.id}>
                          <div className="assignment-project-header">
                            <button
                              className="assignment-expand"
                              type="button"
                              onClick={() =>
                                setExpandedProjects((current) =>
                                  expanded
                                    ? current.filter((id) => id !== project.id)
                                    : [...current, project.id],
                                )
                              }
                              aria-label={project.name}
                              title={project.name}
                            >
                              {expanded ? (
                                <ChevronDown size={16} />
                              ) : (
                                <ChevronRight size={16} />
                              )}
                            </button>
                            <label className="assignment-project-check">
                              <input
                                type="checkbox"
                                checked={selected}
                                disabled={loading}
                                onChange={(event) =>
                                  toggleProject(project, event.target.checked)
                                }
                              />
                              <span>
                                <strong>{project.name}</strong>
                                <code>{project.root_path}</code>
                              </span>
                            </label>
                            {selected ? (
                              <span className="assignment-project-count">
                                {t('assignment.agentCount', {
                                  count: selectedTools.length,
                                })}
                              </span>
                            ) : null}
                          </div>
                          {expanded ? (
                            <div className="assignment-project-tools">
                              {projectCapableTools.map((tool) => {
                                const checked = selectedTools.includes(tool.key)
                                const available =
                                  tool.enabled &&
                                  project.enabled_tool_keys.includes(tool.key)
                                return (
                                  <label
                                    className={`${checked ? 'selected' : ''}${!available ? ' unavailable' : ''}`}
                                    key={`${project.id}-${tool.key}`}
                                  >
                                    <input
                                      type="checkbox"
                                      checked={checked}
                                      disabled={
                                        loading ||
                                        !selected ||
                                        (!available && !checked)
                                      }
                                      onChange={(event) =>
                                        toggleProjectTool(
                                          project.id,
                                          tool.key,
                                          event.target.checked,
                                        )
                                      }
                                    />
                                    <span>{tool.label}</span>
                                    {!available ? (
                                      <small>{t('assignment.unavailable')}</small>
                                    ) : null}
                                  </label>
                                )
                              })}
                            </div>
                          ) : null}
                        </div>
                      )
                    })}
                  </div>
                )}
              </section>
            </>
          )}
        </div>

        <div className="modal-footer assignment-footer">
          {preview ? (
            <button
              className="btn btn-secondary"
              type="button"
              onClick={() => setPreview(null)}
              disabled={loading}
            >
              {t('back')}
            </button>
          ) : (
            <button
              className="btn btn-secondary"
              type="button"
              onClick={onRequestClose}
              disabled={loading}
            >
              {t('cancel')}
            </button>
          )}
          <button
            className="btn btn-primary"
            type="button"
            onClick={() => void (preview ? handleApply() : handlePreview())}
            disabled={loading || skills.length === 0}
          >
            <Save size={16} />
            {preview ? t('assignment.confirmApply') : t('assignment.save')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(AssignmentModal)
