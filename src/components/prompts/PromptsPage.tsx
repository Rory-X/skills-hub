import { memo } from 'react'
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  FilePlus2,
  FileText,
  FolderGit2,
  History,
  Link2,
  LockKeyhole,
  RefreshCw,
  RotateCcw,
  Save,
  Settings2,
  ShieldCheck,
  Trash2,
  XCircle,
  Zap,
} from 'lucide-react'
import type { TFunction } from 'i18next'
import SelectMenu from '../common/SelectMenu'
import PromptChainPopover from './PromptChainPopover'
import type {
  ProjectSummaryDto,
  PromptDocumentDto,
  PromptInstructionChainDto,
  PromptProfileDto,
  PromptSourceDto,
  PromptWatchStatusDto,
} from '../skills/types'

type PromptsPageProps = {
  profiles: PromptProfileDto[]
  projects: ProjectSummaryDto[]
  returnProjectName?: string
  selectedProfileId: string
  selectedProjectId: string
  sources: PromptSourceDto[]
  selectedSourcePath: string
  document: PromptDocumentDto | null
  editorContent: string
  dirty: boolean
  loading: boolean
  saving: boolean
  externalChange: boolean
  conflict: boolean
  watchStatus: PromptWatchStatusDto | null
  chain: PromptInstructionChainDto | null
  chainLoading: boolean
  lastDeletedRevisionId: string | null
  onBackToProject: () => void
  onProfileChange: (profileId: string) => void
  onProjectChange: (projectId: string) => void
  onOpenProfiles: () => void
  onSelectSource: (source: PromptSourceDto) => void
  onContentChange: (content: string) => void
  onSave: () => void
  onForceSave: () => void
  onReload: () => void
  onDelete: () => void
  onOpenHistory: () => void
  onOpenPromptChain: () => void
  onRestoreLastDelete: () => void
  formatTimestamp: (value?: number | null) => string
  t: TFunction
}

const sourceStatusIcon = (source: PromptSourceDto) => {
  if (!source.exists) return <FilePlus2 size={15} />
  if (!source.writable) return <LockKeyhole size={15} />
  return <FileText size={15} />
}

