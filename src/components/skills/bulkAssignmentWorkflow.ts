import { assignmentDraftForSkill } from './assignmentDraft'
import {
  isGlobalToolAvailable,
  isProjectToolAvailable,
} from './toolAvailability'
import type {
  AssignmentDraftDto,
  AssignmentApplyResultDto,
  AssignmentPreviewDto,
  ManagedSkill,
  ProjectSummaryDto,
  ToolInfoDto,
} from './types'

export type BulkAssignmentMode = 'add' | 'replace' | 'clear'
export type BulkAssignmentOrigin = 'blank' | 'template'

export const EMPTY_ASSIGNMENT_DRAFT: AssignmentDraftDto = {
  global_tools: [],
  projects: [],
}

export const getBulkAssignmentDraft = (
  mode: BulkAssignmentMode,
  draft: AssignmentDraftDto,
): AssignmentDraftDto => (mode === 'clear' ? EMPTY_ASSIGNMENT_DRAFT : draft)

export const requiresBulkClearAcknowledgement = (
  mode: BulkAssignmentMode,
  draft: AssignmentDraftDto,
) =>
  mode === 'clear' ||
  (mode === 'replace' &&
    draft.global_tools.length === 0 &&
    draft.projects.length === 0)

export const selectEntireLibrary = (
  _currentSelectedIds: string[],
  managedSkillIds: string[],
) => Array.from(new Set(managedSkillIds))

export const assignmentDraftFromTemplate = (
  skill: ManagedSkill,
  tools: ToolInfoDto[],
  projects: ProjectSummaryDto[],
): AssignmentDraftDto => {
  const draft = assignmentDraftForSkill(skill)
  const toolsByKey = new Map(tools.map((tool) => [tool.key, tool]))
  const projectsById = new Map(projects.map((project) => [project.id, project]))
  return {
    global_tools: draft.global_tools.filter((toolKey) => {
      const tool = toolsByKey.get(toolKey)
      return Boolean(tool && isGlobalToolAvailable(tool))
    }),
    projects: draft.projects.flatMap((projectDraft) => {
      const project = projectsById.get(projectDraft.project_id)
      if (!project) return []
      const toolKeys = projectDraft.tool_keys.filter((toolKey) => {
        const tool = toolsByKey.get(toolKey)
        return Boolean(
          tool && isProjectToolAvailable(tool, project.enabled_tool_keys),
        )
      })
      return toolKeys.length > 0
        ? [{ project_id: projectDraft.project_id, tool_keys: toolKeys }]
        : []
    }),
  }
}

export type BulkAssignmentPreviewSummary = {
  removeCount: number
  addCount: number
  keepCount: number
  blockedCount: number
  affectedSkillCount: number
}

export const buildBulkAssignmentPreviewSummary = (
  preview: AssignmentPreviewDto,
): BulkAssignmentPreviewSummary => {
  const affectedSkillIds = new Set<string>()
  let removeCount = 0
  let addCount = 0
  let keepCount = 0
  let blockedCount = 0
  for (const item of preview.items) {
    affectedSkillIds.add(item.skill_id)
    if (item.outcome === 'remove') removeCount += 1
    else if (item.outcome === 'ready') addCount += 1
    else if (item.outcome === 'assigned' || item.outcome === 'same_content') {
      keepCount += 1
    }
    if (item.blocking && item.outcome !== 'remove') blockedCount += 1
  }
  return {
    removeCount,
    addCount,
    keepCount,
    blockedCount,
    affectedSkillCount: affectedSkillIds.size,
  }
}

const summarizeApplySkills = (
  skills: AssignmentApplyResultDto['skills'],
): AssignmentApplyResultDto => ({
  skills,
  succeeded: skills.reduce((sum, skill) => sum + skill.succeeded, 0),
  failed: skills.reduce((sum, skill) => sum + skill.failed, 0),
  skipped: skills.reduce((sum, skill) => sum + skill.skipped, 0),
})

export const withSourceBlockedSkills = (
  result: AssignmentApplyResultDto,
  blockedSkills: ManagedSkill[],
): AssignmentApplyResultDto =>
  summarizeApplySkills([
    ...result.skills,
    ...blockedSkills.map((skill) => ({
      skill_id: skill.id,
      skill_name: skill.name,
      succeeded: 0,
      failed: 0,
      skipped: 1,
      errors: [],
    })),
  ])

export const mergeRetriedAssignmentResult = (
  previous: AssignmentApplyResultDto,
  retried: AssignmentApplyResultDto,
): AssignmentApplyResultDto => {
  const retriedById = new Map(
    retried.skills.map((skill) => [skill.skill_id, skill]),
  )
  return summarizeApplySkills(
    previous.skills.map(
      (skill) => retriedById.get(skill.skill_id) ?? skill,
    ),
  )
}
