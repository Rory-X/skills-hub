import { describe, expect, it } from 'vitest'
import type { PromptDocumentDto, PromptSourceDto } from '../skills/types'
import {
  choosePromptSource,
  promptChangeTouchesSource,
  promptEditorIsDirty,
} from './promptState'

const source = (overrides: Partial<PromptSourceDto>): PromptSourceDto => ({
  id: overrides.path ?? 'source',
  tool: 'codex',
  profile_id: 'codex:test',
  project_id: null,
  scope: 'global',
  kind: 'primary',
  path: '/tmp/AGENTS.md',
  display_path: 'AGENTS.md',
  real_path: null,
  exists: false,
  is_symlink: false,
  link_target: null,
  writable: true,
  git_status: null,
  content_hash: null,
  modified_at: null,
  byte_len: null,
  empty: true,
  encoding: 'utf-8',
  can_create: true,
  can_delete: true,
  warning: null,
  ...overrides,
})

describe('prompt editor state', () => {
  it('preserves the selected path and otherwise prefers an existing writable source', () => {
    const missing = source({ path: '/tmp/missing.md' })
    const existing = source({ path: '/tmp/existing.md', exists: true })
    expect(choosePromptSource([missing, existing])?.path).toBe(existing.path)
    expect(choosePromptSource([missing, existing], missing.path)?.path).toBe(
      missing.path,
    )
  })

  it('matches watcher events against logical and real symlink paths', () => {
    const linked = source({ real_path: '/tmp/dotfiles/AGENTS.md' })
    expect(promptChangeTouchesSource(['/tmp/AGENTS.md'], linked)).toBe(true)
    expect(
      promptChangeTouchesSource(['/tmp/dotfiles/AGENTS.md'], linked),
    ).toBe(true)
    expect(promptChangeTouchesSource(['/tmp/unrelated.md'], linked)).toBe(false)
    expect(
      promptChangeTouchesSource(['/private/tmp/AGENTS.md'], linked),
    ).toBe(true)
  })

  it('detects explicit editor changes', () => {
    const document: PromptDocumentDto = {
      source: source({ exists: true }),
      content: 'before',
      content_hash: 'hash',
      line_ending: 'lf',
      has_bom: false,
      read_only: false,
      warnings: [],
    }
    expect(promptEditorIsDirty(document, 'before')).toBe(false)
    expect(promptEditorIsDirty(document, 'after')).toBe(true)
  })
})
