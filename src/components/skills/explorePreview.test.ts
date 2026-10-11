import { describe, expect, it } from 'vitest'
import { parseExplorePreviewTarget } from './explorePreview'

describe('parseExplorePreviewTarget', () => {
  it('uses a GitHub tree subpath when present', () => {
    expect(
      parseExplorePreviewTarget('https://github.com/acme/tools/tree/main/skills/demo'),
    ).toEqual({
      repoUrl: 'https://github.com/acme/tools/tree/main',
      subpath: 'skills/demo',
    })
  })

  it('falls back to the repository root', () => {
    expect(parseExplorePreviewTarget('https://github.com/acme/tools.git')).toEqual({
      repoUrl: 'https://github.com/acme/tools.git',
      subpath: '.',
    })
  })

  it('normalizes a direct SKILL.md blob URL to its directory', () => {
    expect(
      parseExplorePreviewTarget('https://github.com/acme/tools/blob/main/skills/demo/SKILL.md'),
    ).toEqual({
      repoUrl: 'https://github.com/acme/tools/tree/main',
      subpath: 'skills/demo',
    })
  })
})
