export const getVisibleAssignmentProjects = <T extends { id: string }>(
  projects: T[],
  contextProjectId?: string,
) =>
  contextProjectId
    ? projects.filter((project) => project.id === contextProjectId)
    : projects

export const getVisibleAssignmentContextItems = <
  T extends { scope: string; project_id?: string | null },
>(assignments: T[], contextProjectId?: string) =>
  contextProjectId
    ? assignments.filter(
        (assignment) =>
          assignment.scope === 'project' &&
          assignment.project_id === contextProjectId,
      )
    : assignments

export const countVisibleAssignedProjects = (
  projectTools: Record<string, string[]>,
  visibleProjects: { id: string }[],
) =>
  visibleProjects.filter((project) => (projectTools[project.id] ?? []).length > 0)
    .length

export const getExpandedProjectsAfterToggle = (
  expandedProjectIds: string[],
  projectId: string,
) =>
  Array.from(new Set([...expandedProjectIds, projectId]))

export const getProjectToolsAfterToggle = (
  projectTools: Record<string, string[]>,
  projectId: string,
  checked: boolean,
) => {
  const next = { ...projectTools }
  if (checked) next[projectId] = []
  else delete next[projectId]
  return next
}
