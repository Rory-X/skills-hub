import { describe, expect, it } from 'vitest'

import { getGitImportCustomName } from './gitImportName'

describe('getGitImportCustomName', () => {
  it('uses the custom name when exactly one skill is selected', () => {
    expect(getGitImportCustomName(1, ' custom-name ')).toBe('custom-name')
  })

  it('keeps each original skill name when multiple skills are selected', () => {
    expect(getGitImportCustomName(2, 'custom-name')).toBeUndefined()
  })
})
