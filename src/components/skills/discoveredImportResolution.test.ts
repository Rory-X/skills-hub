import { describe, expect, it } from 'vitest'

import {
  getDiscoveredHubResolutionKey,
  getDiscoveredImportDecision,
} from './discoveredImportResolution'
import type { OnboardingGroup } from './types'

const group = (relation: 'same' | 'different' | 'unknown'): OnboardingGroup => ({
  name: 'web-deploy',
  has_conflict: false,
  hub_skill: {
    id: 'hub-skill',
    name: 'web-deploy',
    central_path: '/hub/web-deploy',
  },
  variants: [
    {
      tool: 'codebuddy',
      name: 'web-deploy',
      path: '/codebuddy/web-deploy',
      is_link: false,
      hub_relation: relation,
    },
  ],
})

describe('getDiscoveredImportDecision', () => {
  it('reuses a matching Hub skill without requiring a resolution', () => {
    const decision = getDiscoveredImportDecision(group('same'), undefined, undefined)
    expect(decision.unresolvedHubConflict).toBe(false)
    expect(decision.importName).toBe('web-deploy')
    expect(decision.conflictResolution).toBeUndefined()
  })

  it('blocks a differing Hub skill until the user chooses a resolution', () => {
    const decision = getDiscoveredImportDecision(
      group('different'),
      undefined,
      undefined,
    )
    expect(decision.unresolvedHubConflict).toBe(true)
  })

  it('maps all conflict choices to executable import decisions', () => {
    expect(
      getDiscoveredImportDecision(group('different'), undefined, {
        kind: 'use_hub',
      }).conflictResolution,
    ).toBe('use_hub')
    expect(
      getDiscoveredImportDecision(group('different'), undefined, {
        kind: 'replace_hub',
      }).conflictResolution,
    ).toBe('replace_hub')

    const renamed = getDiscoveredImportDecision(group('different'), undefined, {
      kind: 'rename',
      name: ' web-deploy-local ',
    })
    expect(renamed.importName).toBe('web-deploy-local')
    expect(renamed.renamed).toBe(true)
    expect(renamed.unresolvedHubConflict).toBe(false)
    expect(getDiscoveredHubResolutionKey({ kind: 'rename', name: 'x' })).toBe(
      'rename',
    )
  })
})
