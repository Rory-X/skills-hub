export type SkillListMode = 'all' | 'pending'

export const matchesSkillListMode = (
  pendingAssignment: boolean,
  mode: SkillListMode,
) => mode === 'all' || pendingAssignment

export const getSkillListModeAfterImport = (
  currentMode: SkillListMode,
  reimporting: boolean,
): SkillListMode => (reimporting ? currentMode : 'pending')
