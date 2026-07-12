import { describe, expect, it } from 'vitest'
import {
  assignmentDraftFromTemplate,
  buildBulkAssignmentPreviewSummary,
  getBulkAssignmentDraft,
  getVisibleBulkAssignmentPreviewStats,
  mergeRetriedAssignmentResult,
  withSourceBlockedSkills,
  selectEntireLibrary,
} from './bulkAssignmentWorkflow'
import type {
  AssignmentDraftDto,
  AssignmentPreviewDto,
  ManagedSkill,
  ProjectSummaryDto,
  ToolInfoDto,
} from './types'

const draft: AssignmentDraftDto = {
  global_tools: ['codex'],
  projects: [{ project_id: 'project-1', tool_keys: ['codex'] }],
}

const tools = [
  {
    key: 'codex',
    enabled: true,
    installed: true,
    supports_project_scope: true,
  },
  {
    key: 'legacy',
    enabled: true,
    installed: false,
    supports_project_scope: true,
  },
] as ToolInfoDto[]

const projects = [
  { id: 'project-1', enabled_tool_keys: ['codex', 'legacy'] },
] as ProjectSummaryDto[]

const template = {
  assignments: [
    { tool: 'codex', scope: 'global' },
    { tool: 'legacy', scope: 'global' },
    { tool: 'codex', scope: 'project', project_id: 'project-1' },
    { tool: 'legacy', scope: 'project', project_id: 'project-1' },
  ],
} as ManagedSkill

describe('bulk assignment workflow', () => {
  it('uses an empty target only for clear mode and preserves the configured target otherwise', () => {
    expect(getBulkAssignmentDraft('add', draft)).toEqual(draft)
    expect(getBulkAssignmentDraft('replace', draft)).toEqual(draft)
    expect(getBulkAssignmentDraft('clear', draft)).toEqual({
      global_tools: [],
      projects: [],
    })
  })

  it('keeps affected Skills visible and hides zero-value preview stats', () => {
    expect(
      getVisibleBulkAssignmentPreviewStats({
        affectedSkillCount: 56,
        removeCount: 468,
        addCount: 0,
        keepCount: 0,
        blockedCount: 0,
      }),
    ).toEqual([
      { key: 'affected', count: 56 },
      { key: 'remove', count: 468 },
    ])
    expect(
      getVisibleBulkAssignmentPreviewStats({
        affectedSkillCount: 0,
        removeCount: 0,
        addCount: 0,
        keepCount: 0,
        blockedCount: 0,
      }),
    ).toEqual([])
  })

  it('selects the entire managed library independently of the visible filter result', () => {
    expect(selectEntireLibrary(['visible'], ['visible', 'hidden', 'disabled'])).toEqual([
      'visible',
      'hidden',
      'disabled',
    ])
  })

  it('copies only currently available assignments from a template', () => {
    expect(assignmentDraftFromTemplate(template, tools, projects)).toEqual(draft)
  })

  it('summarizes destructive changes and affected skills for confirmation copy', () => {
    const preview = {
      items: [
        { skill_id: 'a', outcome: 'remove', blocking: true },
        { skill_id: 'a', outcome: 'ready' },
        { skill_id: 'b', outcome: 'remove' },
        { skill_id: 'b', outcome: 'assigned' },
        { skill_id: 'c', outcome: 'conflict', blocking: true },
      ],
    } as AssignmentPreviewDto

    expect(buildBulkAssignmentPreviewSummary(preview)).toEqual({
      removeCount: 2,
      addCount: 1,
      keepCount: 1,
      blockedCount: 1,
      affectedSkillCount: 3,
    })
  })

  it('keeps source-blocked and prior successful Skills visible after retrying failures', () => {
    const initial = withSourceBlockedSkills(
      {
        succeeded: 1,
        failed: 1,
        skipped: 0,
        skills: [
          { skill_id: 'ok', skill_name: 'OK', succeeded: 1, failed: 0, skipped: 0, errors: [] },
          { skill_id: 'failed', skill_name: 'Failed', succeeded: 0, failed: 1, skipped: 0, errors: ['no'] },
        ],
      },
      [{ id: 'blocked', name: 'Blocked' } as ManagedSkill],
    )
    const merged = mergeRetriedAssignmentResult(initial, {
      succeeded: 1,
      failed: 0,
      skipped: 0,
      skills: [
        { skill_id: 'failed', skill_name: 'Failed', succeeded: 1, failed: 0, skipped: 0, errors: [] },
      ],
    })

    expect(merged.skills.map((skill) => skill.skill_id)).toEqual([
      'ok',
      'failed',
      'blocked',
    ])
    expect(merged).toMatchObject({ succeeded: 2, failed: 0, skipped: 1 })
  })
})
