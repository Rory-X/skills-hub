import { memo, useState } from 'react'
import {
  AlertTriangle,
  Box,
  Copy,
  Folder,
  Github,
  Power,
  RefreshCw,
  Share2,
  Tag,
  Trash2,
} from 'lucide-react'
import { toast } from 'sonner'
import type { TFunction } from 'i18next'
import type { ManagedSkill, ToolOption } from './types'

type GithubInfo = {
  label: string
  href: string
}

type SkillCardProps = {
  skill: ManagedSkill
  installedTools: ToolOption[]
  loading: boolean
  bulkMode: boolean
  bulkSelected: boolean
  highlighted: boolean
  getGithubInfo: (url: string | null | undefined) => GithubInfo | null
  getSkillSourceLabel: (skill: ManagedSkill) => string
  formatRelative: (ms: number | null | undefined) => string
  onUpdate: (skill: ManagedSkill) => void
  onDelete: (skillId: string) => void
  onToggleEnabled: (skill: ManagedSkill) => void
  onOpenScope: (skill: ManagedSkill) => void
  onOpenDetail: (skill: ManagedSkill) => void
  onEditTags: (skill: ManagedSkill) => void
  onToggleBulkSelection: (skillId: string) => void
  getSkillScope: (skill: ManagedSkill) => 'global' | 'project'
  getSkillProjects: (skill: ManagedSkill) => string[]
  t: TFunction
}

const MAX_VISIBLE_BADGES = 5