const PromptsPage = ({
  profiles,
  projects,
  returnProjectName,
  selectedProfileId,
  selectedProjectId,
  sources,
  selectedSourcePath,
  document,
  editorContent,
  dirty,
  loading,
  saving,
  externalChange,
  conflict,
  watchStatus,
  chain,
  chainLoading,
  lastDeletedRevisionId,
  onBackToProject,
  onProfileChange,
  onProjectChange,
  onOpenProfiles,
  onSelectSource,
  onContentChange,
  onSave,
  onForceSave,
  onReload,
  onDelete,
  onOpenHistory,
  onOpenPromptChain,
  onRestoreLastDelete,
  formatTimestamp,
  t,
}: PromptsPageProps) => {
  const selectedProfile = profiles.find(
    (profile) => profile.id === selectedProfileId,
  )
  const selectedProject = projects.find(
    (project) => project.id === selectedProjectId,
  )
  const editorDisabled = !document || document.read_only || saving
  const sourceCount = sources.filter((source) => source.exists).length

  return (
    <div className="prompts-page">
      <div className="prompts-hero">
        <div className="prompts-heading">
          {returnProjectName ? (
            <button
              className="icon-btn"
              type="button"
              onClick={onBackToProject}
              title={t('prompts.backToProject', { project: returnProjectName })}
              aria-label={t('prompts.backToProject', { project: returnProjectName })}
            >
              <ArrowLeft size={18} />
            </button>
          ) : null}
          <div>
            <div className="prompts-title-row">
              <h1>{t('prompts.title')}</h1>
              <span className="prompts-local-badge">
                <ShieldCheck size={14} />
                {t('prompts.localOnly')}
              </span>
            </div>
            <p>{t('prompts.help')}</p>
          </div>
        </div>
        <div className="prompts-context-controls">
          <label>
            <span>{t('prompts.profile')}</span>
            <SelectMenu
              className="prompt-context-select"
              value={selectedProfileId}
              options={profiles.map((profile) => ({
                value: profile.id,
                label: profile.label,
              }))}
              onChange={onProfileChange}
              ariaLabel={t('prompts.profile')}
              disabled={loading || profiles.length === 0}
              menuMinWidth={210}
            />
          </label>
          <label>
            <span>{t('prompts.scope')}</span>
            <SelectMenu
              className="prompt-context-select"
              value={selectedProjectId}
              options={[
                { value: '', label: t('prompts.globalOnly') },
                ...projects.map((project) => ({
                  value: project.id,
                  label: project.name,
                })),
              ]}
              onChange={onProjectChange}
              ariaLabel={t('prompts.scope')}
              disabled={loading}
              menuMinWidth={210}
            />
          </label>
          <button className="btn btn-secondary" type="button" onClick={onOpenProfiles}>
            <Settings2 size={16} />
            {t('prompts.manageProfiles')}
          </button>
        </div>
      </div>

      <div className="prompts-workbench">
        <aside className="prompt-sources-panel">
          <div className="prompt-panel-heading">
            <div>
              <strong>{t('prompts.sources')}</strong>
              <span>{t('prompts.sourceCount', { count: sourceCount })}</span>
            </div>
            {watchStatus ? (
              <span
                className={`prompt-watch-indicator${watchStatus.truncated ? ' warning' : ''}`}
                title={t(
                  watchStatus.truncated
                    ? 'prompts.watchLimited'
                    : 'prompts.watching',
                )}
              >
                <Zap size={13} />
                {t('prompts.live')}
              </span>
            ) : null}
          </div>
          {selectedProfile ? (
            <PromptChainPopover
              key={`${selectedProfileId}:${selectedProjectId}`}
              chain={chain}
              loading={chainLoading}
              disabled={loading}
              contextLabel={`${selectedProfile.label} · ${selectedProject?.name ?? t('prompts.globalOnly')}`}
              onOpen={onOpenPromptChain}
              onSelectSource={onSelectSource}
              t={t}
            />
          ) : null}
          <div className="prompt-source-list" aria-busy={loading}>
            {sources.length === 0 ? (
              <div className="prompt-empty-state">
                {loading ? t('prompts.loading') : t('prompts.noSources')}
              </div>
            ) : (
              sources.map((source) => (
                <button
                  className={`prompt-source-item${source.path === selectedSourcePath ? ' active' : ''}`}
                  type="button"
                  key={source.id}
                  onClick={() => onSelectSource(source)}
                  disabled={loading}
                >
                  <span className="prompt-source-icon">{sourceStatusIcon(source)}</span>
                  <span className="prompt-source-copy">
                    <span className="prompt-source-name">
                      <strong>{source.display_path}</strong>
                      {source.is_symlink ? <Link2 size={13} /> : null}
                    </span>
                    <span className="prompt-source-meta">
                      <span>{t(`prompts.scopes.${source.scope}`)}</span>
                      <span>{t(`prompts.kinds.${source.kind}`)}</span>
                      {!source.exists ? <span>{t('prompts.missing')}</span> : null}
                    </span>
                  </span>
                  {source.git_status ? (
                    <span className="prompt-git-status" title={source.git_status}>
                      <FolderGit2 size={13} />
                    </span>
                  ) : null}
                </button>
              ))
            )}
          </div>
        </aside>

        <section className="prompt-editor-panel">
          {document ? (
            <>
              <div className="prompt-editor-header">
                <div className="prompt-editor-heading">
                  <div>
                    <strong>{document.source.display_path}</strong>
                    {dirty ? <span className="prompt-dirty-dot">{t('prompts.unsaved')}</span> : null}
                  </div>
                  <code title={document.source.path}>{document.source.path}</code>
                </div>
                <div className="prompt-editor-actions">
                  <button
                    className="btn btn-quiet"
                    type="button"
                    onClick={onOpenHistory}
                    disabled={saving}
                  >
                    <History size={15} />
                    {t('prompts.history')}
                  </button>
                  <button
                    className="btn btn-quiet"
                    type="button"
                    onClick={onReload}
                    disabled={saving}
                  >
                    <RefreshCw size={15} />
                    {t('prompts.reload')}
                  </button>
                  {document.source.exists && document.source.can_delete ? (
                    <button
                      className="btn btn-quiet danger"
                      type="button"
                      onClick={onDelete}
                      disabled={saving}
                    >
                      <Trash2 size={15} />
                      {t('prompts.delete')}
                    </button>
                  ) : null}
                  <button
                    className="btn btn-primary"
                    type="button"
                    onClick={onSave}
                    disabled={!dirty || editorDisabled}
                  >
                    <Save size={15} />
                    {saving ? t('prompts.saving') : t('prompts.save')}
                  </button>
                </div>
              </div>

              {externalChange ? (
                <div className="prompt-alert warning">
                  <AlertTriangle size={17} />
                  <div>
                    <strong>{t('prompts.externalChangeTitle')}</strong>
                    <span>{t('prompts.externalChangeBody')}</span>
                  </div>
                  <button className="btn btn-secondary" type="button" onClick={onReload}>
                    {t('prompts.reloadDisk')}
                  </button>
                </div>
              ) : null}
              {conflict ? (
                <div className="prompt-alert danger">
                  <XCircle size={17} />
                  <div>
                    <strong>{t('prompts.conflictTitle')}</strong>
                    <span>{t('prompts.conflictBody')}</span>
                  </div>
                  <div className="prompt-alert-actions">
                    <button className="btn btn-secondary" type="button" onClick={onReload}>
                      {t('prompts.useDisk')}
                    </button>
                    <button className="btn btn-danger" type="button" onClick={onForceSave}>
                      {t('prompts.overwrite')}
                    </button>
                  </div>
                </div>
              ) : null}
              {document.source.is_symlink ? (
                <div className="prompt-alert info">
                  <Link2 size={17} />
                  <div>
                    <strong>{t('prompts.symlinkTitle')}</strong>
                    <span>{t('prompts.symlinkBody', { path: document.source.real_path ?? document.source.link_target ?? '' })}</span>
                  </div>
                </div>
              ) : null}
              {document.read_only || document.warnings.length > 0 || document.source.warning ? (
                <div className="prompt-alert warning">
                  <LockKeyhole size={17} />
                  <div>
                    <strong>{document.read_only ? t('prompts.readOnly') : t('prompts.warning')}</strong>
                    <span>
                      {[...document.warnings, document.source.warning]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                  </div>
                </div>
              ) : null}
              {lastDeletedRevisionId && !document.source.exists ? (
                <div className="prompt-alert success">
                  <CheckCircle2 size={17} />
                  <div>
                    <strong>{t('prompts.deletedRecoverably')}</strong>
                    <span>{t('prompts.deletedRecoverablyBody')}</span>
                  </div>
                  <button className="btn btn-secondary" type="button" onClick={onRestoreLastDelete}>
                    <RotateCcw size={15} />
                    {t('prompts.undoDelete')}
                  </button>
                </div>
              ) : null}

              <textarea
                className="prompt-editor-textarea"
                value={editorContent}
                onChange={(event) => onContentChange(event.target.value)}
                disabled={editorDisabled}
                spellCheck={false}
                aria-label={t('prompts.editorLabel')}
                placeholder={
                  document.source.exists
                    ? t('prompts.editorPlaceholder')
                    : t('prompts.createPlaceholder')
                }
              />
              <div className="prompt-editor-statusbar">
                <span>{document.line_ending.toUpperCase()}</span>
                {document.has_bom ? <span>UTF-8 BOM</span> : <span>UTF-8</span>}
                <span>{t('prompts.characterCount', { count: editorContent.length })}</span>
                {document.source.modified_at ? (
                  <span>{formatTimestamp(document.source.modified_at)}</span>
                ) : null}
                <span className="prompt-next-session-note">
                  {t('prompts.nextSessionEffect')}
                </span>
              </div>
            </>
          ) : (
            <div className="prompt-editor-empty">
              <FileText size={30} />
              <strong>{t('prompts.selectSource')}</strong>
              <span>{t('prompts.selectSourceBody')}</span>
            </div>
          )}
        </section>

      </div>
    </div>
  )
}

export default memo(PromptsPage)
