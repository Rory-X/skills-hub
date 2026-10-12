import type {
  ManagedSkill,
  SkillTemplateDto,
  TemplateDiffDto,
  TemplateMode,
} from '../skills/types'

export type TemplateDraft = {
  name: string
  description: string
  mode: TemplateMode
  skillIds: string[]
}

export const createTemplateDraft = (
  template?: SkillTemplateDto | null,
  initialSkillIds: string[] = [],
): TemplateDraft => ({
  name: template?.name ?? '',
  description: template?.description ?? '',
  mode: template?.mode ?? 'live',
  skillIds: normalizeTemplateSkillIds(
    template?.skills.map((skill) => skill.id) ?? initialSkillIds,
  ),
})

export const normalizeTemplateSkillIds = (skillIds: string[]) =>
  Array.from(new Set(skillIds.map((skillId) => skillId.trim()).filter(Boolean))).sort()

export const sortTemplateSkillsBySelection = (
  skills: ManagedSkill[],
  selectedSkillIds: string[],
) => {
  const selected = new Set(normalizeTemplateSkillIds(selectedSkillIds))
  return [...skills].sort((left, right) => {
    const selectedOrder = Number(selected.has(right.id)) - Number(selected.has(left.id))
    return selectedOrder || left.name.localeCompare(right.name)
  })
}

export const templateDraftCanSave = (
  draft: TemplateDraft,
  skills: ManagedSkill[],
  existingSkillIds: string[] = [],
) => {
  if (!draft.name.trim()) return false
  const existing = new Set(existingSkillIds)
  const library = new Map(skills.map((skill) => [skill.id, skill]))
  const selected = normalizeTemplateSkillIds(draft.skillIds)
  return (
    selected.length > 0 &&
    selected.every((skillId) => {
      const skill = library.get(skillId)
      return Boolean(skill && (skill.enabled || existing.has(skillId)))
    })
  )
}

export const templateDiffCount = (diff: TemplateDiffDto | null) => ({
  added: diff?.added.length ?? 0,
  removed: diff?.removed.length ?? 0,
  retained: diff?.retained.length ?? 0,
})
