import { describe, expect, it } from 'vitest'

import {
  getManageableProjectSkillIds,
  getProjectLibraryAddAction,
} from './projectBulkActions'
import type { ProjectSkillDto } from '../skills/types'

describe('getProjectLibraryAddAction', () => {
  it('adds one selected library Skill directly', () => {
    expect(getProjectLibraryAddAction(['skill-1'])).toEqual({
      kind: 'direct',
      skillId: 'skill-1',
    })
  })

  it('opens batch confirmation for multiple selected library Skills', () => {
    expect(getProjectLibraryAddAction(['skill-1', 'skill-2'])).toEqual({
      kind: 'batch',
      skillIds: ['skill-1', 'skill-2'],
    })
  })
})

describe('getManageableProjectSkillIds', () => {
  it('only includes Skills injected from the library', () => {
    const skill = (
      name: string,
      source: ProjectSkillDto['source'],
      linkedSkillId?: string,
      matchedSkillId?: string,
    ): ProjectSkillDto => ({
      name,
      source,
      status: 'healthy',
      linked_skill_id: linkedSkillId,
      matched_skill_id: matchedSkillId,
      tool_keys: [],
      paths: [],
    })

    expect(
      getManageableProjectSkillIds([
        skill('library', 'library', 'skill-1'),
        skill('matched', 'matched_library', undefined, 'skill-2'),
        skill('project', 'project'),
      ]),
    ).toEqual(['skill-1'])
  })
})
