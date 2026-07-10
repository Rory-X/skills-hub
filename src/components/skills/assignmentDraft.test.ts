import { describe, expect, it } from 'vitest'
import { assignmentDraftForSkill } from './assignmentDraft'
import type { ManagedSkill } from './types'

const skill: ManagedSkill = {
  id: 'skill-1',
  name: 'review',
  source_type: 'local',
  central_path: '/tmp/review',
  created_at: 1,
  updated_at: 1,
  enabled: true,
  status: 'ok',
  tags: [],
  targets: [],
  pending_assignment: false,
  assignment_issue_count: 0,
  assignments: [
    {
      tool: 'codex',
      scope: 'global',
      ownership: 'hub',
      state: 'active',
      valid: true,
      can_ignore: false,
      issue_ignored: false,
    },
    {
      tool: 'cursor',
      scope: 'project',
      project_id: 'project-1',
      ownership: 'native',
      state: 'active',
      valid: true,
      can_ignore: false,
      issue_ignored: false,
    },
    {
      tool: 'claude_code',
      scope: 'project',
      project_id: 'project-2',
      ownership: 'hub',
      state: 'active',
      valid: true,
      can_ignore: false,
      issue_ignored: false,
    },
  ],
}

describe('assignmentDraftForSkill', () => {
  it('adds tools to one project without replacing other assignments', () => {
    expect(assignmentDraftForSkill(skill, 'project-1', ['codex'])).toEqual({
      global_tools: ['codex'],
      projects: [
        { project_id: 'project-1', tool_keys: ['cursor', 'codex'] },
        { project_id: 'project-2', tool_keys: ['claude_code'] },
      ],
    })
  })

  it('removes only the selected project assignment', () => {
    expect(assignmentDraftForSkill(skill, 'project-1', null)).toEqual({
      global_tools: ['codex'],
      projects: [{ project_id: 'project-2', tool_keys: ['claude_code'] }],
    })
  })
})
