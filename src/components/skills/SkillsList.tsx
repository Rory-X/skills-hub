import { memo } from 'react'
import { MessageCircle } from 'lucide-react'
import type { TFunction } from 'i18next'
import type { ManagedSkill, OnboardingPlan, ToolOption } from './types'
import SkillCard from './SkillCard'

type GithubInfo = {
  label: string
  href: string
}

type SkillsListProps = {
  plan: OnboardingPlan | null
  visibleSkills: ManagedSkill[]
  installedTools: ToolOption[]
  loading: boolean
  bulkMode: boolean
  selectedSkillIds: string[]
  highlightedSkillIds: string[]
  getGithubInfo: (url: string | null | undefined) => GithubInfo | null
  getSkillSourceLabel: (skill: ManagedSkill) => string
  formatRelative: (ms: number | null | undefined) => string
  onReviewImport: () => void
  onUpdateSkill: (skill: ManagedSkill) => void
  onDeleteSkill: (skillId: string) => void
  onToggleSkillEnabled: (skill: ManagedSkill) => void
  onOpenScope: (skill: ManagedSkill) => void
  onOpenDetail: (skill: ManagedSkill) => void
  onEditTags: (skill: ManagedSkill) => void
  onToggleBulkSelection: (skillId: string) => void
  getSkillScope: (skill: ManagedSkill) => 'global' | 'project'
  getSkillProjects: (skill: ManagedSkill) => string[]
  t: TFunction
}

const SkillsList = ({
  plan,
  visibleSkills,
  installedTools,
  loading,
  bulkMode,
  selectedSkillIds,
  highlightedSkillIds,
  getGithubInfo,
  getSkillSourceLabel,
  formatRelative,
  onReviewImport,
  onUpdateSkill,
  onDeleteSkill,
  onToggleSkillEnabled,
  onOpenScope,
  onOpenDetail,
  onEditTags,
  onToggleBulkSelection,
  getSkillScope,
  getSkillProjects,
  t,
}: SkillsListProps) => {
  const selectedSkillSet = new Set(selectedSkillIds)
  const highlightedSkillSet = new Set(highlightedSkillIds)

  return (
    <div className="skills-list">
      {plan && plan.total_skills_found > 0 ? (
        <div className="discovered-banner">
          <div className="banner-left">
            <div className="banner-icon">
              <MessageCircle size={18} />
            </div>
            <div className="banner-content">
              <div className="banner-title">{t('discoveredTitle')}</div>
              <div className="banner-subtitle">
                {t('discoveredCount', { count: plan.total_skills_found })}
              </div>
            </div>
          </div>
          <button
            className="btn btn-warning"
            type="button"
            onClick={onReviewImport}
            disabled={loading}
          >
            {t('reviewImport')}
          </button>
        </div>
      ) : null}

      {visibleSkills.length === 0 ? (
        <div className="empty">{t('skillsEmpty')}</div>
      ) : (
        <>
          {visibleSkills.map((skill) => (
            <SkillCard
              key={skill.id}
              skill={skill}
              installedTools={installedTools}
              loading={loading}
              bulkMode={bulkMode}
              bulkSelected={selectedSkillSet.has(skill.id)}
              highlighted={highlightedSkillSet.has(skill.id)}
              getGithubInfo={getGithubInfo}
              getSkillSourceLabel={getSkillSourceLabel}
              formatRelative={formatRelative}
              onUpdate={onUpdateSkill}
              onDelete={onDeleteSkill}
              onToggleEnabled={onToggleSkillEnabled}
              onOpenScope={onOpenScope}
              onOpenDetail={onOpenDetail}
              onEditTags={onEditTags}
              onToggleBulkSelection={onToggleBulkSelection}
              getSkillScope={getSkillScope}
              getSkillProjects={getSkillProjects}
              t={t}
            />
          ))}
        </>
      )}
    </div>
  )
}

export default memo(SkillsList)
