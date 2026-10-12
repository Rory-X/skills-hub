import { memo, useState } from 'react'
import { ArrowLeft, ChevronDown, FileText, Plus, Trash2 } from 'lucide-react'
import type { TFunction } from 'i18next'
import type {
  CustomToolConfigDto,
  ToolConfigDto,
  ToolStatusDto,
} from './types'

type ToolsPageProps = {
  toolStatus: ToolStatusDto | null
  toolConfig: ToolConfigDto | null
  onToolConfigChange: (config: ToolConfigDto) => void
  onOpenPrompts?: (toolKey: string) => void
  embedded?: boolean
  onBack?: () => void
  t: TFunction
}

const ToolsPage = ({
  toolStatus,
  toolConfig,
  onToolConfigChange,
  onOpenPrompts,
  embedded = false,
  onBack,
  t,
}: ToolsPageProps) => {
  const [customToolName, setCustomToolName] = useState('')
  const [customToolSkillsDir, setCustomToolSkillsDir] = useState('')
  const [customToolProjectDir, setCustomToolProjectDir] = useState('')
  const [showMissingTools, setShowMissingTools] = useState(false)
  const effectiveToolConfig = toolConfig ?? {
    disabled_builtin_tools: [],
    custom_tools: [],
  }
  const disabledBuiltinTools = new Set(effectiveToolConfig.disabled_builtin_tools)
  const customToolsByKey = new Map(
    effectiveToolConfig.custom_tools.map((tool) => [tool.key, tool]),
  )

  const makeCustomToolKey = (label: string) => {
    const slug = label
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]+/g, '_')
      .replace(/^_+|_+$/g, '')
    const base = slug && /^[a-z]/.test(slug) ? slug : 'custom_tool'
    const prefix = `custom_${base}`
    const existingKeys = new Set(effectiveToolConfig.custom_tools.map((tool) => tool.key))
    if (!existingKeys.has(prefix)) return prefix
    let suffix = 2
    while (existingKeys.has(`${prefix}_${suffix}`)) suffix += 1
    return `${prefix}_${suffix}`
  }

  const updateToolConfig = (next: ToolConfigDto) => {
    onToolConfigChange({
      disabled_builtin_tools: [...next.disabled_builtin_tools],
      custom_tools: next.custom_tools.map((tool) => ({ ...tool })),
    })
  }

  const setBuiltinToolEnabled = (key: string, enabled: boolean) => {
    const disabled = new Set(effectiveToolConfig.disabled_builtin_tools)
    if (enabled) {
      disabled.delete(key)
    } else {
      disabled.add(key)
    }
    updateToolConfig({
      ...effectiveToolConfig,
      disabled_builtin_tools: [...disabled],
    })
  }

  const setCustomToolEnabled = (key: string, enabled: boolean) => {
    updateToolConfig({
      ...effectiveToolConfig,
      custom_tools: effectiveToolConfig.custom_tools.map((tool) =>
        tool.key === key ? { ...tool, enabled } : tool,
      ),
    })
  }

  const removeCustomTool = (key: string) => {
    updateToolConfig({
      ...effectiveToolConfig,
      custom_tools: effectiveToolConfig.custom_tools.filter(
        (tool) => tool.key !== key,
      ),
    })
  }

  const addCustomTool = () => {
    const label = customToolName.trim()
    const skillsDir = customToolSkillsDir.trim()
    if (!label || !skillsDir) return
    const nextTool: CustomToolConfigDto = {
      key: makeCustomToolKey(label),
      label,
      skills_dir: skillsDir,
      project_skills_dir: customToolProjectDir.trim() || null,
      enabled: true,
    }
    updateToolConfig({
      ...effectiveToolConfig,
      custom_tools: [...effectiveToolConfig.custom_tools, nextTool],
    })
    setCustomToolName('')
    setCustomToolSkillsDir('')
    setCustomToolProjectDir('')
  }

  const tools = toolStatus?.tools ?? []
  const isToolEnabled = (tool: (typeof tools)[number]) => {
    if (tool.is_custom) {
      return customToolsByKey.get(tool.key)?.enabled ?? tool.enabled
    }
    return !disabledBuiltinTools.has(tool.key)
  }
  const primaryTools = tools.filter((tool) => tool.installed || tool.is_custom)
  const missingTools = tools.filter((tool) => !tool.installed && !tool.is_custom)
  const totalCount = tools.length
  const enabledCount = tools.filter(isToolEnabled).length
  const detectedCount = tools.filter((tool) => tool.installed).length
  const customCount = tools.filter((tool) => tool.is_custom).length

  const renderToolCard = (tool: (typeof tools)[number]) => {
    const enabled = isToolEnabled(tool)
    return (
      <div
        className={`tool-card${!tool.installed ? ' missing' : ''}`}
        key={tool.key}
      >
        <div className="tool-card-head">
          <div className="tool-card-title">
            <span className="tool-card-avatar" aria-hidden="true">
              {tool.label.slice(0, 2).toUpperCase()}
            </span>
            <span className="tool-management-name">
              {t(`tools.${tool.key}`, { defaultValue: tool.label })}
            </span>
          </div>
          <div className="tool-management-actions">
            {onOpenPrompts &&
            (tool.key === 'codex' ||
              tool.key === 'claude_code' ||
              tool.key === 'pi') ? (
              <button
                type="button"
                className="icon-btn"
                title={t('prompts.openForTool')}
                aria-label={t('prompts.openForTool')}
                onClick={() => onOpenPrompts(tool.key)}
              >
                <FileText size={16} />
              </button>
            ) : null}
            <button
              type="button"
              className={`settings-toggle${enabled ? ' checked' : ''}`}
              aria-pressed={enabled}
              onClick={() => {
                if (tool.is_custom) {
                  setCustomToolEnabled(tool.key, !enabled)
                } else {
                  setBuiltinToolEnabled(tool.key, !enabled)
                }
              }}
            >
              <span className="settings-toggle-knob" />
            </button>
            {tool.is_custom ? (
              <button
                type="button"
                className="icon-btn danger"
                title={t('toolManagement.removeCustom')}
                aria-label={t('toolManagement.removeCustom')}
                onClick={() => removeCustomTool(tool.key)}
              >
                <Trash2 size={16} />
              </button>
            ) : null}
          </div>
        </div>
        <div className="tool-card-badges">
          <span
            className={`tool-management-status ${
              tool.installed ? 'installed' : 'missing'
            }`}
          >
            {tool.installed
              ? t('toolManagement.detected')
              : t('toolManagement.notDetected')}
          </span>
          {tool.is_custom ? (
            <span className="tool-management-custom">
              {t('toolManagement.custom')}
            </span>
          ) : null}
        </div>
        <div className="tool-card-path mono" title={tool.skills_dir}>
          {tool.skills_dir}
        </div>
        {tool.project_skills_dir ? (
          <div className="tool-card-path" title={tool.project_skills_dir}>
            {t('toolManagement.projectDir', {
              path: tool.project_skills_dir,
            })}
          </div>
        ) : null}
      </div>
    )
  }

  return (
    <div className={`tools-page${embedded ? ' embedded' : ''}`}>
      {!embedded ? (
        <div className="detail-header">
          <button className="icon-btn" type="button" onClick={onBack} aria-label={t('back')}>
            <ArrowLeft size={18} />
          </button>
          <div>
            <h2>{t('toolManagement.title')}</h2>
            <p>{t('toolManagement.pageHint')}</p>
          </div>
        </div>
      ) : null}

      <div className="tools-page-body">
        <div className="tools-summary-grid">
          <div className="tools-summary-card">
            <span>{t('toolManagement.totalCount')}</span>
            <strong>{totalCount}</strong>
          </div>
          <div className="tools-summary-card">
            <span>{t('toolManagement.enabledCount')}</span>
            <strong>{enabledCount}</strong>
          </div>
          <div className="tools-summary-card">
            <span>{t('toolManagement.detectedCount')}</span>
            <strong>{detectedCount}</strong>
          </div>
          <div className="tools-summary-card">
            <span>{t('toolManagement.customCount')}</span>
            <strong>{customCount}</strong>
          </div>
        </div>

        <section className="tools-panel">
          <div className="tools-panel-head">
            <div>
              <h3>{t('toolManagement.builtinSection')}</h3>
              <p>{t('toolManagement.builtinHint')}</p>
            </div>
          </div>
          <div className="tools-list">
            {tools.length > 0 ? (
              <div className="tools-card-grid">
                {primaryTools.length > 0 ? (
                  primaryTools.map(renderToolCard)
                ) : (
                  <div className="settings-helper">
                    {t('toolManagement.noDetectedTools')}
                  </div>
                )}
              </div>
            ) : (
              <div className="settings-helper">{t('detectingTools')}</div>
            )}
          </div>
          {missingTools.length > 0 ? (
            <div className="tools-missing-section">
              <button
                className="tools-missing-toggle"
                type="button"
                onClick={() => setShowMissingTools((open) => !open)}
                aria-expanded={showMissingTools}
              >
                <span>
                  {t('toolManagement.missingSection', {
                    count: missingTools.length,
                  })}
                </span>
                <ChevronDown
                  size={16}
                  className={showMissingTools ? 'expanded' : ''}
                />
              </button>
              {showMissingTools ? (
                <div className="tools-card-grid missing-tools-grid">
                  {missingTools.map(renderToolCard)}
                </div>
              ) : null}
            </div>
          ) : null}
        </section>

        <section className="tools-panel">
          <div className="tools-panel-head">
            <div>
              <h3>{t('toolManagement.customSection')}</h3>
              <p>{t('toolManagement.customHint')}</p>
            </div>
          </div>
          <div className="tool-management-add tools-add-grid">
            <input
              className="settings-input"
              value={customToolName}
              placeholder={t('toolManagement.namePlaceholder')}
              onChange={(event) => setCustomToolName(event.target.value)}
            />
            <input
              className="settings-input mono"
              value={customToolSkillsDir}
              placeholder={t('toolManagement.skillsDirPlaceholder')}
              onChange={(event) => setCustomToolSkillsDir(event.target.value)}
            />
            <input
              className="settings-input mono"
              value={customToolProjectDir}
              placeholder={t('toolManagement.projectDirPlaceholder')}
              onChange={(event) => setCustomToolProjectDir(event.target.value)}
            />
            <button
              className="btn btn-primary tool-management-add-btn"
              type="button"
              onClick={addCustomTool}
              disabled={!customToolName.trim() || !customToolSkillsDir.trim()}
            >
              <Plus size={16} />
              {t('toolManagement.addCustom')}
            </button>
          </div>
        </section>
      </div>
    </div>
  )
}

export default memo(ToolsPage)
