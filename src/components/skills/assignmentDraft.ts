import type { AssignmentDraftDto, ManagedSkill } from './types'

export const assignmentDraftForSkill = (
  skill: ManagedSkill,
  projectId?: string,
  projectToolKeys?: string[] | null,
): AssignmentDraftDto => {
  const globalTools = new Set<string>()
  const projectTools = new Map<string, Set<string>>()
  for (const assignment of skill.assignments) {
    if (assignment.scope === 'global') {
      globalTools.add(assignment.tool)
    } else if (assignment.project_id) {
      const tools = projectTools.get(assignment.project_id) ?? new Set<string>()
      tools.add(assignment.tool)
      projectTools.set(assignment.project_id, tools)
    }
  }
  if (projectId) {
    if (projectToolKeys === null) {
      projectTools.delete(projectId)
    } else if (projectToolKeys) {
      const tools = projectTools.get(projectId) ?? new Set<string>()
      for (const tool of projectToolKeys) tools.add(tool)
      projectTools.set(projectId, tools)
    }
  }
  return {
    global_tools: Array.from(globalTools),
    projects: Array.from(projectTools, ([project_id, toolKeys]) => ({
      project_id,
      tool_keys: Array.from(toolKeys),
    })),
  }
}
