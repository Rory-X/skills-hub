import { describe, expect, it } from 'vitest'
import type { ManagedSkill } from '../skills/types'
import {
  createTemplateDraft,
  normalizeTemplateSkillIds,
  sortTemplateSkillsBySelection,
  templateDraftCanSave,
  templateDiffCount,
} from './templateState'

const skill = (id: string, enabled = true): ManagedSkill => ({
  id,
  name: id,
  source_type: 'local',
  central_path: `/skills/${id}`,
  created_at: 1,
  updated_at: 1,
  enabled,
  status: 'ok',
  tags: [],
  targets: [],
  assignments: [],
  pending_assignment: false,
  assignment_issue_count: 0,
})

describe('template state', () => {
  it('normalizes an unordered Skill reference set', () => {
    expect(normalizeTemplateSkillIds(['b', 'a', 'b', ''])).toEqual(['a', 'b'])
  })

  it('orders selected Skills before the rest of the library', () => {
    const skills = [skill('charlie'), skill('alpha'), skill('bravo'), skill('delta')]

    expect(
      sortTemplateSkillsBySelection(skills, ['delta', 'bravo']).map(({ id }) => id),
    ).toEqual(['bravo', 'delta', 'alpha', 'charlie'])
  })

  it('requires a name and at least one eligible Skill', () => {
    const skills = [skill('enabled'), skill('disabled', false)]
    const draft = (skillId: string) => ({
      ...createTemplateDraft(null, [skillId]),
      name: 'Workflow',
    })
    expect(
      templateDraftCanSave(draft('enabled'), skills),
    ).toBe(true)
    expect(
      templateDraftCanSave(draft('disabled'), skills),
    ).toBe(false)
  })

  it('allows an existing disabled reference to remain but not to be newly added', () => {
    const skills = [skill('disabled', false)]
    const draft = {
      ...createTemplateDraft(null, ['disabled']),
      name: 'Workflow',
    }
    expect(templateDraftCanSave(draft, skills, ['disabled'])).toBe(true)
    expect(templateDraftCanSave(draft, skills)).toBe(false)
  })

  it('counts a project switch diff', () => {
    expect(
      templateDiffCount({
        added: [{ id: 'a', name: 'A', enabled: true }],
        removed: [],
        retained: [
          { id: 'b', name: 'B', enabled: true },
          { id: 'c', name: 'C', enabled: true },
        ],
      }),
    ).toEqual({ added: 1, removed: 0, retained: 2 })
  })
})
