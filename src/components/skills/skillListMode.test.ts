import { describe, expect, it } from 'vitest'

import {
  getSkillListModeAfterImport,
  matchesSkillListMode,
} from './skillListMode'

describe('Skill library list mode', () => {
  it('shows every Skill in all mode', () => {
    expect(matchesSkillListMode(false, 'all')).toBe(true)
    expect(matchesSkillListMode(true, 'all')).toBe(true)
  })

  it('shows only unassigned Skills in pending mode', () => {
    expect(matchesSkillListMode(true, 'pending')).toBe(true)
    expect(matchesSkillListMode(false, 'pending')).toBe(false)
  })

  it('opens unassigned Skills after a fresh import but preserves reimport context', () => {
    expect(getSkillListModeAfterImport('all', false)).toBe('pending')
    expect(getSkillListModeAfterImport('all', true)).toBe('all')
    expect(getSkillListModeAfterImport('pending', true)).toBe('pending')
  })
})
