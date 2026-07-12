import { describe, expect, it } from 'vitest'

import {
  getExpandedProjectsAfterToggle,
  getProjectToolsAfterToggle,
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
})
