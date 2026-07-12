import { memo, useMemo, useState } from 'react'
import {
  AlertTriangle,
  ArrowRight,
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
  AssignmentPreviewItemDto,
  AssignmentPreviewDto,
  ManagedSkill,
  ProjectSummaryDto,
  ToolInfoDto,
} from '../types'
import {
  getExpandedProjectsAfterToggle,
  getVisibleAssignmentProjects,
} from '../assignmentModalState'
import {
  isGlobalToolAvailable,
  isProjectToolAvailable,
} from '../toolAvailability'
import {
  assignmentDraftFromTemplate,
  buildBulkAssignmentPreviewSummary,
  EMPTY_ASSIGNMENT_DRAFT,
  getBulkAssignmentDraft,
  mergeRetriedAssignmentResult,
  requiresBulkClearAcknowledgement,
  withSourceBlockedSkills,
  type BulkAssignmentMode,
  type BulkAssignmentOrigin,
} from '../bulkAssignmentWorkflow'

type AssignmentModalProps = {
  open: boolean
  loading: boolean
  skills: ManagedSkill[]
  tools: ToolInfoDto[]
  projects: ProjectSummaryDto[]
  contextProjectId?: string
  additive: boolean
  libraryBatch?: boolean
  projectBatchReplace?: boolean
  requireProjectTools?: boolean
  initialDraft?: AssignmentDraftDto | null
  onRequestClose: () => void
  onPreview: (
    draft: AssignmentDraftDto,
    additiveOverride?: boolean,
    skillIds?: string[],
  ) => Promise<AssignmentPreviewDto | null>
  onApply: (
    draft: AssignmentDraftDto,
    replaceConflictKeys: string[],
    additiveOverride?: boolean,
    skillIds?: string[],
  ) => Promise<AssignmentApplyResultDto | null>
  onBatchFinished?: () => void
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
  contextProjectId,
  additive,
  libraryBatch = false,
  projectBatchReplace = false,
  requireProjectTools = false,
  initialDraft,
  onRequestClose,
  onPreview,
  onApply,
  onBatchFinished,
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
  const [bulkMode, setBulkMode] = useState<BulkAssignmentMode | null>(
    libraryBatch ? null : additive ? 'add' : 'replace',
  )
  const [bulkOrigin, setBulkOrigin] = useState<BulkAssignmentOrigin | null>(null)
  const [bulkTemplateId, setBulkTemplateId] = useState('')
  const [bulkStep, setBulkStep] = useState<'mode' | 'configure' | 'preview' | 'result'>(
    libraryBatch ? 'mode' : 'configure',
  )
  const [bulkResult, setBulkResult] = useState<AssignmentApplyResultDto | null>(null)
  const [acknowledgedClearKey, setAcknowledgedClearKey] = useState<string | null>(null)

  const toolsByKey = useMemo(
    () => Object.fromEntries(tools.map((tool) => [tool.key, tool])),
    [tools],
  )
  const projectCapableTools = useMemo(
    () => tools.filter((tool) => tool.supports_project_scope),
    [tools],
  )
  const visibleProjects = useMemo(
    () => getVisibleAssignmentProjects(projects, contextProjectId),
    [contextProjectId, projects],
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
  const effectiveDraft = getBulkAssignmentDraft(bulkMode ?? 'replace', draft)
  const requiresClearAcknowledgement =
    libraryBatch &&
    requiresBulkClearAcknowledgement(bulkMode ?? 'replace', effectiveDraft)
  const clearAcknowledgementKey = JSON.stringify([bulkMode, effectiveDraft])
  const clearAcknowledged = acknowledgedClearKey === clearAcknowledgementKey
  const effectiveAdditive = libraryBatch ? bulkMode === 'add' : additive
  const sourceBlockedSkills = useMemo(
    () =>
      bulkMode === 'clear'
        ? []
        : skills.filter((skill) => Boolean(skill.source_issue)),
    [bulkMode, skills],
  )
  const eligibleSkills = useMemo(() => {
    const blockedIds = new Set(sourceBlockedSkills.map((skill) => skill.id))
    return skills.filter((skill) => !blockedIds.has(skill.id))
  }, [skills, sourceBlockedSkills])
  const previewSummary = preview
    ? buildBulkAssignmentPreviewSummary(preview)
    : null
  const previewGroups = useMemo(() => {
    const groups = new Map<string, { skillName: string; items: AssignmentPreviewItemDto[] }>()
    for (const item of preview?.items ?? []) {
      const group = groups.get(item.skill_id) ?? {
        skillName: item.skill_name,
        items: [],
      }
      group.items.push(item)
      groups.set(item.skill_id, group)
    }
    return Array.from(groups, ([skillId, group]) => ({ skillId, ...group }))
  }, [preview])

  const selectedContextProjectTools = contextProjectId
    ? projectTools[contextProjectId] ?? []
    : []

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
        return Boolean(
          tool?.supports_project_scope &&
            isProjectToolAvailable(tool, project.enabled_tool_keys),
        )
      })
      return { ...current, [project.id]: inherited }
    })
    setExpandedProjects((current) =>
      getExpandedProjectsAfterToggle(current, project.id),
    )
  }

  const toggleProjectTool = (
    projectId: string,
    toolKey: string,
    checked: boolean,
  ) => {
    setProjectTools((current) => {
      const selected = current[projectId] ?? []
      const nextSelected = checked
        ? Array.from(new Set([...selected, toolKey]))
        : selected.filter((key) => key !== toolKey)
      if (nextSelected.length === 0) {
        const next = { ...current }
        delete next[projectId]
        return next
      }
      return { ...current, [projectId]: nextSelected }
    })
  }

  const handlePreview = async () => {
    const skillIds = libraryBatch ? eligibleSkills.map((skill) => skill.id) : undefined
    const result = await onPreview(effectiveDraft, effectiveAdditive, skillIds)
    if (!result) return
    setPreview(result)
    setReplaceConflictKeys([])
    if (libraryBatch) setBulkStep('preview')
  }

  const handleApply = async () => {
    const skillIds = libraryBatch ? eligibleSkills.map((skill) => skill.id) : undefined
    const result = await onApply(
      effectiveDraft,
      replaceConflictKeys,
      effectiveAdditive,
      skillIds,
    )
    if (libraryBatch && result) {
      setBulkResult(withSourceBlockedSkills(result, sourceBlockedSkills))
      setBulkStep('result')
    }
  }

  const handleRetryFailed = async () => {
    if (!bulkResult) return
    const failedIds = bulkResult.skills
      .filter((skill) => skill.failed > 0)
      .map((skill) => skill.skill_id)
    if (failedIds.length === 0) return
    const result = await onApply(
      effectiveDraft,
      replaceConflictKeys,
      effectiveAdditive,
      failedIds,
    )
    if (result) {
      setBulkResult((current) =>
        current ? mergeRetriedAssignmentResult(current, result) : result,
      )
    }
  }

  const isHistoricalUnavailableRemoval = (item: AssignmentPreviewItemDto) => {
    if (item.outcome !== 'remove' && item.outcome !== 'unsafe') return false
    const tool = toolsByKey[item.tool]
    if (!tool) return true
    if (item.scope === 'global') return !isGlobalToolAvailable(tool)
    const project = projects.find((candidate) => candidate.id === item.project_id)
    return !project || !isProjectToolAvailable(tool, project.enabled_tool_keys)
  }

  const renderPreviewItem = (item: AssignmentPreviewItemDto) => {
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
          {isHistoricalUnavailableRemoval(item) ? (
            <small className="historical-unavailable-removal">
              {t('assignment.unavailableRemoval')}
            </small>
          ) : null}
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
  }

  return (
    <div className="modal-backdrop" onClick={loading ? undefined : onRequestClose}>
      <div
        className={`modal assignment-modal${libraryBatch ? ' bulk-assignment-workflow' : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="assignment-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <div className="modal-title" id="assignment-modal-title">
              {libraryBatch
                ? t('assignment.bulkWorkflowTitle', { count: skills.length })
                : skills.length === 1
                ? t('assignment.title', { name: skills[0]?.name ?? '' })
                : t('assignment.batchTitle', { count: skills.length })}
            </div>
            {libraryBatch ? (
              <div className="bulk-assignment-steps" aria-label={t('assignment.steps.label')}>
                {(['mode', 'configure', 'preview', 'result'] as const).map(
                  (step, index) => (
                    <span
                      className={bulkStep === step ? 'active' : ''}
                      key={step}
                    >
                      {index + 1}. {t(`assignment.steps.${step}`)}
                    </span>
                  ),
                )}
              </div>
            ) : additive || projectBatchReplace ? (
              <div className="assignment-mode-label">
                {t(
                  projectBatchReplace
                    ? 'assignment.projectBatchReplace'
                    : 'assignment.additive',
                )}
              </div>
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
          {libraryBatch && bulkStep === 'mode' ? (
            <div className="bulk-assignment-mode-grid">
              {(['add', 'replace', 'clear'] as const).map((mode) => (
                <button
                  className={`bulk-assignment-mode-card${mode === 'clear' ? ' danger' : ''}`}
                  type="button"
                  key={mode}
                  onClick={() => {
                    setBulkMode(mode)
                    setBulkOrigin(mode === 'replace' ? null : 'blank')
                    const startingDraft =
                      mode === 'add' && initialDraft
                        ? initialDraft
                        : EMPTY_ASSIGNMENT_DRAFT
                    setGlobalTools(startingDraft.global_tools)
                    setProjectTools(
                      Object.fromEntries(
                        startingDraft.projects.map((project) => [
                          project.project_id,
                          project.tool_keys,
                        ]),
                      ),
                    )
                    setPreview(null)
                    setBulkStep('configure')
                  }}
                >
                  <strong>{t(`assignment.bulkModes.${mode}.title`)}</strong>
                  <span>{t(`assignment.bulkModes.${mode}.description`)}</span>
                  <ArrowRight size={18} />
                </button>
              ))}
            </div>
          ) : libraryBatch && bulkStep === 'result' && bulkResult ? (
            <div className="bulk-assignment-result">
              <div className="bulk-assignment-result-summary">
                <Check size={22} />
                <div>
                  <strong>{t('assignment.resultTitle')}</strong>
                  <span>
                    {t('assignment.resultSummary', {
                      succeeded: bulkResult.succeeded,
                      failed: bulkResult.failed,
                      skipped: bulkResult.skipped,
                    })}
                  </span>
                </div>
              </div>
              <div className="bulk-assignment-result-list">
                {bulkResult.skills.map((skill) => (
                  <div
                    className={skill.failed > 0 ? 'failed' : 'success'}
                    key={skill.skill_id}
                  >
                    <strong>{skill.skill_name}</strong>
                    <span>
                      {skill.failed > 0
                        ? t('assignment.resultFailed', { count: skill.failed })
                        : skill.skipped > 0
                          ? t('assignment.resultSkipped', { count: skill.skipped })
                          : t('assignment.resultSucceeded')}
                    </span>
                    {skill.errors.map((error) => (
                      <code key={error}>{error}</code>
                    ))}
                  </div>
                ))}
              </div>
            </div>
          ) : preview ? (
            <div className="assignment-conflicts">
              {libraryBatch && previewSummary ? (
                <div className="bulk-assignment-preview-summary">
                  <div>
                    <span>{t('assignment.previewAffected')}</span>
                    <strong>{previewSummary.affectedSkillCount}</strong>
                  </div>
                  <div>
                    <span>{t('assignment.previewRemove')}</span>
                    <strong>{previewSummary.removeCount}</strong>
                  </div>
                  <div>
                    <span>{t('assignment.previewAdd')}</span>
                    <strong>{previewSummary.addCount}</strong>
                  </div>
                  <div>
                    <span>{t('assignment.previewKeep')}</span>
                    <strong>{previewSummary.keepCount}</strong>
                  </div>
                  <div className={previewSummary.blockedCount > 0 ? 'danger' : ''}>
                    <span>{t('assignment.previewBlocked')}</span>
                    <strong>{previewSummary.blockedCount}</strong>
                  </div>
                </div>
              ) : null}
              {libraryBatch && sourceBlockedSkills.length > 0 ? (
                <div className="bulk-assignment-source-blocked">
                  <AlertTriangle size={17} />
                  <div>
                    <span>
                      {t('assignment.sourceBlocked', {
                        count: sourceBlockedSkills.length,
                      })}
                    </span>
                    <strong>
                      {sourceBlockedSkills.map((skill) => skill.name).join('、')}
                    </strong>
                  </div>
                </div>
              ) : null}
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
              {libraryBatch && conflicts.length > 0 ? (
                <div className="bulk-conflict-actions">
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={() => setReplaceConflictKeys([])}
                    disabled={loading}
                  >
                    {t('assignment.keepAllConflicts')}
                  </button>
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={() =>
                      setReplaceConflictKeys(conflicts.map((item) => item.key))
                    }
                    disabled={loading}
                  >
                    {t('assignment.replaceAllConflicts')}
                  </button>
                </div>
              ) : null}
              <div className="assignment-conflict-list">
                {libraryBatch
                  ? previewGroups.map((group) => (
                      <details
                        className="bulk-preview-skill-group"
                        key={group.skillId}
                        open
                      >
                        <summary>
                          <strong>{group.skillName}</strong>
                          <span>
                            {t('assignment.changeCount', {
                              count: group.items.length,
                            })}
                          </span>
                        </summary>
                        <div>
                          {group.items.some(isHistoricalUnavailableRemoval) ? (
                            <section className="historical-unavailable-group">
                              <strong>{t('assignment.unavailableRemovalGroup')}</strong>
                              {group.items
                                .filter(isHistoricalUnavailableRemoval)
                                .map(renderPreviewItem)}
                            </section>
                          ) : null}
                          {group.items
                            .filter((item) => !isHistoricalUnavailableRemoval(item))
                            .map(renderPreviewItem)}
                        </div>
                      </details>
                    ))
                  : preview.items.map(renderPreviewItem)}
                {preview.items.length === 0 ? (
                  <div className="assignment-empty">{t('assignment.noChanges')}</div>
                ) : null}
              </div>
              {requiresClearAcknowledgement ? (
                <label className="bulk-clear-acknowledgement">
                  <input
                    type="checkbox"
                    checked={clearAcknowledged}
                    onChange={(event) =>
                      setAcknowledgedClearKey(
                        event.target.checked ? clearAcknowledgementKey : null,
                      )
                    }
                  />
                  <span>
                    {t('assignment.clearAcknowledgement', {
                      count: eligibleSkills.length,
                    })}
                  </span>
                </label>
              ) : null}
            </div>
          ) : (
            <>
              {libraryBatch ? (
                <div className={`bulk-assignment-config-intro${bulkMode === 'clear' ? ' danger' : ''}`}>
                  <strong>{t(`assignment.bulkModes.${bulkMode}.title`)}</strong>
                  <span>
                    {t(
                      bulkMode === 'add'
                        ? 'assignment.addConfigHint'
                        : bulkMode === 'replace'
                          ? 'assignment.replaceConfigHint'
                          : 'assignment.clearConfigHint',
                      {
                        count:
                          bulkMode === 'clear'
                            ? skills.length
                            : eligibleSkills.length,
                      },
                    )}
                  </span>
                  {bulkMode === 'replace' ? (
                    <div className="bulk-assignment-origin-picker">
                      <button
                        className={bulkOrigin === 'blank' ? 'selected' : ''}
                        type="button"
                        onClick={() => {
                          setBulkOrigin('blank')
                          setBulkTemplateId('')
                          setGlobalTools([])
                          setProjectTools({})
                        }}
                      >
                        <strong>{t('assignment.originBlank')}</strong>
                        <span>{t('assignment.originBlankHint')}</span>
                      </button>
                      <label className={bulkOrigin === 'template' ? 'selected' : ''}>
                        <strong>{t('assignment.originTemplate')}</strong>
                        <select
                          value={bulkTemplateId}
                          onChange={(event) => {
                            const templateId = event.target.value
                            setBulkTemplateId(templateId)
                            if (!templateId) {
                              setBulkOrigin(null)
                              return
                            }
                            const template = skills.find(
                              (skill) => skill.id === templateId,
                            )
                            if (!template) return
                            const next = assignmentDraftFromTemplate(
                              template,
                              tools,
                              projects,
                            )
                            setBulkOrigin('template')
                            setGlobalTools(next.global_tools)
                            setProjectTools(
                              Object.fromEntries(
                                next.projects.map((project) => [
                                  project.project_id,
                                  project.tool_keys,
                                ]),
                              ),
                            )
                          }}
                        >
                          <option value="">{t('assignment.selectTemplate')}</option>
                          {skills.map((skill) => (
                            <option value={skill.id} key={skill.id}>
                              {skill.name}
                            </option>
                          ))}
                        </select>
                        <span>{t('assignment.originTemplateHint')}</span>
                      </label>
                    </div>
                  ) : null}
                </div>
              ) : null}
              {!libraryBatch && existingIssueCount > 0 ? (
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
              {!contextProjectId &&
              bulkMode !== 'clear' &&
              (!libraryBatch || bulkMode !== 'replace' || bulkOrigin) ? (
                <section className="assignment-section">
                  <div className="assignment-section-title">
                    <Globe2 size={17} />
                    <span>{t('assignment.globalTitle')}</span>
                    <span className="assignment-count">{globalTools.length}</span>
                  </div>
                  <div className="assignment-tool-grid">
                    {tools.map((tool) => {
                      const checked = globalTools.includes(tool.key)
                      const available = isGlobalToolAvailable(tool)
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
                          {!available ? (
                            <small>{t('assignment.unavailable')}</small>
                          ) : null}
                        </label>
                      )
                    })}
                  </div>
                </section>
              ) : null}

              {bulkMode !== 'clear' &&
              (!libraryBatch || bulkMode !== 'replace' || bulkOrigin) ? (
              <section className="assignment-section project-assignment-section">
                <div className="assignment-section-title">
                  <FolderKanban size={17} />
                  <span>{t('assignment.projectsTitle')}</span>
                  <span className="assignment-count">
                    {Object.values(projectTools).filter((keys) => keys.length > 0).length}
                  </span>
                </div>
                {visibleProjects.length === 0 ? (
                  <div className="assignment-empty">{t('assignment.noProjects')}</div>
                ) : (
                  <div className="assignment-project-list">
                    {visibleProjects.map((project) => {
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
                                const available = isProjectToolAvailable(
                                  tool,
                                  project.enabled_tool_keys,
                                )
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
                {requireProjectTools && selectedContextProjectTools.length === 0 ? (
                  <div className="assignment-empty">
                    {t('assignment.selectAtLeastOneProjectAgent')}
                  </div>
                ) : null}
              </section>
              ) : null}
            </>
          )}
        </div>

        <div className="modal-footer assignment-footer">
          {libraryBatch && bulkStep === 'mode' ? (
            <button
              className="btn btn-secondary"
              type="button"
              onClick={onRequestClose}
              disabled={loading}
            >
              {t('cancel')}
            </button>
          ) : libraryBatch && bulkStep === 'result' ? (
            <>
              {bulkResult?.skills.some((skill) => skill.failed > 0) ? (
                <button
                  className="btn btn-secondary"
                  type="button"
                  onClick={() => void handleRetryFailed()}
                  disabled={loading}
                >
                  <RefreshCw size={16} />
                  {t('assignment.retryFailed')}
                </button>
              ) : null}
              <button
                className="btn btn-primary"
                type="button"
                onClick={onBatchFinished ?? onRequestClose}
                disabled={loading}
              >
                <Check size={16} />
                {t('done')}
              </button>
            </>
          ) : (
            <>
              <button
                className="btn btn-secondary"
                type="button"
                onClick={() => {
                  if (preview) {
                    setPreview(null)
                    if (libraryBatch) setBulkStep('configure')
                  } else if (libraryBatch) {
                    setBulkStep('mode')
                    setBulkMode(null)
                  } else {
                    onRequestClose()
                  }
                }}
                disabled={loading}
              >
                {preview || libraryBatch ? t('back') : t('cancel')}
              </button>
              <button
                className={`btn ${libraryBatch && previewSummary && previewSummary.removeCount > 0 ? 'btn-danger' : 'btn-primary'}`}
                type="button"
                onClick={() => void (preview ? handleApply() : handlePreview())}
                disabled={
                  loading ||
                  skills.length === 0 ||
                  eligibleSkills.length === 0 ||
                  (requireProjectTools && selectedContextProjectTools.length === 0) ||
                  (libraryBatch && bulkMode === 'replace' && !bulkOrigin) ||
                  (requiresClearAcknowledgement &&
                    Boolean(preview) &&
                    !clearAcknowledged)
                }
              >
                <Save size={16} />
                {libraryBatch && preview && previewSummary
                  ? t('assignment.confirmDestructiveApply', {
                      remove: previewSummary.removeCount,
                      add: previewSummary.addCount,
                    })
                  : preview
                    ? t('assignment.confirmApply')
                    : t('assignment.reviewChanges')}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

export default memo(AssignmentModal)
