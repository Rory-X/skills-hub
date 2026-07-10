import { memo, useMemo, useState } from 'react'
import { AlertTriangle, CheckCircle2, Download, Search, Wrench } from 'lucide-react'
import type { TFunction } from 'i18next'
import {
  getDiscoveredHubResolutionKey,
  getDiscoveredImportDecision,
  type DiscoveredHubResolution,
} from '../discoveredImportResolution'
import type { OnboardingPlan } from '../types'

type ImportModalProps = {
  open: boolean
  loading: boolean
  plan: OnboardingPlan
  selected: Record<string, boolean>
  variantChoice: Record<string, string>
  hubResolutions: Record<string, DiscoveredHubResolution | undefined>
  reservedSkillNames: string[]
  onRequestClose: () => void
  onToggleGroup: (groupName: string, checked: boolean) => void
  onSelectVariant: (groupName: string, path: string) => void
  onResolveHubConflict: (
    groupName: string,
    resolution: DiscoveredHubResolution,
  ) => void
  onImport: () => void
  t: TFunction
}

const ImportModal = ({
  open,
  loading,
  plan,
  selected,
  variantChoice,
  hubResolutions,
  reservedSkillNames,
  onRequestClose,
  onToggleGroup,
  onSelectVariant,
  onResolveHubConflict,
  onImport,
  t,
}: ImportModalProps) => {
  const [query, setQuery] = useState('')
  const [resolutionGroupName, setResolutionGroupName] = useState<string | null>(
    null,
  )
  const [draftResolution, setDraftResolution] = useState<
    DiscoveredHubResolution['kind']
  >('use_hub')
  const [draftRename, setDraftRename] = useState('')
  const normalizedQuery = query.trim().toLowerCase()
  const filteredGroups = useMemo(() => {
    if (!normalizedQuery) return plan.groups
    return plan.groups.filter((group) => {
      const fields = [
        group.name,
        ...group.variants.flatMap((variant) => [
          variant.path,
          variant.tool,
          variant.plugin_name ?? '',
          variant.plugin_version ?? '',
        ]),
      ]
      return fields.some((value) => value.toLowerCase().includes(normalizedQuery))
    })
  }, [normalizedQuery, plan.groups])
  const selectedCount = filteredGroups.filter((group) => selected[group.name]).length
  const allVisibleSelected =
    filteredGroups.length > 0 &&
    filteredGroups.every((group) => selected[group.name])
  const unresolvedHubConflictCount = plan.groups.filter((group) => {
    if (!selected[group.name]) return false
    return getDiscoveredImportDecision(
      group,
      variantChoice[group.name],
      hubResolutions[group.name],
    ).unresolvedHubConflict
  }).length

  const openResolution = (groupName: string) => {
    const existing = hubResolutions[groupName]
    setDraftResolution(existing?.kind ?? 'use_hub')
    setDraftRename(
      existing?.kind === 'rename' ? existing.name : `${groupName}-local`,
    )
    setResolutionGroupName(groupName)
  }

  const normalizedDraftRename = draftRename.trim()
  const renameIsTaken =
    draftResolution === 'rename' &&
    (reservedSkillNames.some(
      (name) => name.toLowerCase() === normalizedDraftRename.toLowerCase(),
    ) ||
      plan.groups.some(
        (group) =>
          group.name !== resolutionGroupName &&
          group.name.toLowerCase() === normalizedDraftRename.toLowerCase(),
      ))
  const renameIsInvalid =
    draftResolution === 'rename' &&
    (normalizedDraftRename.length === 0 || renameIsTaken)

  const confirmResolution = () => {
    if (!resolutionGroupName || renameIsInvalid) return
    const resolution: DiscoveredHubResolution =
      draftResolution === 'rename'
        ? { kind: 'rename', name: normalizedDraftRename }
        : { kind: draftResolution }
    onResolveHubConflict(resolutionGroupName, resolution)
    setResolutionGroupName(null)
  }

  const toggleVisibleGroups = (checked: boolean) => {
    filteredGroups.forEach((group) => onToggleGroup(group.name, checked))
  }

  if (!open) return null

  return (
    <div className="modal-backdrop" onClick={onRequestClose}>
      <div
        className="modal modal-lg modal-discovered"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <div className="modal-title">{t('importTitle')}</div>
          <button
            className="modal-close"
            type="button"
            onClick={onRequestClose}
            aria-label={t('close')}
          >
            ✕
          </button>
        </div>
        <div className="modal-body">
          <div className="import-summary">
            <div>{t('importSummary')}</div>
            <div className="import-metrics">
              <span>{t('toolsScanned', { count: plan.total_tools_scanned })}</span>
              <span>{t('skillsFound', { count: plan.total_skills_found })}</span>
            </div>
          </div>
          <div className="search-container import-search">
            <Search size={16} className="search-icon-abs" />
            <input
              className="search-input"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('searchPlaceholder')}
            />
          </div>
          <div className="sync-row">
            <label className="inline-checkbox">
              <input
                type="checkbox"
                checked={allVisibleSelected}
                onChange={(event) => toggleVisibleGroups(event.target.checked)}
                disabled={filteredGroups.length === 0}
              />
              {t('selectAll')}
            </label>
            <span className="pick-toolbar-count">
              {t('selectedCount', {
                selected: selectedCount,
                total: filteredGroups.length,
              })}
            </span>
          </div>
          <div className="groups discovered-list">
            {filteredGroups.length === 0 ? (
              <div className="empty">{t('importSearchEmpty')}</div>
            ) : null}
            {filteredGroups.map((group) => {
              const decision = getDiscoveredImportDecision(
                group,
                variantChoice[group.name],
                hubResolutions[group.name],
              )
              const hubRelation = decision.variant?.hub_relation
              const hubResolutionKey = getDiscoveredHubResolutionKey(
                hubResolutions[group.name],
              )
              const hubResolution = hubResolutions[group.name]
              const hasHubConflict =
                Boolean(group.hub_skill) && hubRelation !== 'same'
              const isResolving = resolutionGroupName === group.name

              return (
              <div className="group-card" key={group.name}>
                <div className="group-title">
                  <label className="group-select">
                    <input
                      type="checkbox"
                      checked={Boolean(selected[group.name])}
                      onChange={(event) =>
                        onToggleGroup(group.name, event.target.checked)
                      }
                    />
                    <span>{group.name}</span>
                  </label>
                  <div className="discovered-group-status">
                    {group.hub_skill ? (
                      hubRelation === 'same' ? (
                        <span className="badge success">
                          <CheckCircle2 size={12} />
                          {t('discoveredHub.same')}
                        </span>
                      ) : (
                        <span className="badge danger">
                          <AlertTriangle size={12} />
                          {t(
                            hubRelation === 'unknown'
                              ? 'discoveredHub.unknown'
                              : 'discoveredHub.conflict',
                          )}
                        </span>
                      )
                    ) : group.has_conflict ? (
                      <span className="badge danger">{t('sourcesConflict')}</span>
                    ) : (
                      <span className="badge">{t('sourcesConsistent')}</span>
                    )}
                    {group.hub_skill && group.has_conflict ? (
                      <span className="badge danger">{t('sourcesConflict')}</span>
                    ) : null}
                  </div>
                </div>
                <div className="group-variants">
                  {group.variants.map((variant) => {
                    const sourceLabel = variant.plugin_name
                      ? variant.plugin_version
                        ? t('claudePluginSourceWithVersion', {
                            plugin: variant.plugin_name,
                            version: variant.plugin_version,
                          })
                        : t('claudePluginSource', {
                            plugin: variant.plugin_name,
                          })
                      : `${t('foundIn')} ${variant.tool}`

                    return (
                      <div
                        className="variant-row"
                        key={`${group.name}-${variant.tool}-${variant.path}`}
                      >
                        {group.has_conflict ? (
                          <input
                            type="radio"
                            name={`variant-${group.name}`}
                            checked={variantChoice[group.name] === variant.path}
                            onChange={() =>
                              onSelectVariant(group.name, variant.path)
                            }
                          />
                        ) : (
                          <span className="variant-spacer" />
                        )}
                        <div className="variant-info">
                          <span className="path">{variant.path}</span>
                          <span className="found-pill">{sourceLabel}</span>
                        </div>
                        {variant.is_link ? (
                          <span className="meta">
                            {t('linkLabel', {
                              target: variant.link_target ?? t('unknown'),
                            })}
                          </span>
                        ) : (
                          <span className="meta">{t('directory')}</span>
                        )}
                      </div>
                    )
                  })}
                </div>
                {group.hub_skill ? (
                  <div className={`discovered-hub-action${hasHubConflict ? ' conflict' : ''}`}>
                    <div>
                      <strong>
                        {hasHubConflict
                          ? t('discoveredHub.conflictTitle')
                          : t('discoveredHub.reuseTitle')}
                      </strong>
                      <span>
                        {hasHubConflict
                          ? hubResolutionKey
                            ? t(`discoveredHub.resolutions.${hubResolutionKey}.selected`, {
                                name:
                                  hubResolution?.kind === 'rename'
                                    ? hubResolution.name
                                    : group.name,
                              })
                            : t('discoveredHub.conflictImpact')
                          : t('discoveredHub.reuseImpact')}
                      </span>
                    </div>
                    {hasHubConflict ? (
                      <button
                        className="btn btn-secondary discovered-resolve-btn"
                        type="button"
                        onClick={() => openResolution(group.name)}
                        disabled={loading}
                      >
                        <Wrench size={14} />
                        {hubResolutionKey
                          ? t('discoveredHub.changeResolution')
                          : t('discoveredHub.handleConflict')}
                      </button>
                    ) : null}
                  </div>
                ) : null}
                {isResolving ? (
                  <div className="discovered-resolution-editor">
                    <fieldset>
                      <legend>{t('discoveredHub.chooseResolution')}</legend>
                      {(['use_hub', 'replace_hub', 'rename'] as const).map((kind) => {
                        const key =
                          kind === 'use_hub'
                            ? 'useHub'
                            : kind === 'replace_hub'
                              ? 'replaceHub'
                              : 'rename'
                        return (
                          <label key={kind}>
                            <input
                              type="radio"
                              name={`hub-resolution-${group.name}`}
                              checked={draftResolution === kind}
                              onChange={() => setDraftResolution(kind)}
                              disabled={loading}
                            />
                            <span>
                              <strong>{t(`discoveredHub.resolutions.${key}.title`)}</strong>
                              <small>{t(`discoveredHub.resolutions.${key}.description`)}</small>
                            </span>
                          </label>
                        )
                      })}
                    </fieldset>
                    {draftResolution === 'rename' ? (
                      <div className="discovered-rename-field">
                        <input
                          value={draftRename}
                          onChange={(event) => setDraftRename(event.target.value)}
                          placeholder={t('discoveredHub.renamePlaceholder')}
                          aria-label={t('discoveredHub.renameLabel')}
                          disabled={loading}
                        />
                        {renameIsInvalid ? (
                          <span>
                            {renameIsTaken
                              ? t('discoveredHub.renameTaken')
                              : t('discoveredHub.renameRequired')}
                          </span>
                        ) : null}
                      </div>
                    ) : null}
                    <div className="discovered-resolution-actions">
                      <button
                        className="btn btn-secondary"
                        type="button"
                        onClick={() => setResolutionGroupName(null)}
                        disabled={loading}
                      >
                        {t('cancel')}
                      </button>
                      <button
                        className="btn btn-primary"
                        type="button"
                        onClick={confirmResolution}
                        disabled={loading || renameIsInvalid}
                      >
                        {t('confirm')}
                      </button>
                    </div>
                  </div>
                ) : null}
              </div>
              )
            })}
          </div>
        </div>
        <div className="modal-footer">
          <button
            className="btn btn-primary"
            onClick={onImport}
            disabled={
              loading || selectedCount === 0 || unresolvedHubConflictCount > 0
            }
          >
            <Download size={14} />
            {t('importAndSync')}
          </button>
          <button
            className="btn btn-secondary"
            onClick={onRequestClose}
            disabled={loading}
          >
            {t('close')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default memo(ImportModal)
