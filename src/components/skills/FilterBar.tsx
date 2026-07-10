import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUpDown, Check, CheckSquare, ChevronDown, Search, Tags } from 'lucide-react'
import type { TFunction } from 'i18next'
import type { TagWithCountDto } from './types'

type FilterBarProps = {
  listMode: 'all' | 'pending'
  allCount: number
  pendingCount: number
  sortBy: 'updated' | 'name'
  searchQuery: string
  scopeFilter: 'all' | 'global' | 'project'
  tags: TagWithCountDto[]
  selectedTagIds: number[]
  includeUntagged: boolean
  untaggedCount: number
  bulkMode: boolean
  bulkSelectedCount: number
  onSortChange: (value: 'updated' | 'name') => void
  onListModeChange: (value: 'all' | 'pending') => void
  onSearchChange: (value: string) => void
  onScopeFilterChange: (value: 'all' | 'global' | 'project') => void
  onToggleTag: (tagId: number) => void
  onToggleUntagged: () => void
  onClearTags: () => void
  onManageTags: () => void
  onToggleBulkMode: () => void
  t: TFunction
}

const FilterBar = ({
  listMode,
  allCount,
  pendingCount,
  sortBy,
  searchQuery,
  scopeFilter,
  tags,
  selectedTagIds,
  includeUntagged,
  untaggedCount,
  bulkMode,
  bulkSelectedCount,
  onSortChange,
  onListModeChange,
  onSearchChange,
  onScopeFilterChange,
  onToggleTag,
  onToggleUntagged,
  onClearTags,
  onManageTags,
  onToggleBulkMode,
  t,
}: FilterBarProps) => {
  const [tagMenuOpen, setTagMenuOpen] = useState(false)
  const [tagQuery, setTagQuery] = useState('')
  const tagMenuRef = useRef<HTMLDivElement | null>(null)
  const scopeOptions: { value: 'all' | 'global' | 'project'; label: string }[] = [
    { value: 'all', label: t('scope.all') },
    { value: 'global', label: t('scope.global') },
    { value: 'project', label: t('scope.project') },
  ]
  const selectedTagSet = useMemo(() => new Set(selectedTagIds), [selectedTagIds])
  const selectedCount = selectedTagIds.length + (includeUntagged ? 1 : 0)
  const filteredTags = useMemo(() => {
    const query = tagQuery.trim().toLowerCase()
    if (!query) return tags
    return tags.filter((tag) => tag.name.toLowerCase().includes(query))
  }, [tagQuery, tags])

  useEffect(() => {
    if (!tagMenuOpen) return
    const handlePointerDown = (event: MouseEvent) => {
      if (!tagMenuRef.current?.contains(event.target as Node)) {
        setTagMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handlePointerDown)
    return () => document.removeEventListener('mousedown', handlePointerDown)
  }, [tagMenuOpen])

  return (
    <div className="filter-bar">
      <div className="skill-list-tabs" role="tablist" aria-label={t('allSkills')}>
        <button
          className={listMode === 'all' ? 'active' : ''}
          type="button"
          role="tab"
          aria-selected={listMode === 'all'}
          onClick={() => onListModeChange('all')}
        >
          {t('allSkills')}
          <span>{allCount}</span>
        </button>
        <button
          className={listMode === 'pending' ? 'active' : ''}
          type="button"
          role="tab"
          aria-selected={listMode === 'pending'}
          onClick={() => onListModeChange('pending')}
        >
          {t('assignment.pendingTab')}
          <span>{pendingCount}</span>
        </button>
      </div>
      <div className="filter-actions">
        <button className="btn btn-secondary sort-btn" type="button">
          {scopeOptions.find((option) => option.value === scopeFilter)?.label ?? t('scope.all')}
          <ChevronDown size={12} />
          <select
            aria-label={t('scope.filterLabel')}
            value={scopeFilter}
            onChange={(event) =>
              onScopeFilterChange(event.target.value as 'all' | 'global' | 'project')
            }
          >
            {scopeOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </button>
        <button className="btn btn-secondary sort-btn" type="button">
          {sortBy === 'updated' ? t('sortUpdated') : t('sortName')}
          <ArrowUpDown size={12} />
          <select
            aria-label={t('filterSort')}
            value={sortBy}
            onChange={(event) => onSortChange(event.target.value as 'updated' | 'name')}
          >
            <option value="updated">{t('sortUpdated')}</option>
            <option value="name">{t('sortName')}</option>
          </select>
        </button>
        <button
          className={`btn btn-secondary bulk-mode-btn${bulkMode ? ' active' : ''}`}
          type="button"
          onClick={onToggleBulkMode}
        >
          <CheckSquare size={14} />
          {bulkMode
            ? t('bulk.selectedShort', { count: bulkSelectedCount })
            : t('bulk.manage')}
        </button>
        <div className="tag-filter-wrap" ref={tagMenuRef}>
          <button
            className={`btn btn-secondary tag-filter-btn${selectedCount > 0 ? ' active' : ''}`}
            type="button"
            onClick={() => setTagMenuOpen((open) => !open)}
          >
            <Tags size={14} />
            {selectedCount > 0
              ? t('tagsSelected', { count: selectedCount })
              : t('tags')}
            <ChevronDown size={12} />
          </button>
          {tagMenuOpen ? (
            <div className="tag-filter-menu">
              <div className="tag-filter-head">
                <span>{t('tags')}</span>
                <span>{t('matchAny')}</span>
              </div>
              <div className="tag-filter-search">
                <Search size={15} />
                <input
                  value={tagQuery}
                  onChange={(event) => setTagQuery(event.target.value)}
                  placeholder={t('searchTags')}
                />
              </div>
              <div className="tag-filter-options">
                <button
                  className={`tag-filter-option${includeUntagged ? ' selected' : ''}`}
                  type="button"
                  onClick={onToggleUntagged}
                >
                  <span className="tag-check">{includeUntagged ? <Check size={14} /> : null}</span>
                  <span>{t('untagged')}</span>
                  <span className="tag-count">{untaggedCount}</span>
                </button>
                {filteredTags.map((tag) => {
                  const selected = selectedTagSet.has(tag.id)
                  return (
                    <button
                      key={tag.id}
                      className={`tag-filter-option${selected ? ' selected' : ''}`}
                      type="button"
                      onClick={() => onToggleTag(tag.id)}
                    >
                      <span className="tag-check">{selected ? <Check size={14} /> : null}</span>
                      <span>{tag.name}</span>
                      <span className="tag-count">{tag.skill_count}</span>
                    </button>
                  )
                })}
              </div>
              <div className="tag-filter-footer">
                <button type="button" onClick={onClearTags} disabled={selectedCount === 0}>
                  {t('clearAll')}
                </button>
                <button type="button" onClick={onManageTags}>
                  {t('manageTags')}
                </button>
              </div>
            </div>
          ) : null}
        </div>
        <div className="search-container">
          <Search size={16} className="search-icon-abs" />
          <input
            className="search-input"
            value={searchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={t('searchPlaceholder')}
          />
        </div>
      </div>
    </div>
  )
}

export default memo(FilterBar)
