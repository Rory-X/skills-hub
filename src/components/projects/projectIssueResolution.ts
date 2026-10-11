import type {
  ProjectIssueActionDto,
  ProjectIssueDto,
} from '../skills/types'

export const getDirectProjectIssueAction = (
  issue: ProjectIssueDto,
): ProjectIssueActionDto | null => {
  if (issue.actions.length !== 1) return null
  const [action] = issue.actions
  return action.confirmation_required ? null : action
}
