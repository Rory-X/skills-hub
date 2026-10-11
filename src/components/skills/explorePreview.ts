export type ExplorePreviewTarget = {
  repoUrl: string
  subpath: string
}

export function parseExplorePreviewTarget(sourceUrl: string): ExplorePreviewTarget {
  const trimmed = sourceUrl.trim()
  try {
    const url = new URL(trimmed)
    const parts = url.pathname.split('/').filter(Boolean)
    if (parts.length >= 2 && (parts[2] === 'tree' || parts[2] === 'blob') && parts.length >= 4) {
      const rawSubpath = parts.slice(4).join('/')
      return {
        repoUrl: `${url.origin}/${parts[0]}/${parts[1]}/tree/${parts[3]}`,
        subpath: rawSubpath.replace(/\/(?:SKILL|skill)\.md$/i, '') || '.',
      }
    }
    return { repoUrl: trimmed, subpath: '.' }
  } catch {
    return { repoUrl: trimmed, subpath: '.' }
  }
}
