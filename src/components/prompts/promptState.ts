import type {
  PromptDocumentDto,
  PromptProfileDto,
  PromptProfileInputDto,
  PromptSourceDto,
} from '../skills/types'

export const choosePromptSource = (
  sources: PromptSourceDto[],
  preferredPath?: string | null,
) => {
  if (preferredPath) {
    const preferred = sources.find((source) => source.path === preferredPath)
    if (preferred) return preferred
  }
  return (
    sources.find((source) => source.exists && source.writable) ??
    sources.find((source) => source.exists) ??
    sources.find((source) => source.can_create) ??
    sources[0] ??
    null
  )
}

export const promptChangeTouchesSource = (
  paths: string[],
  source?: PromptSourceDto | null,
) => {
  if (!source) return true
  const normalize = (path: string) => {
    const normalized = path.replaceAll('\\', '/').toLowerCase()
    return normalized.startsWith('/private/') ? normalized.slice(8) : normalized
  }
  const logicalPath = normalize(source.path)
  const realPath = source.real_path ? normalize(source.real_path) : null
  return paths.some(
    (path) => {
      const normalized = normalize(path)
      return normalized === logicalPath || Boolean(realPath && normalized === realPath)
    },
  )
}

export const promptEditorIsDirty = (
  document: PromptDocumentDto | null,
  editorContent: string,
) => Boolean(document && editorContent !== document.content)

export const customPromptProfileDrafts = (
  profiles: PromptProfileDto[],
): PromptProfileInputDto[] =>
  profiles
    .filter((profile) => profile.source === 'custom')
    .map((profile) => ({
      id: profile.id,
      tool: profile.tool,
      label: profile.label,
      config_root: profile.config_root,
    }))
