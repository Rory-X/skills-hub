import { describe, expect, it } from 'vitest'

import {
  countVisibleAssignedProjects,
  getExpandedProjectsAfterToggle,
  getProjectToolsAfterToggle,
  getVisibleAssignmentContextItems,
  getVisibleAssignmentProjects,
} from './assignmentModalState'

describe('assignment modal project context', () => {
  it('does not select every Agent when a project is selected', () => {
    expect(getProjectToolsAfterToggle({}, 'project-1', true)).toEqual({
      'project-1': [],
    })
  })

  it('keeps the current project expanded after clearing all agents', () => {
    expect(getExpandedProjectsAfterToggle(['project-1'], 'project-1')).toEqual([
      'project-1',
    ])
  })

  it('shows only the current project in project detail context', () => {
    const projects = [{ id: 'project-1' }, { id: 'project-2' }]
    expect(getVisibleAssignmentProjects(projects, 'project-1')).toEqual([
      { id: 'project-1' },
    ])
  })

  it('shows every project outside project detail context', () => {
    const projects = [{ id: 'project-1' }, { id: 'project-2' }]
    expect(getVisibleAssignmentProjects(projects)).toEqual(projects)
  })

  it('shows only current-project assignment issues in project detail context', () => {
    const assignments = [
      { scope: 'global', project_id: null, tool: 'codex' },
      { scope: 'project', project_id: 'project-1', tool: 'cursor' },
      { scope: 'project', project_id: 'project-2', tool: 'claude_code' },
    ]
    expect(getVisibleAssignmentContextItems(assignments, 'project-1')).toEqual([
      { scope: 'project', project_id: 'project-1', tool: 'cursor' },
    ])
    expect(getVisibleAssignmentContextItems(assignments)).toEqual(assignments)
  })

  it('counts assigned projects only inside the visible context', () => {
    expect(
      countVisibleAssignedProjects(
        { 'project-1': ['codex'], 'project-2': ['cursor'] },
        [{ id: 'project-1' }],
      ),
    ).toBe(1)
  })
})
