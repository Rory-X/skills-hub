import { describe, expect, it } from 'vitest'

import {
  getAvailableProjectToolKeys,
  isGlobalToolAvailable,
  isProjectToolAvailable,
} from './toolAvailability'

const tool = (installed: boolean) => ({
  key: 'codex',
  enabled: true,
  installed,
})

describe('Agent availability', () => {
  it('does not allow an uninstalled Agent globally or in a project', () => {
    expect(isGlobalToolAvailable(tool(false))).toBe(false)
    expect(isProjectToolAvailable(tool(false), ['codex'])).toBe(false)
    expect(getAvailableProjectToolKeys([tool(false)], ['codex'])).toEqual([])
  })

  it('allows an installed and enabled Agent in an enabled project', () => {
    expect(isGlobalToolAvailable(tool(true))).toBe(true)
    expect(isProjectToolAvailable(tool(true), ['codex'])).toBe(true)
    expect(getAvailableProjectToolKeys([tool(true)], ['codex'])).toEqual([
      'codex',
    ])
  })
})
