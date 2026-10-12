import { memo } from 'react'
import {
  FileText,
  FolderKanban,
  Library,
  Search,
  Settings,
  SlidersHorizontal,
  Workflow,
} from 'lucide-react'
import type { TFunction } from 'i18next'

type HeaderProps = {
  language: string
  loading: boolean
  activeView:
    | 'projects'
    | 'projectDetail'
    | 'myskills'
    | 'templates'
    | 'explore'
    | 'prompts'
    | 'settings'
    | 'manage'
  onToggleLanguage: () => void
  onOpenSettings: () => void
  onOpenUpdates: () => void
  updateFailureCount: number
  onViewChange: (
    view: 'projects' | 'myskills' | 'templates' | 'explore' | 'prompts' | 'manage',
  ) => void
  t: TFunction
}

const Header = ({
  language,
  activeView,
  onToggleLanguage,
  onOpenSettings,
  onOpenUpdates,
  updateFailureCount,
  onViewChange,
  t,
}: HeaderProps) => {
  return (
    <header className="skills-header">
      <div className="header-left">
        <div className="brand-area">
          <img className="logo-icon" src="/logo.png" alt="" />
          <div className="brand-text-wrap">
            <div className="brand-text">{t('appName')}</div>
          </div>
        </div>
        <nav className="nav-tabs">
          <button
            className={`nav-tab${activeView === 'projects' || activeView === 'projectDetail' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('projects')}
          >
            <FolderKanban size={16} />
            {t('navProjects')}
          </button>
          <button
            className={`nav-tab${activeView === 'myskills' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('myskills')}
          >
            <Library size={16} />
            {t('navSkillLibrary')}
          </button>
          <button
            className={`nav-tab${activeView === 'templates' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('templates')}
          >
            <Workflow size={16} />
            {t('navTemplates')}
          </button>
          <button
            className={`nav-tab${activeView === 'explore' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('explore')}
          >
            <Search size={16} />
            {t('navExplore')}
          </button>
          <button
            className={`nav-tab${activeView === 'prompts' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('prompts')}
          >
            <FileText size={16} />
            {t('navPrompts')}
          </button>
          <button
            className={`nav-tab${activeView === 'manage' ? ' active' : ''}`}
            type="button"
            onClick={() => {
              if (updateFailureCount > 0) {
                onOpenUpdates()
                return
              }
              onViewChange('manage')
            }}
          >
            <SlidersHorizontal size={16} />
            {t('navManageCenter')}
            {updateFailureCount > 0 ? (
              <span className="nav-alert-badge">
                {updateFailureCount > 99 ? '99+' : updateFailureCount}
              </span>
            ) : null}
          </button>
        </nav>
      </div>
      <div className="header-actions">
        <button className="lang-btn" type="button" onClick={onToggleLanguage}>
          {language === 'en' ? t('languageShort.en') : t('languageShort.zh')}
        </button>
        <button className={`icon-btn${activeView === 'settings' ? ' active' : ''}`} type="button" onClick={onOpenSettings}>
          <Settings size={18} />
        </button>
      </div>
    </header>
  )
}

export default memo(Header)
