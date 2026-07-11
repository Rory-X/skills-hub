export const getVisibleAssignmentProjects = <T extends { id: string }>(
  projects: T[],
  contextProjectId?: string,
) =>
  contextProjectId
    ? projects.filter((project) => project.id === contextProjectId)
    : projects

export const getExpandedProjectsAfterToggle = (
  expandedProjectIds: string[],
  projectId: string,
) =>
  Array.from(new Set([...expandedProjectIds, projectId]))
