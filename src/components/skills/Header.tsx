import { memo } from 'react'
import {
  FolderKanban,
  Library,
  Search,
  Settings,
  SlidersHorizontal,
} from 'lucide-react'
import type { TFunction } from 'i18next'

type HeaderProps = {
  language: string
  loading: boolean
  activeView:
    | 'projects'
    | 'projectDetail'
    | 'myskills'
    | 'explore'
    | 'settings'
    | 'manage'
  onToggleLanguage: () => void
  onOpenSettings: () => void
  onViewChange: (view: 'projects' | 'myskills' | 'explore' | 'manage') => void
  t: TFunction
}

const Header = ({
  language,
  activeView,
  onToggleLanguage,
  onOpenSettings,
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
            className={`nav-tab${activeView === 'explore' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('explore')}
          >
            <Search size={16} />
            {t('navExplore')}
          </button>
          <button
            className={`nav-tab${activeView === 'manage' ? ' active' : ''}`}
            type="button"
            onClick={() => onViewChange('manage')}
          >
            <SlidersHorizontal size={16} />
            {t('navManageCenter')}
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
