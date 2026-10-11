export type ProjectLibraryAddAction =
  | { kind: 'none' }
  | { kind: 'direct'; skillId: string }
  | { kind: 'batch'; skillIds: string[] }

export const getProjectLibraryAddAction = (
  selectedSkillIds: string[],
): ProjectLibraryAddAction => {
  if (selectedSkillIds.length === 0) return { kind: 'none' }
  if (selectedSkillIds.length === 1) {
    return { kind: 'direct', skillId: selectedSkillIds[0] }
  }
  return { kind: 'batch', skillIds: selectedSkillIds }
}

export const getManageableProjectSkillIds = (skills: ProjectSkillDto[]) =>
  skills.flatMap((skill) =>
    skill.source === 'library' && skill.linked_skill_id
      ? [skill.linked_skill_id]
      : [],
  )
import type { ProjectSkillDto } from '../skills/types'
