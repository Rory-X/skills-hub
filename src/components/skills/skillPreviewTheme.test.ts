import { describe, expect, it } from 'vitest'
import appCss from '../../App.css?raw'
import themeCss from '../../index.css?raw'

function collectCustomProperties(css: string, pattern: RegExp): Set<string> {
  return new Set(Array.from(css.matchAll(pattern), (match) => match[1]))
}

describe('Skill preview theme styles', () => {
  it('only references custom properties declared by the app theme', () => {
    const previewStyles = appCss.slice(
      appCss.indexOf('/* Unified read-only Skill preview drawer */'),
    )
    const declared = collectCustomProperties(
      themeCss,
      /(--[a-z0-9-]+)\s*:/gi,
    )
    const referenced = collectCustomProperties(
      previewStyles,
      /var\((--[a-z0-9-]+)/gi,
    )
    const missing = Array.from(referenced).filter(
      (property) => !declared.has(property),
    )

    expect(missing).toEqual([])
  })
})
