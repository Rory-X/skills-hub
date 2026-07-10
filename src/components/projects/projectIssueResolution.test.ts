import { describe, expect, it } from 'vitest'
import type { ProjectIssueDto } from '../skills/types'
import { getDirectProjectIssueAction } from './projectIssueResolution'

const issueWithActions = (
  actions: ProjectIssueDto['actions'],
): ProjectIssueDto => ({
  id: 'issue-1',
  kind: 'target_missing',
  skill_id: 'skill-1',
  skill_name: 'review',
  tool_key: 'codex',
  path: '/project/.agents/skills/review',
  actions,
  fingerprint: 'fingerprint-1',
  can_ignore: true,
})

describe('getDirectProjectIssueAction', () => {
  it('returns the only safe action for one-click handling', () => {
    const action = {
      kind: 'retry_injection' as const,
      confirmation_required: false,
      destructive: false,
    }

    expect(getDirectProjectIssueAction(issueWithActions([action]))).toBe(action)
  })

  it('requires the dialog for a confirmation action', () => {
    const issue = issueWithActions([
      {
        kind: 'delete_invalid_skill',
        confirmation_required: true,
        destructive: true,
      },
    ])

    expect(getDirectProjectIssueAction(issue)).toBeNull()
  })

  it('requires the dialog when several resolutions are available', () => {
    const issue = issueWithActions([
      {
        kind: 'disable_git_exclude',
        confirmation_required: true,
        destructive: false,
      },
      {
        kind: 'initialize_git_repository',
        confirmation_required: true,
        destructive: false,
      },
    ])

    expect(getDirectProjectIssueAction(issue)).toBeNull()
  })
})
