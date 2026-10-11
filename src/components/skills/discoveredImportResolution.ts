import type { OnboardingGroup, OnboardingVariant } from './types'

export type DiscoveredHubResolution =
  | { kind: 'use_hub' }
  | { kind: 'replace_hub' }
  | { kind: 'rename'; name: string }

export type DiscoveredImportDecision = {
  variant: OnboardingVariant | undefined
  importName: string
  conflictResolution?: 'use_hub' | 'replace_hub'
  unresolvedHubConflict: boolean
  renamed: boolean
}

export const getDiscoveredImportDecision = (
  group: OnboardingGroup,
  selectedPath: string | undefined,
  resolution: DiscoveredHubResolution | undefined,
): DiscoveredImportDecision => {
  const variant =
    group.variants.find((candidate) => candidate.path === selectedPath) ??
    group.variants[0]
  const hasHubConflict =
    Boolean(group.hub_skill) && variant?.hub_relation !== 'same'

  if (!hasHubConflict) {
    return {
      variant,
      importName: group.name,
      unresolvedHubConflict: false,
      renamed: false,
    }
  }
  if (resolution?.kind === 'rename') {
    return {
      variant,
      importName: resolution.name.trim(),
      unresolvedHubConflict: resolution.name.trim().length === 0,
      renamed: true,
    }
  }
  if (resolution?.kind === 'use_hub' || resolution?.kind === 'replace_hub') {
    return {
      variant,
      importName: group.name,
      conflictResolution: resolution.kind,
      unresolvedHubConflict: false,
      renamed: false,
    }
  }
  return {
    variant,
    importName: group.name,
    unresolvedHubConflict: true,
    renamed: false,
  }
}

export const getDiscoveredHubResolutionKey = (
  resolution: DiscoveredHubResolution | undefined,
) => {
  if (!resolution) return null
  return resolution.kind === 'rename'
    ? 'rename'
    : resolution.kind === 'replace_hub'
      ? 'replaceHub'
      : 'useHub'
}