const SkillCard = ({
  skill,
  installedTools,
  loading,
  bulkMode,
  bulkSelected,
  highlighted,
  getGithubInfo,
  getSkillSourceLabel,
  formatRelative,
  onUpdate,
  onDelete,
  onToggleEnabled,
  onOpenScope,
  onOpenDetail,
  onEditTags,
  onToggleBulkSelection,
  getSkillScope,
  getSkillProjects,
  t,
}: SkillCardProps) => {
  const typeKey = skill.source_type.toLowerCase()
  const iconNode = typeKey.includes('git') ? (
    <Github size={20} />
  ) : typeKey.includes('local') ? (
    <Folder size={20} />
  ) : (
    <Box size={20} />
  )
  const github = getGithubInfo(skill.source_ref)
  const copyValue = (github?.href ?? skill.source_ref ?? '').trim()
  const skillScope = getSkillScope(skill)
  const projectCount = getSkillProjects(skill).length
  const globalAssignmentCount = skill.assignments.filter(
    (assignment) => assignment.scope === 'global',
  ).length
  const projectAssignmentCount = new Set(
    skill.assignments
      .filter((assignment) => assignment.scope === 'project')
      .map((assignment) => assignment.project_id)
      .filter(Boolean),
  ).size
  const skillEnabled = skill.enabled !== false

  const handleCopy = async () => {
    if (!copyValue) return
    try {
      await navigator.clipboard.writeText(copyValue)
      toast.success(t('copied'))
    } catch {
      toast.error(t('copyFailed'))
    }
  }

  // Split tools into synced and remaining for badge display
  const syncedTools: {
    tool: ToolOption
    assignment: (typeof skill.assignments)[0]
  }[] = []
  const unsyncedTools: ToolOption[] = []
  for (const tool of installedTools) {
    const toolAssignments = skill.assignments.filter(
      (item) => item.tool === tool.id,
    )
    const assignment =
      toolAssignments.find((item) => item.valid) ?? toolAssignments[0]
    if (assignment) {
      syncedTools.push({ tool, assignment })
    } else {
      unsyncedTools.push(tool)
    }
  }

  const [expanded, setExpanded] = useState(false)
  const needsCollapse = syncedTools.length > MAX_VISIBLE_BADGES
  const visibleSynced = expanded ? syncedTools : syncedTools.slice(0, MAX_VISIBLE_BADGES)
  const remainingCount = syncedTools.length - MAX_VISIBLE_BADGES
  const showUnsyncedTools = expanded || !needsCollapse

  return (
    <div
      className={`skill-card${bulkMode ? ' bulk-mode' : ''}${bulkSelected ? ' bulk-selected' : ''}${highlighted ? ' newly-added' : ''}${!skillEnabled ? ' disabled-skill' : ''}`}
    >
      {bulkMode ? (
        <label className="bulk-skill-check" aria-label={t('bulk.toggleSkill')}>
          <input
            type="checkbox"
            checked={bulkSelected}
            onChange={() => onToggleBulkSelection(skill.id)}
            disabled={loading}
          />
          <span />
        </label>
      ) : null}
      <div className="skill-icon">{iconNode}</div>
      <div className="skill-main">
        <div className="skill-header-row">
          <button
            type="button"
            className="skill-name clickable"
            onClick={() => onOpenDetail(skill)}
          >
            {skill.name}
          </button>
          {skill.tags.length > 0 ? (
            <div className="skill-tags-inline">
              {skill.tags.slice(0, 3).map((tag) => (
                <button
                  key={tag.id}
                  className="skill-tag-pill"
                  type="button"
                  onClick={() => onEditTags(skill)}
                >
                  #{tag.name}
                </button>
              ))}
              {skill.tags.length > 3 ? (
                <button
                  className="skill-tag-pill muted"
                  type="button"
                  onClick={() => onEditTags(skill)}
                >
                  +{skill.tags.length - 3}
                </button>
              ) : null}
            </div>
          ) : null}
          {!skillEnabled ? (
            <span className="skill-disabled-badge">{t('disabled')}</span>
          ) : null}
          {skill.pending_assignment ? (
            <span className="skill-pending-badge">{t('assignment.pendingBadge')}</span>
          ) : null}
          {skill.assignment_issue_count > 0 ? (
            <span className="skill-attention-badge">
              <AlertTriangle size={12} />
              {t('assignment.needsAttention', {
                count: skill.assignment_issue_count,
              })}
            </span>
          ) : null}
        </div>
        {skill.description ? (
          <div className="skill-desc">{skill.description}</div>
        ) : null}
        <div className="skill-meta-row">
          {github ? (
            <div className="skill-source">
              <button
                className="repo-pill copyable"
                type="button"
                title={t('copy')}
                aria-label={t('copy')}
                onClick={() => void handleCopy()}
                disabled={!copyValue}
              >
                {github.label}
                <span className="copy-icon" aria-hidden="true">
                  <Copy size={12} />
                </span>
              </button>
            </div>
          ) : (
            <div className="skill-source">
              <button
                className="repo-pill copyable"
                type="button"
                title={t('copy')}
                aria-label={t('copy')}
                onClick={() => void handleCopy()}
                disabled={!copyValue}
              >
                <span className="mono">{getSkillSourceLabel(skill)}</span>
                <span className="copy-icon" aria-hidden="true">
                  <Copy size={12} />
                </span>
              </button>
            </div>
          )}
          <div className="skill-source time">
            <span className="dot">•</span>
            {formatRelative(skill.updated_at)}
          </div>
          <button
            className={`scope-badge ${skillScope}`}
            type="button"
            onClick={() => onOpenScope(skill)}
          >
            {t('assignment.summary', {
              global: globalAssignmentCount,
              projects: projectAssignmentCount || projectCount,
            })}
          </button>
        </div>
        <div className={`tool-matrix${!expanded && needsCollapse ? ' collapsed' : ''}`}>
          {visibleSynced.map(({ tool, assignment }) => (
            <button
              key={`${skill.id}-${tool.id}`}
              type="button"
              className={`tool-pill active${assignment.state === 'error' && !assignment.issue_ignored ? ' error' : ''}`}
              title={`${tool.label} (${assignment.ownership})`}
              onClick={() => onOpenScope(skill)}
            >
              <span className="status-badge" />
              {tool.label}
            </button>
          ))}
          {needsCollapse && !expanded ? (
            <button
              type="button"
              className="tool-pill more-badge"
              onClick={() => setExpanded(true)}
            >
              {t('moreTools', { count: remainingCount })}
            </button>
          ) : null}
          {showUnsyncedTools &&
            unsyncedTools.map((tool) => {
              const disabled = false
              return (
                <button
                  key={`${skill.id}-${tool.id}`}
                  type="button"
                  className={`tool-pill ${disabled ? 'disabled' : 'inactive'}`}
                  title={tool.label}
                  onClick={() => onOpenScope(skill)}
                  disabled={disabled}
                >
                  {tool.label}
                </button>
              )
            })}
        </div>
      </div>
      <div className="skill-actions-col">
        <button
          className="card-btn assignment-action"
          type="button"
          onClick={() => onOpenScope(skill)}
          disabled={loading}
          aria-label={t('assignment.open')}
          title={t('assignment.open')}
        >
          <Share2 size={16} />
        </button>
        <button
          className={`card-btn tag-action${skill.tags.length > 0 ? ' has-tags' : ''}`}
          type="button"
          onClick={() => onEditTags(skill)}
          disabled={loading}
          aria-label={t('editTags')}
          title={t('editTags')}
        >
          <Tag size={16} />
        </button>
        <button
          className="card-btn primary-action"
          type="button"
          onClick={() => onUpdate(skill)}
          disabled={loading || !skillEnabled}
          aria-label={t('update')}
        >
          <RefreshCw size={16} />
        </button>
        <button
          className={`card-btn power-action${skillEnabled ? ' enabled' : ''}`}
          type="button"
          onClick={() => onToggleEnabled(skill)}
          disabled={loading}
          aria-label={skillEnabled ? t('disableSkill') : t('enableSkill')}
          title={skillEnabled ? t('disableSkill') : t('enableSkill')}
        >
          <Power size={16} />
        </button>
        <button
          className="card-btn danger-action"
          type="button"
          onClick={() => onDelete(skill.id)}
          disabled={loading}
          aria-label={t('remove')}
        >
          <Trash2 size={16} />
        </button>
      </div>
    </div>
  )
}

export default memo(SkillCard)
