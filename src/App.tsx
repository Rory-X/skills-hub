import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import './App.css'
import { useTranslation } from 'react-i18next'
import { Toaster, toast } from 'sonner'
import ExplorePage from './components/skills/ExplorePage'
import FilterBar from './components/skills/FilterBar'
import SkillDetailView from './components/skills/SkillDetailView'
import Header from './components/skills/Header'
import LoadingOverlay from './components/skills/LoadingOverlay'
import SkillsList from './components/skills/SkillsList'
import TagsPage from './components/skills/TagsPage'
import AddSkillModal from './components/skills/modals/AddSkillModal'
import AssignmentModal from './components/skills/modals/AssignmentModal'
import BulkDeleteModal from './components/skills/modals/BulkDeleteModal'
import BulkTagsModal from './components/skills/modals/BulkTagsModal'
import DeleteModal from './components/skills/modals/DeleteModal'
import EditSkillTagsModal from './components/skills/modals/EditSkillTagsModal'
import GitPickModal from './components/skills/modals/GitPickModal'
import LocalPickModal from './components/skills/modals/LocalPickModal'
import ImportModal from './components/skills/modals/ImportModal'
import NewToolsModal from './components/skills/modals/NewToolsModal'
import SettingsPage from './components/skills/SettingsPage'
import ToolsPage from './components/skills/ToolsPage'
import UpdatesPage from './components/skills/UpdatesPage'
import ProjectDetailPage from './components/projects/ProjectDetailPage'
import ProjectsPage from './components/projects/ProjectsPage'
import {
  getAutoUpdateToastKey,
  shouldKeepWaitingForTriggeredAutoUpdate,
} from './components/skills/autoUpdateSettings'
import {
  getDiscoveredImportDecision,
  type DiscoveredHubResolution,
} from './components/skills/discoveredImportResolution'
import { getGitImportCustomName } from './components/skills/gitImportName'
import {
  assignmentDraftForSkill,
  assignmentDraftReplacingProjectTools,
} from './components/skills/assignmentDraft'
import { getAvailableProjectToolKeys } from './components/skills/toolAvailability'
import { selectEntireLibrary } from './components/skills/bulkAssignmentWorkflow'
import type {
  AutoUpdateConfigDto,
  AssignmentApplyResultDto,
  AssignmentDraftDto,
  AssignmentPreviewDto,
  FeaturedSkillDto,
  GitSkillCandidate,
  GithubProxyConfigDto,
  InstallResultDto,
  LocalSkillCandidate,
  ManagedSkill,
  OnboardingPlan,
  OnlineSkillDto,
  ProjectDetailDto,
  ProjectIssueActionDto,
  ProjectSummaryDto,
  TagWithCountDto,
  ToolConfigDto,
  ToolOption,
  ToolStatusDto,
  UpdateResultDto,
} from './components/skills/types'

type SkillScopeState = Record<
  string,
  {
    scope: 'global' | 'project'
    projects: string[]
  }
>

type ActiveView =
  | 'projects'
  | 'projectDetail'
  | 'myskills'
  | 'explore'
  | 'detail'
  | 'settings'
  | 'manage'

type ProjectAssignmentBatch = {
  mode: 'library-add' | 'current-tools'
  skillIds: string[]
}
type ManagementTab = 'tags' | 'tools' | 'updates'

function App() {
  const { t, i18n } = useTranslation()
  const language = i18n.resolvedLanguage ?? i18n.language ?? 'en'
  const languageStorageKey = 'skills-language'
  const themeStorageKey = 'skills-theme'
  const skillScopeStorageKey = 'skills-project-scope-state-v1'
  const toggleLanguage = useCallback(() => {
    void i18n.changeLanguage(language === 'en' ? 'zh' : 'en')
  }, [i18n, language])
  const [themePreference, setThemePreference] = useState<'system' | 'light' | 'dark'>(
    'system',
  )
  const [systemTheme, setSystemTheme] = useState<'light' | 'dark'>('light')
  const [plan, setPlan] = useState<OnboardingPlan | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [variantChoice, setVariantChoice] = useState<Record<string, string>>({})
  const [hubResolutions, setHubResolutions] = useState<
    Record<string, DiscoveredHubResolution | undefined>
  >({})
  const [actionMessage, setActionMessage] = useState<string | null>(null)
  const [successToastMessage, setSuccessToastMessage] = useState<string | null>(
    null,
  )
  const [managedSkills, setManagedSkills] = useState<ManagedSkill[]>([])
  const [highlightedSkillIds, setHighlightedSkillIds] = useState<string[]>([])
  const [localPath, setLocalPath] = useState('')
  const [localName, setLocalName] = useState('')
  const [gitUrl, setGitUrl] = useState('')
  const [gitName, setGitName] = useState('')
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const [gitCandidates, setGitCandidates] = useState<GitSkillCandidate[]>([])
  const [gitCandidatesRepoUrl, setGitCandidatesRepoUrl] = useState<string>('')
  const [showGitPickModal, setShowGitPickModal] = useState(false)
  const [gitCandidateSelected, setGitCandidateSelected] = useState<
    Record<string, boolean>
  >({})
  const [localCandidates, setLocalCandidates] = useState<LocalSkillCandidate[]>([])
  const [localCandidatesBasePath, setLocalCandidatesBasePath] = useState('')
  const [showLocalPickModal, setShowLocalPickModal] = useState(false)
  const [localCandidateSelected, setLocalCandidateSelected] = useState<
    Record<string, boolean>
  >({})
  const [loadingStartAt, setLoadingStartAt] = useState<number | null>(null)
  const [toolStatus, setToolStatus] = useState<ToolStatusDto | null>(null)
  const [toolConfig, setToolConfig] = useState<ToolConfigDto | null>(null)
  const [showNewToolsModal, setShowNewToolsModal] = useState(false)
  const [showAddModal, setShowAddModal] = useState(false)
  const [reimportSkill, setReimportSkill] = useState<ManagedSkill | null>(null)
  const [showImportModal, setShowImportModal] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [sortBy, setSortBy] = useState<'updated' | 'name'>('updated')
  const [scopeFilter, setScopeFilter] = useState<'all' | 'global' | 'project'>('all')
  const [skillListMode, setSkillListMode] = useState<'all' | 'pending'>('all')
  const [activeView, setActiveView] = useState<ActiveView>('projects')
  const [managementTab, setManagementTab] = useState<ManagementTab>('tags')
  const [detailSkill, setDetailSkill] = useState<ManagedSkill | null>(null)
  const [tags, setTags] = useState<TagWithCountDto[]>([])
  const [selectedTagIds, setSelectedTagIds] = useState<number[]>([])
  const [includeUntagged, setIncludeUntagged] = useState(false)
  const [tagEditorSkill, setTagEditorSkill] = useState<ManagedSkill | null>(null)
  const [pendingDeleteTag, setPendingDeleteTag] = useState<TagWithCountDto | null>(null)
  const [addModalTab, setAddModalTab] = useState<'local' | 'git'>('git')
  const [addModalTagIds, setAddModalTagIds] = useState<number[]>([])
  const [featuredSkills, setFeaturedSkills] = useState<FeaturedSkillDto[]>([])
  const [featuredLoading, setFeaturedLoading] = useState(false)
  const [exploreFilter, setExploreFilter] = useState('')
  const [searchResults, setSearchResults] = useState<OnlineSkillDto[]>([])
  const [searchLoading, setSearchLoading] = useState(false)
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [autoSelectSkillName, setAutoSelectSkillName] = useState<string | null>(null)
  const [scopeModalSkill, setScopeModalSkill] = useState<ManagedSkill | null>(null)
  const [showBatchAssignmentModal, setShowBatchAssignmentModal] = useState(false)
  const [assignmentInitialDraft, setAssignmentInitialDraft] =
    useState<AssignmentDraftDto | null>(null)
  const [projects, setProjects] = useState<ProjectSummaryDto[]>([])
  const [projectImportPath, setProjectImportPath] = useState('')
  const [selectedProjectDetail, setSelectedProjectDetail] =
    useState<ProjectDetailDto | null>(null)
  const [skillScopeState, setSkillScopeState] = useState<SkillScopeState>({})
  const [bulkMode, setBulkMode] = useState(false)
  const [bulkSelectedIds, setBulkSelectedIds] = useState<string[]>([])
  const [projectAssignmentBatch, setProjectAssignmentBatch] =
    useState<ProjectAssignmentBatch | null>(null)
  const [showBulkDeleteModal, setShowBulkDeleteModal] = useState(false)
  const [showBulkTagsModal, setShowBulkTagsModal] = useState(false)

  const isTauri =
    typeof window !== 'undefined' &&
    Boolean(
      (window as { __TAURI__?: unknown }).__TAURI__ ||
        (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
    )

  const invokeTauri = useCallback(
    async <T,>(command: string, args?: Record<string, unknown>) => {
      if (!isTauri) {
        throw new Error('Tauri API is not available')
      }
      const { invoke } = await import('@tauri-apps/api/core')
      return invoke<T>(command, args)
    },
    [isTauri],
  )
  const formatErrorMessage = useCallback(
    (raw: string) => {
      if (raw.includes('CANCELLED|')) {
        return '' // Silently ignore cancelled operations
      }
      if (raw.includes('skill already exists in central repo')) {
        // Extract skill name from path like: skill already exists in central repo: "/path/to/react-best-practices"
        const pathMatch = raw.match(/central repo:\s*"?([^"]+)"?/)
        if (pathMatch) {
          const skillName = pathMatch[1].split('/').pop() ?? ''
          if (skillName) {
            return t('errors.skillExistsInHubNamed', { name: skillName })
          }
        }
        return t('errors.skillExistsInHub')
      }
      if (raw.startsWith('TARGET_EXISTS|')) {
        return t('errors.targetExists')
      }
      if (raw.startsWith('TOOL_NOT_INSTALLED|')) {
        return t('errors.toolNotInstalled')
      }
      if (raw.startsWith('TOOL_NOT_WRITABLE|')) {
        const parts = raw.split('|')
        return t('errors.toolNotWritable', { tool: parts[1] ?? '', path: parts[2] ?? '' })
      }
      if (raw.startsWith('PROJECT_SCOPE_UNSUPPORTED|')) {
        const tool = raw.split('|')[1] ?? ''
        return t('projectSync.unsupportedTool', { tool })
      }
      if (raw.startsWith('PROJECT_LIBRARY_NAME_REQUIRED|')) {
        return t('projects.libraryNameRequired')
      }
      if (raw.startsWith('HUB_NAME_CONFLICT|')) {
        return t('errors.resolveHubConflict')
      }
      if (raw.includes('未在该仓库中发现可导入的 Skills')) {
        return t('errors.noSkillsFoundInRepo')
      }
      return raw
    },
    [t],
  )
  const showActionErrors = useCallback(
    (errors: { title: string; message: string }[]) => {
      if (errors.length === 0) return
      const head = errors[0]
      const more =
        errors.length > 1
          ? t('errors.moreCount', { count: errors.length - 1 })
          : ''
      toast.error(
        `${formatErrorMessage(`${head.title}\n${head.message}`)}${more}`,
        { duration: 3200 },
      )
    },
    [formatErrorMessage, t],
  )
  const isSkillNameTaken = useCallback(
    (name: string) =>
      managedSkills.some((skill) => skill.name.toLowerCase() === name.toLowerCase()),
    [managedSkills],
  )

  const formatRelative = (ms: number | null | undefined) => {
    if (!ms) return t('relative.empty')
    const diff = Date.now() - ms
    if (diff < 0) return t('relative.empty')
    const minutes = Math.floor(diff / 60000)
    if (minutes < 1) return t('relative.justNow')
    if (minutes < 60) {
      return t('relative.minutesAgo', { minutes })
    }
    const hours = Math.floor(minutes / 60)
    if (hours < 24) {
      return t('relative.hoursAgo', { hours })
    }
    const days = Math.floor(hours / 24)
    return t('relative.daysAgo', { days })
  }

  const getSkillSourceLabel = (skill: ManagedSkill) => {
    const key = skill.source_type.toLowerCase()
    if (key.includes('git') && skill.source_ref) {
      return skill.source_ref
    }
    return skill.central_path
  }

  const getGithubInfo = (url: string | null | undefined) => {
    if (!url) return null
    const normalized = url.replace(/^git\+/, '')
    try {
      const parsed = new URL(normalized)
      if (!parsed.hostname.includes('github.com')) return null
      const parts = parsed.pathname.split('/').filter(Boolean)
      const owner = parts[0]
      const repo = parts[1]?.replace(/\.git$/, '')
      if (!owner || !repo) return null
      return {
        label: `${owner}/${repo}`,
        href: `https://github.com/${owner}/${repo}`,
      }
    } catch {
      const match = normalized.match(/github\.com\/([^/]+)\/([^/#?]+)/i)
      if (!match) return null
      const owner = match[1]
      const repo = match[2].replace(/\.git$/, '')
      return {
        label: `${owner}/${repo}`,
        href: `https://github.com/${owner}/${repo}`,
      }
    }
  }

  const loadPlan = useCallback(async () => {
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      const result = await invokeTauri<OnboardingPlan>('get_onboarding_plan')
      setPlan(result)
      const defaultSelected: Record<string, boolean> = {}
      const defaultChoice: Record<string, string> = {}
      result.groups.forEach((group) => {
        defaultSelected[group.name] = true
        const first = group.variants[0]
        if (first) {
          defaultChoice[group.name] = first.path
        }
      })
      setSelected(defaultSelected)
      setVariantChoice(defaultChoice)
      setHubResolutions({})
      return result
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      return null
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }, [invokeTauri])

  const loadManagedSkills = useCallback(async () => {
    try {
      const result = await invokeTauri<ManagedSkill[]>('get_managed_skills')
      setManagedSkills(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri])

  const highlightSkill = useCallback((skillId: string) => {
    setHighlightedSkillIds((current) => Array.from(new Set([...current, skillId])))
    window.setTimeout(() => {
      setHighlightedSkillIds((current) => current.filter((id) => id !== skillId))
    }, 5000)
  }, [])

  const loadTags = useCallback(async () => {
    try {
      const result = await invokeTauri<TagWithCountDto[]>('get_tags')
      setTags(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri])

  const loadProjects = useCallback(async () => {
    try {
      const result = await invokeTauri<ProjectSummaryDto[]>('list_projects')
      setProjects(result)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri])

  useEffect(() => {
    if (isTauri) {
      loadManagedSkills()
      loadTags()
      loadProjects()
    }
  }, [isTauri, loadManagedSkills, loadProjects, loadTags])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      const raw = window.localStorage.getItem(skillScopeStorageKey)
      if (raw) {
        setSkillScopeState(JSON.parse(raw) as SkillScopeState)
      }
    } catch {
      setSkillScopeState({})
    }
  }, [skillScopeStorageKey])

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      window.localStorage.setItem(
        skillScopeStorageKey,
        JSON.stringify(skillScopeState),
      )
    } catch {
      // ignore storage failures
    }
  }, [skillScopeState, skillScopeStorageKey])

  useEffect(() => {
    if (typeof window === 'undefined') return
    const stored = window.localStorage.getItem(themeStorageKey)
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      setThemePreference(stored)
    }
  }, [themeStorageKey])

  useEffect(() => {
    if (typeof window === 'undefined') return
    if (language !== 'en' && language !== 'zh') return
    try {
      window.localStorage.setItem(languageStorageKey, language)
    } catch {
      // ignore storage failures
    }
  }, [language, languageStorageKey])

  useEffect(() => {
    if (typeof window === 'undefined') return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const update = () => {
      setSystemTheme(media.matches ? 'dark' : 'light')
    }
    update()
    if (media.addEventListener) {
      media.addEventListener('change', update)
    } else {
      media.addListener(update)
    }
    return () => {
      if (media.removeEventListener) {
        media.removeEventListener('change', update)
      } else {
        media.removeListener(update)
      }
    }
  }, [])

  useEffect(() => {
    if (typeof document === 'undefined') return
    const resolvedTheme =
      themePreference === 'system' ? systemTheme : themePreference
    document.documentElement.dataset.theme = resolvedTheme
    document.documentElement.style.colorScheme = resolvedTheme
    try {
      window.localStorage.setItem(themeStorageKey, themePreference)
    } catch {
      // ignore storage failures
    }
  }, [systemTheme, themePreference, themeStorageKey])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<string>('get_central_repo_path')
      .then((path) => setStoragePath(path))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<number>('get_git_cache_cleanup_days')
      .then((days) => setGitCacheCleanupDays(days))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<number>('get_git_cache_ttl_secs')
      .then((secs) => setGitCacheTtlSecs(secs))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<string>('get_github_token')
      .then((token) => setGithubToken(token))
      .catch(() => {})
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<GithubProxyConfigDto>('get_github_proxy_config')
      .then((config) => setGithubProxyConfig(config))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
      .then((config) => setAutoUpdateConfig(config))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (!isTauri) return
    invokeTauri<ToolConfigDto>('get_tool_config')
      .then((config) => setToolConfig(config))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }, [isTauri, invokeTauri])

  useEffect(() => {
    if (isTauri) {
      void loadPlan()
    }
  }, [isTauri, loadPlan])

  useEffect(() => {
    if (!successToastMessage) return
    toast.success(successToastMessage, { duration: 1800 })
    setSuccessToastMessage(null)
  }, [successToastMessage])

  useEffect(() => {
    if (!error) return
    const msg = formatErrorMessage(error)
    if (msg) toast.error(msg, { duration: 2600 })
    setError(null)
    setActionMessage(null)
  }, [error, formatErrorMessage])

  const toolInfos = useMemo(() => toolStatus?.tools ?? [], [toolStatus])
  const enabledToolInfos = useMemo(
    () => toolInfos.filter((info) => info.enabled),
    [toolInfos],
  )

  const tools: ToolOption[] = useMemo(() => {
    return enabledToolInfos.map((info) => ({
      id: info.key,
      // Prefer i18n label if present; fallback to backend label.
      label: t(`tools.${info.key}`, { defaultValue: info.label }),
      supports_project_scope: info.supports_project_scope,
    }))
  }, [t, enabledToolInfos])

  const installedToolIds = useMemo(
    () => toolStatus?.installed ?? [],
    [toolStatus],
  )
  const isInstalled = useCallback(
    (id: string) => installedToolIds.includes(id),
    [installedToolIds],
  )
  const installedTools = useMemo(
    () => tools.filter((tool) => installedToolIds.includes(tool.id)),
    [tools, installedToolIds],
  )
  const getSkillProjects = useCallback(
    (skill: ManagedSkill) => {
      const projects = new Set<string>()
      for (const assignment of skill.assignments) {
        if (assignment.scope === 'project' && assignment.project_path) {
          projects.add(assignment.project_path)
        }
      }
      for (const target of skill.targets) {
        if ((target.scope ?? 'global') === 'project' && target.project_path) {
          projects.add(target.project_path)
        }
      }
      return Array.from(projects)
    },
    [],
  )

  const getSkillScope = useCallback(
    (skill: ManagedSkill): 'global' | 'project' => {
      const hasGlobalTarget =
        skill.assignments.some((assignment) => assignment.scope === 'global') ||
        skill.targets.some(
        (target) => (target.scope ?? 'global') === 'global',
      )
      const hasProjectTarget =
        skill.assignments.some((assignment) => assignment.scope === 'project') ||
        skill.targets.some(
        (target) => (target.scope ?? 'global') === 'project',
      )
      if (hasGlobalTarget && !hasProjectTarget) return 'global'
      if (hasProjectTarget && !hasGlobalTarget) return 'project'
      const stored = skillScopeState[skill.id]?.scope
      if (stored === 'global' || stored === 'project') return stored
      return hasProjectTarget ? 'project' : 'global'
    },
    [skillScopeState],
  )

  const visibleSkills = useMemo(() => {
    const query = searchQuery.trim().toLowerCase()
    const selectedTagSet = new Set(selectedTagIds)
    const hasTagFilter = selectedTagIds.length > 0 || includeUntagged
    const filtered = managedSkills.filter((skill) => {
      if (skillListMode === 'pending' && !skill.pending_assignment) return false
      if (scopeFilter !== 'all' && getSkillScope(skill) !== scopeFilter) return false
      if (hasTagFilter) {
        const matchesSelectedTag = skill.tags.some((tag) => selectedTagSet.has(tag.id))
        const matchesUntagged = includeUntagged && skill.tags.length === 0
        if (!matchesSelectedTag && !matchesUntagged) return false
      }
      if (!query) return true
      return (
        skill.name.toLowerCase().includes(query) ||
        skill.central_path.toLowerCase().includes(query) ||
        skill.source_type.toLowerCase().includes(query) ||
        skill.tags.some((tag) => tag.name.toLowerCase().includes(query))
      )
    })
    const sorted = [...filtered].sort((a, b) => {
      if (sortBy === 'name') {
        return a.name.localeCompare(b.name)
      }
      return (b.updated_at ?? 0) - (a.updated_at ?? 0)
    })
    return sorted
  }, [
    getSkillScope,
    includeUntagged,
    managedSkills,
    skillListMode,
    scopeFilter,
    searchQuery,
    selectedTagIds,
    sortBy,
  ])
  const pendingAssignmentCount = useMemo(
    () => managedSkills.filter((skill) => skill.pending_assignment).length,
    [managedSkills],
  )
  const untaggedCount = useMemo(
    () => managedSkills.filter((skill) => skill.tags.length === 0).length,
    [managedSkills],
  )
  const bulkSelectedSkills = useMemo(() => {
    const selectedSet = new Set(bulkSelectedIds)
    return managedSkills.filter((skill) => selectedSet.has(skill.id))
  }, [bulkSelectedIds, managedSkills])

  const bulkSelectedNames = useMemo(
    () => bulkSelectedSkills.map((skill) => skill.name),
    [bulkSelectedSkills],
  )
  const bulkShouldEnable = useMemo(
    () =>
      bulkSelectedSkills.length > 0 &&
      bulkSelectedSkills.every((skill) => skill.enabled === false),
    [bulkSelectedSkills],
  )
  const allVisibleBulkSelected = useMemo(() => {
    if (visibleSkills.length === 0) return false
    const selectedSet = new Set(bulkSelectedIds)
    return visibleSkills.every((skill) => selectedSet.has(skill.id))
  }, [bulkSelectedIds, visibleSkills])
  const allLibraryBulkSelected = useMemo(() => {
    if (managedSkills.length === 0) return false
    const selectedSet = new Set(bulkSelectedIds)
    return managedSkills.every((skill) => selectedSet.has(skill.id))
  }, [bulkSelectedIds, managedSkills])

  useEffect(() => {
    const existingIds = new Set(managedSkills.map((skill) => skill.id))
    setBulkSelectedIds((current) => current.filter((id) => existingIds.has(id)))
  }, [managedSkills])

  const [storagePath, setStoragePath] = useState<string>(t('notAvailable'))
  const [gitCacheCleanupDays, setGitCacheCleanupDays] = useState<number>(30)
  const [gitCacheTtlSecs, setGitCacheTtlSecs] = useState<number>(60)
  const [githubToken, setGithubToken] = useState<string>('')
  const [githubProxyConfig, setGithubProxyConfig] =
    useState<GithubProxyConfigDto>({
      enabled: false,
      port: 7890,
      url: '',
      auto_detected: false,
    })
  const [autoUpdateConfig, setAutoUpdateConfig] =
    useState<AutoUpdateConfigDto | null>(null)
  const [autoUpdateTriggering, setAutoUpdateTriggering] = useState(false)
  const autoUpdateLastRunRef = useRef<number | null>(null)

  useEffect(() => {
    if (!isTauri) return
    if (activeView !== 'manage' || managementTab !== 'updates') return

    let cancelled = false
    const refreshAutoUpdateConfig = async () => {
      const config = await invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
      if (cancelled) return

      const previousLastRun = autoUpdateLastRunRef.current
      const nextLastRun = config.last_run_at ?? null
      autoUpdateLastRunRef.current = nextLastRun
      setAutoUpdateConfig(config)

      if (
        previousLastRun !== null &&
        nextLastRun !== null &&
        nextLastRun !== previousLastRun &&
        config.last_status !== 'running'
      ) {
        await loadManagedSkills()
      }
    }

    void refreshAutoUpdateConfig().catch(() => {})
    const timer = window.setInterval(() => {
      void refreshAutoUpdateConfig().catch(() => {})
    }, autoUpdateConfig?.last_status === 'running' ? 2000 : 10000)

    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [
    activeView,
    managementTab,
    autoUpdateConfig?.last_status,
    invokeTauri,
    isTauri,
    loadManagedSkills,
  ])

  const handlePickStoragePath = useCallback(async () => {
    try {
      if (!isTauri) {
        throw new Error(t('errors.notTauri'))
      }
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('selectStoragePath'),
      })
      if (!selected || Array.isArray(selected)) return
      const newPath = await invokeTauri<string>('set_central_repo_path', {
        path: selected,
      })
      setStoragePath(newPath)
      await loadManagedSkills()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri, isTauri, loadManagedSkills, t])
  const handleGitCacheCleanupDaysChange = useCallback(
    async (nextDays: number) => {
      const normalized = Math.max(0, Math.min(nextDays, 3650))
      setGitCacheCleanupDays(normalized)
      if (!isTauri) return
      try {
        const updated = await invokeTauri<number>('set_git_cache_cleanup_days', {
          days: normalized,
        })
        setGitCacheCleanupDays(updated)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, isTauri],
  )
  const handleGitCacheTtlSecsChange = useCallback(
    async (nextSecs: number) => {
      const normalized = Math.max(0, Math.min(nextSecs, 3600))
      setGitCacheTtlSecs(normalized)
      if (!isTauri) return
      try {
        const updated = await invokeTauri<number>('set_git_cache_ttl_secs', {
          secs: normalized,
        })
        setGitCacheTtlSecs(updated)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, isTauri],
  )
  const handleGithubTokenChange = useCallback(
    async (nextToken: string) => {
      setGithubToken(nextToken)
      if (!isTauri) return
      try {
        await invokeTauri('set_github_token', { token: nextToken })
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, isTauri],
  )
  const handleGithubProxyConfigChange = useCallback(
    async (enabled: boolean, port: number) => {
      const normalizedPort = Math.max(1, Math.min(Math.round(port), 65535))
      setGithubProxyConfig((prev) => ({
        ...prev,
        enabled,
        port: normalizedPort,
        url: enabled ? `http://127.0.0.1:${normalizedPort}` : '',
        auto_detected: false,
      }))
      if (!isTauri) return
      try {
        const saved = await invokeTauri<GithubProxyConfigDto>(
          'set_github_proxy_config',
          {
            enabled,
            port: normalizedPort,
          },
        )
        setGithubProxyConfig(saved)
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        void invokeTauri<GithubProxyConfigDto>('get_github_proxy_config')
          .then((config) => setGithubProxyConfig(config))
          .catch(() => {})
      }
    },
    [invokeTauri, isTauri],
  )
  const handleToolConfigChange = useCallback(
    async (nextConfig: ToolConfigDto) => {
      setToolConfig(nextConfig)
      if (!isTauri) return
      try {
        const saved = await invokeTauri<ToolConfigDto>('set_tool_config', {
          config: nextConfig,
        })
        setToolConfig(saved)
        const status = await invokeTauri<ToolStatusDto>('get_tool_status')
        setToolStatus(status)
        toast.success(t('toolManagement.saved'), { duration: 1600 })
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, isTauri, t],
  )
  const handleAutoUpdateConfigChange = useCallback(
    async (
      enabled: boolean,
      schedule: {
        scheduleType: 'interval' | 'daily'
        intervalValue: number
        intervalUnit: 'minutes' | 'hours'
        dailyTime: string
      },
    ) => {
      const normalizedIntervalValue = schedule.intervalUnit === 'minutes'
        ? Math.max(15, Math.min(schedule.intervalValue, 24 * 30 * 60))
        : Math.max(1, Math.min(schedule.intervalValue, 24 * 30))
      const normalizedDailyTime = /^\d{2}:\d{2}$/.test(schedule.dailyTime)
        ? schedule.dailyTime
        : '03:00'
      const normalizedIntervalHours = schedule.scheduleType === 'daily'
        ? 24
        : schedule.intervalUnit === 'minutes'
          ? Math.max(1, Math.ceil(normalizedIntervalValue / 60))
          : normalizedIntervalValue
      const previousEnabled = autoUpdateConfig?.enabled
      setAutoUpdateConfig((prev) => ({
        enabled,
        interval_hours: normalizedIntervalHours,
        schedule_type: schedule.scheduleType,
        interval_value: normalizedIntervalValue,
        interval_unit: schedule.intervalUnit,
        daily_time: normalizedDailyTime,
        local_skill_count: prev?.local_skill_count ?? 0,
        protected_local_skill_count: prev?.protected_local_skill_count ?? 0,
        task_registered: prev?.task_registered ?? false,
        task_status_detail: prev?.task_status_detail ?? '',
        last_run_at: prev?.last_run_at ?? null,
        last_started_at: prev?.last_started_at ?? null,
        last_finished_at: prev?.last_finished_at ?? null,
        last_status: prev?.last_status ?? null,
        last_error: prev?.last_error ?? null,
        last_checked: prev?.last_checked ?? 0,
        last_updated: prev?.last_updated ?? 0,
        last_failed: prev?.last_failed ?? 0,
        progress: prev?.progress ?? {
          total: 0,
          succeeded: [],
          failed: [],
          running: null,
          pending: [],
        },
      }))
      if (!isTauri) return
      try {
        const updated = await invokeTauri<AutoUpdateConfigDto>(
          'set_auto_update_config',
          {
            enabled,
            intervalHours: normalizedIntervalHours,
            scheduleType: schedule.scheduleType,
            intervalValue: normalizedIntervalValue,
            intervalUnit: schedule.intervalUnit,
            dailyTime: normalizedDailyTime,
          },
        )
        setAutoUpdateConfig(updated)
        setSuccessToastMessage(t(getAutoUpdateToastKey(previousEnabled, enabled)))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        void invokeTauri<AutoUpdateConfigDto>('get_auto_update_config')
          .then((config) => setAutoUpdateConfig(config))
          .catch(() => {})
      }
    },
    [autoUpdateConfig?.enabled, invokeTauri, isTauri, t],
  )
  const handleTriggerAutoUpdateTaskNow = useCallback(async () => {
    if (!isTauri || autoUpdateTriggering) return
    const triggeredAt = Date.now()
    setAutoUpdateTriggering(true)
    setError(null)
    setAutoUpdateConfig((prev) => prev ? {
      ...prev,
      last_run_at: triggeredAt,
      last_started_at: triggeredAt,
      last_finished_at: null,
      last_status: 'running',
      last_error: null,
      last_checked: 0,
      last_updated: 0,
      last_failed: 0,
      progress: {
        total: 0,
        succeeded: [],
        failed: [],
        running: null,
        pending: [],
      },
    } : prev)
    try {
      await invokeTauri('trigger_auto_update_task_now_cmd')
      setSuccessToastMessage(t('autoUpdateTaskTriggered'))
      let sawRunning = false
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await new Promise((resolve) => window.setTimeout(resolve, 2000))
        const latestConfig = await invokeTauri<AutoUpdateConfigDto>(
          'get_auto_update_config',
        )
        if (latestConfig.last_status === 'running') {
          sawRunning = true
        }
        const keepWaiting = shouldKeepWaitingForTriggeredAutoUpdate(
          latestConfig,
          triggeredAt,
          sawRunning,
        )
        if (keepWaiting && latestConfig.last_status !== 'running') {
          continue
        }
        setAutoUpdateConfig(latestConfig)
        if (!keepWaiting) {
          await loadManagedSkills()
          break
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setAutoUpdateTriggering(false)
    }
  }, [autoUpdateTriggering, invokeTauri, isTauri, loadManagedSkills, t])
  const handleRetryAutoUpdateSkills = useCallback(
    async (skillIds: string[]) => {
      if (!isTauri || autoUpdateTriggering || skillIds.length === 0) return
      const selected = managedSkills.filter((skill) => skillIds.includes(skill.id))
      const failed: { skill_id: string; name: string; reason: string }[] = []
      const succeeded: { skill_id: string; name: string; reason: null }[] = []
      setAutoUpdateTriggering(true)
      setError(null)
      try {
        for (const skill of selected) {
          setActionMessage(t('actions.updating', { name: skill.name }))
          try {
            await invokeTauri<UpdateResultDto>('update_managed_skill', {
              skillId: skill.id,
            })
            succeeded.push({ skill_id: skill.id, name: skill.name, reason: null })
          } catch (err) {
            failed.push({
              skill_id: skill.id,
              name: skill.name,
              reason: err instanceof Error ? err.message : String(err),
            })
          }
        }
        setAutoUpdateConfig((current) =>
          current
            ? {
                ...current,
                last_failed: failed.length,
                last_updated: current.last_updated + succeeded.length,
                last_status: failed.length > 0 ? 'error' : 'ok',
                progress: {
                  ...current.progress,
                  succeeded: [
                    ...current.progress.succeeded.filter(
                      (item) => !skillIds.includes(item.skill_id),
                    ),
                    ...succeeded,
                  ],
                  failed,
                  running: null,
                  pending: [],
                },
              }
            : current,
        )
        await loadManagedSkills()
        if (failed.length > 0) {
          showActionErrors(
            failed.map((item) => ({ title: item.name, message: item.reason })),
          )
        } else {
          setSuccessToastMessage(
            t('autoUpdateRetrySuccess', { count: succeeded.length }),
          )
        }
      } finally {
        setAutoUpdateTriggering(false)
        setActionMessage(null)
      }
    },
    [
      autoUpdateTriggering,
      invokeTauri,
      isTauri,
      loadManagedSkills,
      managedSkills,
      showActionErrors,
      t,
    ],
  )
  const handleClearGitCacheNow = useCallback(async () => {
    if (!isTauri) {
      setError(t('errors.notTauri'))
      return
    }
    try {
      const removed = await invokeTauri<number>('clear_git_cache_now')
      setSuccessToastMessage(t('status.gitCacheCleared', { count: removed }))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [invokeTauri, isTauri, t])
  const handlePickLocalPath = useCallback(async () => {
    try {
      if (!isTauri) {
        throw new Error(t('errors.notTauri'))
      }
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('selectLocalFolder'),
      })
      if (!selected || Array.isArray(selected)) return
      setLocalPath(selected)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [isTauri, t])
  const pendingDeleteSkill = useMemo(
    () => managedSkills.find((skill) => skill.id === pendingDeleteId) ?? null,
    [managedSkills, pendingDeleteId],
  )
  const newlyInstalledToolsText = useMemo(() => {
    if (!toolStatus || toolStatus.newly_installed.length === 0) return ''
    return toolStatus.newly_installed
      .map((id) => tools.find((t) => t.id === id)?.label ?? id)
      .join('、')
  }, [toolStatus, tools])

  const runProjectDetailCommand = useCallback(
    async (
      command: string,
      args: Record<string, unknown>,
      successMessage?: string,
    ) => {
      setLoading(true)
      setLoadingStartAt(Date.now())
      try {
        const detail = await invokeTauri<ProjectDetailDto>(command, args)
        setSelectedProjectDetail(detail)
        setActiveView('projectDetail')
        await loadProjects()
        if (successMessage) toast.success(successMessage)
        return detail
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        return null
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [invokeTauri, loadProjects],
  )

  const handlePickProjectRoot = useCallback(async () => {
    try {
      if (!isTauri) throw new Error(t('errors.notTauri'))
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('projects.chooseFolder'),
      })
      if (!selected || Array.isArray(selected)) return
      setProjectImportPath(selected)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [isTauri, t])

  const handleImportProject = useCallback(() => {
    const projectPath = projectImportPath.trim()
    if (!projectPath) return
    void runProjectDetailCommand(
      'import_project',
      { projectPath },
      t('projects.imported'),
    ).then((detail) => {
      if (detail) setProjectImportPath('')
    })
  }, [projectImportPath, runProjectDetailCommand, t])

  const handleOpenProject = useCallback(
    (project: ProjectSummaryDto) => {
      void runProjectDetailCommand('scan_project', { projectId: project.id })
    },
    [runProjectDetailCommand],
  )

  const handleRefreshProject = useCallback(() => {
    if (!selectedProjectDetail) return
    void runProjectDetailCommand('scan_project', {
      projectId: selectedProjectDetail.project.id,
    }, t('projects.refreshed'))
  }, [runProjectDetailCommand, selectedProjectDetail, t])

  const handleProjectToolsChange = useCallback(
    (enabledToolKeys: string[]) => {
      if (!selectedProjectDetail) return
      void runProjectDetailCommand(
        'update_project_settings',
        {
          projectId: selectedProjectDetail.project.id,
          enabledToolKeys,
          localGitExcludeEnabled:
            selectedProjectDetail.project.local_git_exclude_enabled,
        },
        t('projects.settingsUpdated'),
      )
    },
    [runProjectDetailCommand, selectedProjectDetail, t],
  )

  const handleProjectGitExcludeChange = useCallback(
    (localGitExcludeEnabled: boolean) => {
      if (!selectedProjectDetail) return
      void runProjectDetailCommand(
        'update_project_settings',
        {
          projectId: selectedProjectDetail.project.id,
          enabledToolKeys: selectedProjectDetail.project.enabled_tool_keys,
          localGitExcludeEnabled,
        },
        t('projects.settingsUpdated'),
      )
    },
    [runProjectDetailCommand, selectedProjectDetail, t],
  )

  const getAvailableProjectTools = useCallback(() => {
    if (!selectedProjectDetail) return []
    return getAvailableProjectToolKeys(
      selectedProjectDetail.tools,
      selectedProjectDetail.project.enabled_tool_keys,
    )
  }, [selectedProjectDetail])

  const handleAddProjectLibrarySkill = useCallback(
    (skillId: string) => {
      if (!selectedProjectDetail) return
      if (getAvailableProjectTools().length === 0) {
        setError(t('projects.noEnabledAgents'))
        return
      }
      void runProjectDetailCommand(
        'inject_project_skill',
        {
          projectId: selectedProjectDetail.project.id,
          skillId,
        },
        t('projects.skillAdded'),
      ).then((detail) => {
        if (detail) void loadManagedSkills()
      })
    },
    [
      getAvailableProjectTools,
      loadManagedSkills,
      runProjectDetailCommand,
      selectedProjectDetail,
      t,
    ],
  )

  const handleAddProjectLibrarySkills = useCallback(
    (skillIds: string[]) => {
      if (!selectedProjectDetail || skillIds.length === 0) return
      if (skillIds.length === 1) {
        handleAddProjectLibrarySkill(skillIds[0])
        return
      }
      const availableTools = getAvailableProjectTools()
      if (availableTools.length === 0) {
        setError(t('projects.noEnabledAgents'))
        return
      }
      setAssignmentInitialDraft({
        global_tools: [],
        projects: [
          {
            project_id: selectedProjectDetail.project.id,
            tool_keys: availableTools,
          },
        ],
      })
      setProjectAssignmentBatch({ mode: 'library-add', skillIds })
    },
    [
      getAvailableProjectTools,
      handleAddProjectLibrarySkill,
      selectedProjectDetail,
      t,
    ],
  )

  const handleInjectProjectSkill = useCallback(
    (skillId: string) => {
      if (!selectedProjectDetail) return
      const skill = managedSkills.find((item) => item.id === skillId)
      if (!skill) {
        setError(t('errors.skillNotFound'))
        return
      }
      const availableTools = getAvailableProjectTools()
      setAssignmentInitialDraft(
        assignmentDraftForSkill(
          skill,
          selectedProjectDetail.project.id,
          availableTools,
        ),
      )
      setScopeModalSkill(skill)
    },
    [getAvailableProjectTools, managedSkills, selectedProjectDetail, t],
  )

  const handleAssignCurrentProjectSkills = useCallback(
    (skillIds: string[]) => {
      if (!selectedProjectDetail || skillIds.length === 0) return
      const availableTools = getAvailableProjectTools()
      if (availableTools.length === 0) {
        setError(t('projects.noEnabledAgents'))
        return
      }
      setAssignmentInitialDraft({
        global_tools: [],
        projects: [
          {
            project_id: selectedProjectDetail.project.id,
            tool_keys: availableTools,
          },
        ],
      })
      setProjectAssignmentBatch({ mode: 'current-tools', skillIds })
    },
    [getAvailableProjectTools, selectedProjectDetail, t],
  )

  const handleRemoveProjectSkill = useCallback(
    (skillId: string) => {
      if (!selectedProjectDetail) return
      const skill = managedSkills.find((item) => item.id === skillId)
      if (!skill) {
        setError(t('errors.skillNotFound'))
        return
      }
      setAssignmentInitialDraft(
        assignmentDraftForSkill(skill, selectedProjectDetail.project.id, null),
      )
      setScopeModalSkill(skill)
    },
    [managedSkills, selectedProjectDetail, t],
  )

  const handleRemoveProjectSkills = useCallback(
    async (skillIds: string[]) => {
      if (!selectedProjectDetail || skillIds.length === 0) return
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      const collectedErrors: { title: string; message: string }[] = []
      let latestDetail: ProjectDetailDto | null = null
      try {
        for (let index = 0; index < skillIds.length; index++) {
          const skillId = skillIds[index]
          const skill = managedSkills.find((item) => item.id === skillId)
          setActionMessage(
            t('projects.bulkRemoveProgress', {
              current: index + 1,
              total: skillIds.length,
              name: skill?.name ?? skillId,
            }),
          )
          try {
            latestDetail = await invokeTauri<ProjectDetailDto>(
              'remove_project_skill',
              {
                projectId: selectedProjectDetail.project.id,
                skillId,
              },
            )
          } catch (err) {
            collectedErrors.push({
              title: t('projects.bulkRemoveFailed', {
                name: skill?.name ?? skillId,
              }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }
        if (latestDetail) setSelectedProjectDetail(latestDetail)
        await loadManagedSkills()
        await loadProjects()
        if (collectedErrors.length > 0) {
          showActionErrors(collectedErrors)
        } else {
          setSuccessToastMessage(
            t('projects.bulkRemoved', { count: skillIds.length }),
          )
        }
      } finally {
        setActionMessage(null)
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      invokeTauri,
      loadManagedSkills,
      loadProjects,
      managedSkills,
      selectedProjectDetail,
      showActionErrors,
      t,
    ],
  )

  const handleRepairProjectIssue = useCallback(
    (issueId: string, action: ProjectIssueActionDto) => {
      if (!selectedProjectDetail) return
      void runProjectDetailCommand(
        'repair_project_issue',
        {
          projectId: selectedProjectDetail.project.id,
          issueId,
          actionKind: action.kind,
          actionValue: action.value ?? null,
        },
        t('projects.issueRepaired'),
      )
    },
    [runProjectDetailCommand, selectedProjectDetail, t],
  )

  const handleIgnoreProjectIssue = useCallback(
    (issueId: string, fingerprint: string) => {
      if (!selectedProjectDetail) return
      void runProjectDetailCommand(
        'ignore_project_issue',
        {
          projectId: selectedProjectDetail.project.id,
          issueId,
          fingerprint,
        },
        t('projects.issueIgnored'),
      )
    },
    [runProjectDetailCommand, selectedProjectDetail, t],
  )

  const handleIgnoreAllProjectIssues = useCallback(() => {
    if (!selectedProjectDetail) return
    void runProjectDetailCommand(
      'ignore_all_project_issues',
      { projectId: selectedProjectDetail.project.id },
      t('projects.issuesIgnored'),
    )
  }, [runProjectDetailCommand, selectedProjectDetail, t])

  const handleCollectProjectSkill = useCallback(
    async (discoveredSkillId: string, name: string) => {
      if (!selectedProjectDetail) return
      setLoading(true)
      setLoadingStartAt(Date.now())
      const projectId = selectedProjectDetail.project.id
      const collect = (libraryName?: string) =>
        invokeTauri<ProjectDetailDto>('collect_project_skill', {
          projectId,
          discoveredSkillId,
          libraryName,
        })
      try {
        let detail: ProjectDetailDto
        try {
          detail = await collect()
        } catch (err) {
          const raw = err instanceof Error ? err.message : String(err)
          if (!raw.startsWith('PROJECT_LIBRARY_NAME_REQUIRED|')) throw err
          const libraryName = window.prompt(
            t('projects.libraryNamePrompt', { name }),
            `${name}-project`,
          )
          if (!libraryName?.trim()) return
          detail = await collect(libraryName.trim())
        }
        setSelectedProjectDetail(detail)
        await Promise.all([loadProjects(), loadManagedSkills()])
        toast.success(t('projects.collected'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [invokeTauri, loadManagedSkills, loadProjects, selectedProjectDetail, t],
  )

  const handleBackToProjects = useCallback(() => {
    setSelectedProjectDetail(null)
    setActiveView('projects')
    void loadProjects()
  }, [loadProjects])

  const handleOpenSettings = useCallback(() => {
    setActiveView('settings')
  }, [])

  const loadFeaturedSkills = useCallback(async () => {
    if (featuredSkills.length > 0) return
    setFeaturedLoading(true)
    try {
      const result = await invokeTauri<FeaturedSkillDto[]>('get_featured_skills')
      setFeaturedSkills(result)
    } catch {
      // silent — explore tab will show empty state
    } finally {
      setFeaturedLoading(false)
    }
  }, [featuredSkills.length, invokeTauri])

  const handleViewChange = useCallback(
    (view: 'projects' | 'myskills' | 'explore' | 'manage') => {
      setActiveView(view)
      if (view !== 'myskills') {
        setBulkMode(false)
        setBulkSelectedIds([])
        setShowBulkDeleteModal(false)
        setShowBulkTagsModal(false)
      }
      if (view === 'explore') {
        loadFeaturedSkills()
      }
      if (view === 'manage') {
        setManagementTab('tags')
      }
      if (view === 'myskills') {
        setDetailSkill(null)
      }
      if (view === 'projects') {
        setSelectedProjectDetail(null)
        void loadProjects()
      }
    },
    [loadFeaturedSkills, loadProjects],
  )

  const handleOpenDetail = useCallback((skill: ManagedSkill) => {
    setBulkMode(false)
    setBulkSelectedIds([])
    setDetailSkill(skill)
    setActiveView('detail')
  }, [])

  const handleBackToList = useCallback(() => {
    setDetailSkill(null)
    setActiveView('myskills')
  }, [])

  const handleExploreFilterChange = useCallback(
    (value: string) => {
      setExploreFilter(value)
      if (searchTimerRef.current) {
        clearTimeout(searchTimerRef.current)
        searchTimerRef.current = null
      }
      if (value.trim().length < 2) {
        setSearchResults([])
        setSearchLoading(false)
        return
      }
      setSearchLoading(true)
      searchTimerRef.current = setTimeout(async () => {
        try {
          const results = await invokeTauri<OnlineSkillDto[]>(
            'search_skills_online',
            { query: value.trim(), limit: 20 },
          )
          setSearchResults(results)
        } catch {
          toast.error(t('searchError'))
          setSearchResults([])
        } finally {
          setSearchLoading(false)
        }
      }, 500)
    },
    [invokeTauri, t],
  )


  const handleOpenAdd = useCallback(() => {
    setReimportSkill(null)
    setShowAddModal(true)
    setAddModalTagIds([])
  }, [])

  const applySelectedAddModalTags = useCallback(
    async (skillId: string, skillName: string) => {
      if (addModalTagIds.length === 0) return
      try {
        await invokeTauri('set_skill_tags', {
          skillId,
          tagIds: addModalTagIds,
        })
      } catch {
        toast.error(t('tagsApplyFailed', { name: skillName }))
      }
    },
    [addModalTagIds, invokeTauri, t],
  )

  const handleCancelLoading = useCallback(() => {
    void invokeTauri('cancel_current_operation').catch(() => {})
    setLoading(false)
    setLoadingStartAt(null)
    setActionMessage(null)
  }, [invokeTauri])

  const handleCloseAdd = useCallback(() => {
    if (!loading) {
      setShowAddModal(false)
      setReimportSkill(null)
      setAddModalTagIds([])
    }
  }, [loading])

  const handleCloseImport = useCallback(() => {
    if (!loading) setShowImportModal(false)
  }, [loading])

  const handleCloseSettings = useCallback(() => {
    setActiveView('projects')
  }, [])

  const handleThemeChange = useCallback(
    (nextTheme: 'system' | 'light' | 'dark') => {
      setThemePreference(nextTheme)
    },
    [],
  )

  const handleCloseNewTools = useCallback(() => {
    if (!loading) setShowNewToolsModal(false)
  }, [loading])

  const handleCloseDelete = useCallback(() => {
    if (!loading) setPendingDeleteId(null)
  }, [loading])

  const handleCloseGitPick = useCallback(() => {
    if (!loading) {
      setShowGitPickModal(false)
      setReimportSkill(null)
    }
  }, [loading])

  const handleCancelGitPick = useCallback(() => {
    if (loading) return
    setShowGitPickModal(false)
    setGitCandidates([])
    setGitCandidateSelected({})
    setGitCandidatesRepoUrl('')
    setShowAddModal(true)
  }, [loading])

  const handleCloseLocalPick = useCallback(() => {
    if (!loading) {
      setShowLocalPickModal(false)
      setReimportSkill(null)
    }
  }, [loading])

  const handleCancelLocalPick = useCallback(() => {
    if (loading) return
    setShowLocalPickModal(false)
    setLocalCandidates([])
    setLocalCandidateSelected({})
    setLocalCandidatesBasePath('')
    setShowAddModal(true)
  }, [loading])

  const handleSortChange = useCallback((value: 'updated' | 'name') => {
    setSortBy(value)
  }, [])

  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value)
  }, [])

  const handleScopeFilterChange = useCallback(
    (value: 'all' | 'global' | 'project') => {
      setScopeFilter(value)
    },
    [],
  )

  const handleToggleAddModalTag = useCallback((tagId: number) => {
    setAddModalTagIds((current) =>
      current.includes(tagId)
        ? current.filter((id) => id !== tagId)
        : [...current, tagId],
    )
  }, [])

  const handleToggleTagFilter = useCallback((tagId: number) => {
    setSelectedTagIds((current) =>
      current.includes(tagId)
        ? current.filter((id) => id !== tagId)
        : [...current, tagId],
    )
  }, [])

  const handleToggleUntaggedFilter = useCallback(() => {
    setIncludeUntagged((current) => !current)
  }, [])

  const handleClearTagFilters = useCallback(() => {
    setSelectedTagIds([])
    setIncludeUntagged(false)
  }, [])

  const handleOpenTagsPage = useCallback(() => {
    setManagementTab('tags')
    setActiveView('manage')
  }, [])

  const handleReviewUntagged = useCallback(() => {
    setSelectedTagIds([])
    setIncludeUntagged(true)
    setActiveView('myskills')
  }, [])

  const handleViewTag = useCallback((tagId: number) => {
    setSelectedTagIds([tagId])
    setIncludeUntagged(false)
    setActiveView('myskills')
  }, [])

  const handleToggleBulkMode = useCallback(() => {
    setBulkMode((current) => {
      const next = !current
      if (!next) {
        setBulkSelectedIds([])
        setShowBulkDeleteModal(false)
        setShowBulkTagsModal(false)
      }
      return next
    })
  }, [])

  const handleToggleBulkSelection = useCallback((skillId: string) => {
    setBulkSelectedIds((current) =>
      current.includes(skillId)
        ? current.filter((id) => id !== skillId)
        : [...current, skillId],
    )
  }, [])

  const handleSelectVisibleSkills = useCallback(() => {
    setBulkMode(true)
    setBulkSelectedIds((current) => {
      const visibleIds = visibleSkills.map((skill) => skill.id)
      const visibleSet = new Set(visibleIds)
      const allVisibleSelected =
        visibleIds.length > 0 && visibleIds.every((id) => current.includes(id))
      if (allVisibleSelected) {
        return current.filter((id) => !visibleSet.has(id))
      }
      return Array.from(new Set([...current, ...visibleIds]))
    })
  }, [visibleSkills])

  const handleSelectEntireLibrary = useCallback(() => {
    setBulkMode(true)
    setBulkSelectedIds((current) =>
      selectEntireLibrary(
        current,
        managedSkills.map((skill) => skill.id),
      ),
    )
  }, [managedSkills])

  const handleOpenBulkDelete = useCallback(() => {
    if (bulkSelectedIds.length === 0) return
    setShowBulkDeleteModal(true)
  }, [bulkSelectedIds.length])

  const handleCloseBulkDelete = useCallback(() => {
    if (!loading) setShowBulkDeleteModal(false)
  }, [loading])

  const handleOpenBulkTags = useCallback(() => {
    if (bulkSelectedIds.length === 0) return
    setShowBulkTagsModal(true)
  }, [bulkSelectedIds.length])

  const handleCloseBulkTags = useCallback(() => {
    if (!loading) setShowBulkTagsModal(false)
  }, [loading])

  const handleConfirmBulkTags = useCallback(
    async (addTagIds: number[], removeTagIds: number[]) => {
      if (bulkSelectedSkills.length === 0) return
      if (addTagIds.length === 0 && removeTagIds.length === 0) return

      const addSet = new Set(addTagIds)
      const removeSet = new Set(removeTagIds)
      const errors: { title: string; message: string }[] = []
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        for (let index = 0; index < bulkSelectedSkills.length; index++) {
          const skill = bulkSelectedSkills[index]
          setActionMessage(
            t('bulk.tagsProgress', {
              current: index + 1,
              total: bulkSelectedSkills.length,
              name: skill.name,
            }),
          )
          const nextTagIds = new Set<number>()
          for (const tag of skill.tags) {
            if (!removeSet.has(tag.id)) nextTagIds.add(tag.id)
          }
          for (const tagId of addSet) nextTagIds.add(tagId)

          try {
            await invokeTauri('set_skill_tags', {
              skillId: skill.id,
              tagIds: Array.from(nextTagIds),
            })
          } catch (err) {
            errors.push({
              title: t('bulk.tagsFailedTitle', { name: skill.name }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }

        await loadManagedSkills()
        await loadTags()
        setShowBulkTagsModal(false)
        if (errors.length > 0) {
          showActionErrors(errors)
        } else {
          setBulkSelectedIds([])
          setBulkMode(false)
          setSuccessToastMessage(
            t('bulk.tagsSuccess', { count: bulkSelectedSkills.length }),
          )
        }
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [
      bulkSelectedSkills,
      invokeTauri,
      loadManagedSkills,
      loadTags,
      showActionErrors,
      t,
    ],
  )

  const restoreSkillSavedTargets = useCallback(
    async (skill: ManagedSkill) => {
      await invokeTauri('resume_skill_assignments', { skillId: skill.id })
    },
    [invokeTauri],
  )

  const handleToggleSkillEnabled = useCallback(
    async (skill: ManagedSkill) => {
      const nextEnabled = skill.enabled === false
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      setActionMessage(
        nextEnabled
          ? t('bulk.enableProgress', { current: 1, total: 1, name: skill.name })
          : t('bulk.disableProgress', { current: 1, total: 1, name: skill.name }),
      )
      try {
        await invokeTauri('set_skill_enabled', {
          skillId: skill.id,
          enabled: nextEnabled,
        })
        if (nextEnabled) {
          await restoreSkillSavedTargets(skill)
        }
        await loadManagedSkills()
        setSuccessToastMessage(
          nextEnabled
            ? t('skillEnabled', { name: skill.name })
            : t('skillDisabled', { name: skill.name }),
        )
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [invokeTauri, loadManagedSkills, restoreSkillSavedTargets, t],
  )

  const handleToggleBulkEnabled = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return
    const nextEnabled = bulkShouldEnable
    const affectedSkills = bulkSelectedSkills.filter(
      (skill) => (skill.enabled !== false) !== nextEnabled,
    )
    if (affectedSkills.length === 0) return

    const errors: { title: string; message: string }[] = []
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      for (let index = 0; index < affectedSkills.length; index++) {
        const skill = affectedSkills[index]
        setActionMessage(
          nextEnabled
            ? t('bulk.enableProgress', {
                current: index + 1,
                total: affectedSkills.length,
                name: skill.name,
              })
            : t('bulk.disableProgress', {
                current: index + 1,
                total: affectedSkills.length,
                name: skill.name,
              }),
        )
        try {
          await invokeTauri('set_skill_enabled', {
            skillId: skill.id,
            enabled: nextEnabled,
          })
          if (nextEnabled) {
            await restoreSkillSavedTargets(skill)
          }
        } catch (err) {
          errors.push({
            title: nextEnabled
              ? t('bulk.enableFailedTitle', { name: skill.name })
              : t('bulk.disableFailedTitle', { name: skill.name }),
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }
      await loadManagedSkills()
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(
          nextEnabled
            ? t('bulk.enableSuccess', { count: affectedSkills.length })
            : t('bulk.disableSuccess', { count: affectedSkills.length }),
        )
      }
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills,
    bulkShouldEnable,
    invokeTauri,
    loadManagedSkills,
    restoreSkillSavedTargets,
    showActionErrors,
    t,
  ])

  const handleConfirmBulkDelete = useCallback(async () => {
    if (bulkSelectedSkills.length === 0) return

    const errors: { title: string; message: string }[] = []
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      for (let index = 0; index < bulkSelectedSkills.length; index++) {
        const skill = bulkSelectedSkills[index]
        setActionMessage(
          t('bulk.deleteProgress', {
            current: index + 1,
            total: bulkSelectedSkills.length,
            name: skill.name,
          }),
        )
        try {
          await invokeTauri('delete_managed_skill', { skillId: skill.id })
        } catch (err) {
          errors.push({
            title: skill.name,
            message: err instanceof Error ? err.message : String(err),
          })
        }
      }
      await loadManagedSkills()
      await loadTags()
      setShowBulkDeleteModal(false)
      if (errors.length > 0) {
        showActionErrors(errors)
      } else {
        setBulkSelectedIds([])
        setBulkMode(false)
        setSuccessToastMessage(
          t('bulk.deleteSuccess', { count: bulkSelectedSkills.length }),
        )
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [
    bulkSelectedSkills,
    invokeTauri,
    loadManagedSkills,
    loadTags,
    showActionErrors,
    t,
  ])

  const handleCreateTag = useCallback(
    async (name: string) => {
      try {
        await invokeTauri('create_tag', { name })
        await loadTags()
        setSuccessToastMessage(t('tagCreated'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, loadTags, t],
  )

  const handleRenameTag = useCallback(
    async (tagId: number, name: string) => {
      try {
        const renamed = await invokeTauri<{ id: number; name: string }>('rename_tag', {
          tagId,
          name,
        })
        setSelectedTagIds((current) =>
          current.includes(tagId) ? current.map((id) => (id === tagId ? renamed.id : id)) : current,
        )
        await loadManagedSkills()
        await loadTags()
        setSuccessToastMessage(t('tagRenamed'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      }
    },
    [invokeTauri, loadManagedSkills, loadTags, t],
  )

  const handleDeleteTag = useCallback((tag: TagWithCountDto) => {
    setPendingDeleteTag(tag)
  }, [])

  const handleCloseDeleteTag = useCallback(() => {
    if (!loading) setPendingDeleteTag(null)
  }, [loading])

  const handleConfirmDeleteTag = useCallback(async () => {
    if (!pendingDeleteTag) return
    try {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setActionMessage(t('actions.deletingTag', { name: pendingDeleteTag.name }))
      await invokeTauri('delete_tag', { tagId: pendingDeleteTag.id })
      setSelectedTagIds((current) => current.filter((id) => id !== pendingDeleteTag.id))
      await loadManagedSkills()
      await loadTags()
      setPendingDeleteTag(null)
      setSuccessToastMessage(t('tagDeleted'))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
      setActionMessage(null)
    }
  }, [invokeTauri, loadManagedSkills, loadTags, pendingDeleteTag, t])

  const handleOpenEditTags = useCallback((skill: ManagedSkill) => {
    setTagEditorSkill(skill)
  }, [])

  const handleCloseEditTags = useCallback(() => {
    if (!loading) setTagEditorSkill(null)
  }, [loading])

  const handleSaveSkillTags = useCallback(
    async (skill: ManagedSkill, tagIds: number[]) => {
      try {
        setLoading(true)
        setLoadingStartAt(Date.now())
        setActionMessage(t('actions.updatingTags', { name: skill.name }))
        await invokeTauri('set_skill_tags', { skillId: skill.id, tagIds })
        await loadManagedSkills()
        await loadTags()
        setTagEditorSkill(null)
        setSuccessToastMessage(t('tagsUpdated'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [invokeTauri, loadManagedSkills, loadTags, t],
  )

  const handleDeletePrompt = useCallback((skillId: string) => {
    setPendingDeleteId(skillId)
  }, [])

  const handleToggleGitCandidate = useCallback(
    (subpath: string, checked: boolean) => {
      setGitCandidateSelected((prev) =>
        reimportSkill
          ? Object.fromEntries(
              Object.keys(prev).map((key) => [key, checked && key === subpath]),
            )
          : { ...prev, [subpath]: checked },
      )
    },
    [reimportSkill],
  )

  const handleToggleLocalCandidate = useCallback(
    (subpath: string, checked: boolean) => {
      setLocalCandidateSelected((prev) =>
        reimportSkill
          ? Object.fromEntries(
              Object.keys(prev).map((key) => [key, checked && key === subpath]),
            )
          : { ...prev, [subpath]: checked },
      )
    },
    [reimportSkill],
  )

  const handleToggleGroup = useCallback((groupName: string, checked: boolean) => {
    setSelected((prev) => ({
      ...prev,
      [groupName]: checked,
    }))
  }, [])

  const handleSelectVariant = useCallback((groupName: string, path: string) => {
    setVariantChoice((prev) => ({
      ...prev,
      [groupName]: path,
    }))
  }, [])

  const handleResolveHubConflict = useCallback(
    (groupName: string, resolution: DiscoveredHubResolution) => {
      setHubResolutions((current) => ({
        ...current,
        [groupName]: resolution,
      }))
    },
    [],
  )

  const handleReviewImport = useCallback(async () => {
    if (plan) {
      setShowImportModal(true)
      return
    }
    const result = await loadPlan()
    if (result) {
      setShowImportModal(true)
    }
  }, [loadPlan, plan])

  useEffect(() => {
    const load = async () => {
      if (!isTauri) return
      try {
        const status = await invokeTauri<ToolStatusDto>('get_tool_status')
        setToolStatus(status)

        if (status.newly_installed.length > 0) {
          setShowNewToolsModal(true)
        }
      } catch (err) {
        // Non-fatal; app can still work without detection.
        console.warn(err)
      }
    }
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isTauri])

  const handleImport = async () => {
    if (!plan) return
    const importJobs = plan.groups
      .filter((group) => selected[group.name])
      .map((group) => ({
        group,
        decision: getDiscoveredImportDecision(
          group,
          variantChoice[group.name],
          hubResolutions[group.name],
        ),
      }))
    if (importJobs.length === 0) {
      setError(t('errors.selectAtLeastOneSkill'))
      return
    }
    if (importJobs.some((job) => job.decision.unresolvedHubConflict)) {
      setError(t('errors.resolveHubConflict'))
      return
    }
    const renamedCollision = importJobs.find(
      (job) => job.decision.renamed && isSkillNameTaken(job.decision.importName),
    )
    if (renamedCollision) {
      setError(
        t('errors.skillAlreadyExists', {
          name: renamedCollision.decision.importName,
        }),
      )
      return
    }
    const importNames = new Set<string>()
    const duplicateImportName = importJobs.find((job) => {
      const normalized = job.decision.importName.toLowerCase()
      if (importNames.has(normalized)) return true
      importNames.add(normalized)
      return false
    })
    if (duplicateImportName) {
      setError(
        t('errors.duplicateSelectedSkills', {
          name: duplicateImportName.decision.importName,
        }),
      )
      return
    }
    setLoading(true)
    setLoadingStartAt(Date.now())
    setActionMessage(null)
    setError(null)
    try {
      const collectedErrors: { title: string; message: string }[] = []
      let successCount = 0
      for (const { group, decision } of importJobs) {
        const chosenPath = decision.variant?.path
        if (!chosenPath) continue
        const chosenVariant = decision.variant
        const chosenVariantTool = chosenVariant?.tool ?? null
        const chosenFingerprint = chosenVariant?.fingerprint ?? null

        let installResult: InstallResultDto

        try {
          setActionMessage(
            t('actions.importExisting', { name: decision.importName }),
          )
          installResult = await invokeTauri<InstallResultDto>('import_existing_skill', {
            sourcePath: chosenPath,
            name: decision.importName,
            conflictResolution: decision.conflictResolution ?? null,
          })
          highlightSkill(installResult.skill_id)
          successCount += 1
        } catch (err) {
          collectedErrors.push({
            title: t('errors.importFailedTitle', { name: group.name }),
            message: err instanceof Error ? err.message : String(err),
          })
          continue
        }

        const inheritedTools = decision.renamed
          ? []
          : Array.from(
              new Set(
                group.variants
                  .filter((variant) =>
                    chosenFingerprint
                      ? variant.fingerprint === chosenFingerprint
                      : variant.tool === chosenVariantTool,
                  )
                  .map((variant) => variant.tool)
                  .filter((toolId) => isInstalled(toolId)),
              ),
            )
        if (inheritedTools.length > 0) {
          try {
            const draft: AssignmentDraftDto = {
              global_tools: inheritedTools,
              projects: [],
            }
            const preview = await invokeTauri<AssignmentPreviewDto>(
              'preview_skill_assignments',
              { skillIds: [installResult.skill_id], draft, additive: true },
            )
            const replaceConflictKeys = preview.items
              .filter((item) => item.outcome === 'conflict')
              .map((item) => item.key)
            const applied = await invokeTauri<AssignmentApplyResultDto>(
              'apply_skill_assignments',
              {
                skillIds: [installResult.skill_id],
                draft,
                replaceConflictKeys,
                additive: true,
              },
            )
            for (const skill of applied.skills) {
              for (const message of skill.errors) {
                collectedErrors.push({
                  title: t('assignment.applyFailedTitle', { name: skill.skill_name }),
                  message,
                })
              }
            }
          } catch (err) {
            collectedErrors.push({
              title: t('assignment.applyFailedTitle', { name: installResult.name }),
              message: err instanceof Error ? err.message : String(err),
            })
          }
        }
      }

      setActionMessage(t('status.importCompleted'))
      setActionMessage(null)
      await loadManagedSkills()
      await loadPlan()
      if (collectedErrors.length > 0) {
        showActionErrors(collectedErrors)
      } else if (successCount > 0) {
        setSuccessToastMessage(t('status.importCompleted'))
      }
      setShowImportModal(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const handleCreateLocal = async () => {
    if (!localPath.trim()) {
      setError(t('errors.requireLocalPath'))
      return
    }
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    setActionMessage(
      reimportSkill
        ? t('actions.reimporting', { name: reimportSkill.name })
        : t('actions.creatingLocalSkill'),
    )
    try {
      const basePath = localPath.trim()
      const candidates = await invokeTauri<LocalSkillCandidate[]>(
        'list_local_skills_cmd',
        { basePath },
      )
      if (candidates.length === 0) {
        throw new Error(t('errors.noSkillsFoundLocal'))
      }
      if (candidates.length === 1 && candidates[0].valid) {
        const desiredName = localName.trim() || candidates[0].name
        if (!reimportSkill && isSkillNameTaken(desiredName)) {
          setError(t('errors.skillAlreadyExists', { name: desiredName }))
          return
        }
        const created = reimportSkill
          ? await invokeTauri<InstallResultDto>('reimport_local_selection', {
              skillId: reimportSkill.id,
              basePath,
              subpath: candidates[0].subpath,
            })
          : await invokeTauri<InstallResultDto>('install_local_selection', {
              basePath,
              subpath: candidates[0].subpath,
              name: localName.trim() || undefined,
            })
        if (!reimportSkill) {
          await applySelectedAddModalTags(created.skill_id, created.name)
        }
        highlightSkill(created.skill_id)
        setLocalPath('')
        setLocalName('')
        const successMessage = reimportSkill
          ? t('status.skillReimported', { name: created.name })
          : t('status.localSkillCreated')
        setActionMessage(successMessage)
        setSuccessToastMessage(successMessage)
        if (!reimportSkill) setSkillListMode('pending')
        setActionMessage(null)
        setShowAddModal(false)
        setReimportSkill(null)
        await loadManagedSkills()
        await loadTags()
      } else {
        const matchingCandidate = reimportSkill
          ? candidates.find(
              (candidate) =>
                candidate.valid &&
                candidate.name.toLowerCase() === reimportSkill.name.toLowerCase(),
            )
          : null
        setLocalCandidatesBasePath(basePath)
        setLocalCandidates(candidates)
        setLocalCandidateSelected(
          Object.fromEntries(
            candidates.map((candidate) => [
              candidate.subpath,
              reimportSkill
                ? candidate.subpath === matchingCandidate?.subpath
                : candidate.valid,
            ]),
          ),
        )
        setShowAddModal(false)
        setShowLocalPickModal(true)
        setActionMessage(null)
        setLoading(false)
        setLoadingStartAt(null)
        return
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const handleCreateGit = async () => {
    if (!gitUrl.trim()) {
      setError(t('errors.requireGitUrl'))
      return
    }
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    setActionMessage(
      reimportSkill
        ? t('actions.reimporting', { name: reimportSkill.name })
        : t('actions.creatingGitSkill'),
    )
    try {
      const url = gitUrl.trim()
      const isFolderUrl = url.includes('/tree/') || url.includes('/blob/')

      if (isFolderUrl) {
        const candidates = await invokeTauri<GitSkillCandidate[]>(
          'list_git_skills_cmd',
          { repoUrl: url },
        )
        if (candidates.length === 0) {
          throw new Error(t('errors.noSkillsFoundWithHint'))
        }
        if (candidates.length > 1) {
          const matchingCandidate = reimportSkill
            ? candidates.find(
                (candidate) =>
                  candidate.name.toLowerCase() === reimportSkill.name.toLowerCase(),
              )
            : null
          setGitCandidatesRepoUrl(url)
          setGitCandidates(candidates)
          setGitCandidateSelected(
            Object.fromEntries(
              candidates.map((candidate) => [
                candidate.subpath,
                reimportSkill
                  ? candidate.subpath === matchingCandidate?.subpath
                  : true,
              ]),
            ),
          )
          setShowAddModal(false)
          setShowGitPickModal(true)
          setActionMessage(null)
          setLoading(false)
          setLoadingStartAt(null)
          return
        }
        if (!reimportSkill && isSkillNameTaken(candidates[0].name)) {
          setError(t('errors.skillAlreadyExists', { name: candidates[0].name }))
          return
        }
        const created = reimportSkill
          ? await invokeTauri<InstallResultDto>('reimport_git_selection', {
              skillId: reimportSkill.id,
              repoUrl: url,
              subpath: candidates[0].subpath,
            })
          : await invokeTauri<InstallResultDto>('install_git_selection', {
              repoUrl: url,
              subpath: candidates[0].subpath,
              name: gitName.trim() || undefined,
            })
        if (!reimportSkill) {
          await applySelectedAddModalTags(created.skill_id, created.name)
        }
        highlightSkill(created.skill_id)
      } else {
        const candidates = await invokeTauri<GitSkillCandidate[]>(
          'list_git_skills_cmd',
          { repoUrl: url },
        )
        if (candidates.length === 0) {
          throw new Error(t('errors.noSkillsFoundWithHint'))
        }
        if (candidates.length === 1) {
          if (!reimportSkill && isSkillNameTaken(candidates[0].name)) {
            setError(t('errors.skillAlreadyExists', { name: candidates[0].name }))
            return
          }
          const created = reimportSkill
            ? await invokeTauri<InstallResultDto>('reimport_git_selection', {
                skillId: reimportSkill.id,
                repoUrl: url,
                subpath: candidates[0].subpath,
              })
            : await invokeTauri<InstallResultDto>('install_git_selection', {
                repoUrl: url,
                subpath: candidates[0].subpath,
                name: gitName.trim() || undefined,
              })
          if (!reimportSkill) {
            await applySelectedAddModalTags(created.skill_id, created.name)
          }
          highlightSkill(created.skill_id)
        } else if (autoSelectSkillName) {
          // Auto-select the matching skill from online search results.
          // skills.sh name may differ from SKILL.md name (e.g. "json-render-react" vs "react"),
          // so try exact match first, then containment match.
          const target = autoSelectSkillName.toLowerCase()
          const containMatches = candidates.filter((c) => {
            const n = c.name.toLowerCase()
            return target.includes(n) || n.includes(target)
          })
          const match =
            candidates.find((c) => c.name.toLowerCase() === target) ??
            (containMatches.length === 1 ? containMatches[0] : undefined)
          setAutoSelectSkillName(null)
          if (match) {
            if (isSkillNameTaken(match.name)) {
              setError(t('errors.skillAlreadyExists', { name: match.name }))
              return
            }
            const created = await invokeTauri<InstallResultDto>(
              'install_git_selection',
              {
                repoUrl: url,
                subpath: match.subpath,
                name: gitName.trim() || undefined,
              },
            )
            await applySelectedAddModalTags(created.skill_id, created.name)
            highlightSkill(created.skill_id)
          } else {
            // No match found, fall back to picker
            setGitCandidatesRepoUrl(url)
            setGitCandidates(candidates)
            setGitCandidateSelected(
              Object.fromEntries(candidates.map((c) => [c.subpath, true])),
            )
            setShowAddModal(false)
            setShowGitPickModal(true)
            setActionMessage(null)
            setLoading(false)
            setLoadingStartAt(null)
            return
          }
        } else {
          const matchingCandidate = reimportSkill
            ? candidates.find(
                (candidate) =>
                  candidate.name.toLowerCase() === reimportSkill.name.toLowerCase(),
              )
            : null
          setGitCandidatesRepoUrl(url)
          setGitCandidates(candidates)
          setGitCandidateSelected(
            Object.fromEntries(
              candidates.map((candidate) => [
                candidate.subpath,
                reimportSkill
                  ? candidate.subpath === matchingCandidate?.subpath
                  : true,
              ]),
            ),
          )
          setShowAddModal(false)
          setShowGitPickModal(true)
          setActionMessage(null)
          setLoading(false)
          setLoadingStartAt(null)
          return
        }
      }
      setGitUrl('')
      setGitName('')
      const successMessage = reimportSkill
        ? t('status.skillReimported', { name: reimportSkill.name })
        : t('status.gitSkillCreated')
      setActionMessage(successMessage)
      setSuccessToastMessage(successMessage)
      if (!reimportSkill) setSkillListMode('pending')
      setActionMessage(null)
      setShowAddModal(false)
      setReimportSkill(null)
      await loadManagedSkills()
      await loadTags()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const [exploreInstallTrigger, setExploreInstallTrigger] = useState(0)
  const exploreInstallUrlRef = useRef<string | null>(null)

  const handleExploreInstall = useCallback(
    (sourceUrl: string, skillName?: string) => {
      setGitUrl(sourceUrl)
      if (skillName) setAutoSelectSkillName(skillName)
      exploreInstallUrlRef.current = sourceUrl
      setExploreInstallTrigger((n) => n + 1)
    },
    [],
  )

  useEffect(() => {
    if (exploreInstallTrigger > 0 && exploreInstallUrlRef.current && !loading) {
      exploreInstallUrlRef.current = null
      void handleCreateGit()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exploreInstallTrigger])

  const handleInstallSelectedLocalCandidates = async () => {
    const selected = localCandidates.filter(
      (c) => c.valid && localCandidateSelected[c.subpath],
    )
    if (selected.length === 0) {
      setError(t('errors.selectAtLeastOneSkill'))
      return
    }
    if (reimportSkill) {
      if (selected.length !== 1) {
        setError(t('errors.selectOneSkillToReimport'))
        return
      }
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      setActionMessage(t('actions.reimporting', { name: reimportSkill.name }))
      try {
        const created = await invokeTauri<InstallResultDto>(
          'reimport_local_selection',
          {
            skillId: reimportSkill.id,
            basePath: localCandidatesBasePath,
            subpath: selected[0].subpath,
          },
        )
        highlightSkill(created.skill_id)
        setShowLocalPickModal(false)
        setLocalCandidates([])
        setLocalCandidateSelected({})
        setLocalCandidatesBasePath('')
        setLocalPath('')
        setLocalName('')
        setReimportSkill(null)
        setSuccessToastMessage(
          t('status.skillReimported', { name: created.name }),
        )
        setActionMessage(null)
        await loadManagedSkills()
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
      return
    }
    if (selected.length > 1 && localName.trim()) {
      setError(t('errors.multiSelectNoCustomName'))
      return
    }
    if (selected.length > 1) {
      const seen = new Set<string>()
      const dup = selected.find((c) => {
        if (seen.has(c.name)) return true
        seen.add(c.name)
        return false
      })
      if (dup) {
        setError(t('errors.duplicateSelectedSkills', { name: dup.name }))
        return
      }
    }
    const desiredName =
      selected.length === 1 && localName.trim()
        ? localName.trim()
        : selected[0].name
    if (selected.length === 1 && isSkillNameTaken(desiredName)) {
      setError(t('errors.skillAlreadyExists', { name: desiredName }))
      return
    }
    const duplicated = selected.find((c) => isSkillNameTaken(c.name))
    if (selected.length > 1 && duplicated) {
      setError(t('errors.skillAlreadyExists', { name: duplicated.name }))
      return
    }

    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      const collectedErrors: { title: string; message: string }[] = []
      for (let i = 0; i < selected.length; i++) {
        const candidate = selected[i]
        setActionMessage(
          t('actions.importStep', {
            index: i + 1,
            total: selected.length,
            name: candidate.name,
          }),
        )
        try {
          const created = await invokeTauri<InstallResultDto>(
            'install_local_selection',
            {
              basePath: localCandidatesBasePath,
              subpath: candidate.subpath,
              name: localName.trim() || undefined,
            },
          )
          await applySelectedAddModalTags(created.skill_id, created.name)
          highlightSkill(created.skill_id)
        } catch (err) {
          const raw = err instanceof Error ? err.message : String(err)
          collectedErrors.push({
            title: t('errors.importFailedTitle', { name: candidate.name }),
            message: raw,
          })
        }
      }

      setShowLocalPickModal(false)
      setLocalCandidates([])
      setLocalCandidateSelected({})
      setLocalCandidatesBasePath('')
      setLocalPath('')
      setLocalName('')
      setActionMessage(t('status.selectedSkillsInstalled'))
      setSuccessToastMessage(t('status.selectedSkillsInstalled'))
      setSkillListMode('pending')
      setActionMessage(null)
      setShowAddModal(false)
      await loadManagedSkills()
      await loadTags()
      if (collectedErrors.length > 0) showActionErrors(collectedErrors)
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const handleInstallSelectedCandidates = async () => {
    const selected = gitCandidates.filter((c) => gitCandidateSelected[c.subpath])
    if (selected.length === 0) {
      setError(t('errors.selectAtLeastOneSkill'))
      return
    }
    if (reimportSkill) {
      if (selected.length !== 1) {
        setError(t('errors.selectOneSkillToReimport'))
        return
      }
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      setActionMessage(t('actions.reimporting', { name: reimportSkill.name }))
      try {
        const created = await invokeTauri<InstallResultDto>('reimport_git_selection', {
          skillId: reimportSkill.id,
          repoUrl: gitCandidatesRepoUrl,
          subpath: selected[0].subpath,
        })
        highlightSkill(created.skill_id)
        setShowGitPickModal(false)
        setGitCandidates([])
        setGitCandidateSelected({})
        setGitCandidatesRepoUrl('')
        setGitUrl('')
        setGitName('')
        setReimportSkill(null)
        setSuccessToastMessage(
          t('status.skillReimported', { name: created.name }),
        )
        setActionMessage(null)
        await loadManagedSkills()
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
      return
    }
    const duplicated = selected.find((c) => isSkillNameTaken(c.name))
    if (duplicated) {
      setError(t('errors.skillAlreadyExists', { name: duplicated.name }))
      return
    }
    const customName = getGitImportCustomName(selected.length, gitName)

    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      const collectedErrors: { title: string; message: string }[] = []
      for (let i = 0; i < selected.length; i++) {
        const candidate = selected[i]
        setActionMessage(
          t('actions.importStep', {
            index: i + 1,
            total: selected.length,
            name: candidate.name,
          }),
        )
        try {
          const created = await invokeTauri<InstallResultDto>(
            'install_git_selection',
            {
              repoUrl: gitCandidatesRepoUrl,
              subpath: candidate.subpath,
              name: customName,
            },
          )
          await applySelectedAddModalTags(created.skill_id, created.name)
          highlightSkill(created.skill_id)
        } catch (err) {
          const raw = err instanceof Error ? err.message : String(err)
          collectedErrors.push({
            title: t('errors.importFailedTitle', { name: candidate.name }),
            message: raw,
          })
        }
      }

      setShowGitPickModal(false)
      setGitCandidates([])
      setGitCandidateSelected({})
      setGitCandidatesRepoUrl('')
      setGitUrl('')
      setGitName('')
      setActionMessage(t('status.selectedSkillsInstalled'))
      setSuccessToastMessage(t('status.selectedSkillsInstalled'))
      setSkillListMode('pending')
      setActionMessage(null)
      setShowGitPickModal(false)
      setGitCandidates([])
      setGitCandidateSelected({})
      setGitCandidatesRepoUrl('')
      setShowAddModal(false)
      await loadManagedSkills()
      await loadTags()
      if (collectedErrors.length > 0) showActionErrors(collectedErrors)
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const handleDeleteManaged = async (skill: ManagedSkill) => {
    setLoading(true)
    setLoadingStartAt(Date.now())
    setActionMessage(t('actions.removing', { name: skill.name }))
    setError(null)
    try {
      await invokeTauri('delete_managed_skill', { skillId: skill.id })
      setActionMessage(t('status.skillRemoved'))
      setSuccessToastMessage(t('status.skillRemoved'))
      setActionMessage(null)
      setSkillScopeState((prev) => {
        const next = { ...prev }
        delete next[skill.id]
        return next
      })
      await loadManagedSkills()
      await loadTags()
      setPendingDeleteId(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
  }

  const handleSyncAllNewTools = useCallback(() => {
    if (!toolStatus) return
    const available = toolStatus.newly_installed.filter((toolId) =>
      toolStatus.tools.some(
        (tool) => tool.key === toolId && tool.enabled && tool.installed,
      ),
    )
    setShowNewToolsModal(false)
    if (available.length === 0 || managedSkills.length === 0) return
    setAssignmentInitialDraft({ global_tools: available, projects: [] })
    setBulkSelectedIds(managedSkills.map((skill) => skill.id))
    setBulkMode(true)
    setShowBatchAssignmentModal(true)
  }, [managedSkills, toolStatus])

  const handleOpenScope = useCallback((skill: ManagedSkill) => {
    setAssignmentInitialDraft(null)
    setScopeModalSkill(skill)
  }, [])

  const handleCloseScope = useCallback(() => {
    if (!loading) setScopeModalSkill(null)
  }, [loading])

  const handleOpenBatchAssignment = useCallback(() => {
    if (bulkSelectedSkills.length === 0) return
    setAssignmentInitialDraft(null)
    setShowBatchAssignmentModal(true)
  }, [bulkSelectedSkills.length])

  const handleCloseBatchAssignment = useCallback(() => {
    if (!loading) {
      setShowBatchAssignmentModal(false)
      setAssignmentInitialDraft(null)
    }
  }, [loading])

  const handleFinishBatchAssignment = useCallback(() => {
    if (loading) return
    setShowBatchAssignmentModal(false)
    setAssignmentInitialDraft(null)
    setBulkSelectedIds([])
    setBulkMode(false)
  }, [loading])

  const handleCloseProjectAssignmentBatch = useCallback(() => {
    if (!loading) {
      setProjectAssignmentBatch(null)
      setAssignmentInitialDraft(null)
    }
  }, [loading])

  const assignmentModalSkills = useMemo(() => {
    if (projectAssignmentBatch) {
      const selectedIds = new Set(projectAssignmentBatch.skillIds)
      return managedSkills.filter((skill) => selectedIds.has(skill.id))
    }
    if (showBatchAssignmentModal) return bulkSelectedSkills
    if (!scopeModalSkill) return []
    const current = managedSkills.find((skill) => skill.id === scopeModalSkill.id)
    return [current ?? scopeModalSkill]
  }, [
    bulkSelectedSkills,
    managedSkills,
    projectAssignmentBatch,
    scopeModalSkill,
    showBatchAssignmentModal,
  ])

  const projectBatchAddsLibrarySkills =
    projectAssignmentBatch?.mode === 'library-add'
  const projectBatchReplacesCurrentTools =
    projectAssignmentBatch?.mode === 'current-tools'
  const assignmentModalAdditive =
    showBatchAssignmentModal || projectBatchAddsLibrarySkills

  const handlePreviewAssignments = useCallback(
    async (
      draft: AssignmentDraftDto,
      additiveOverride?: boolean,
      skillIds?: string[],
    ): Promise<AssignmentPreviewDto | null> => {
      const selectedSkills = skillIds
        ? managedSkills.filter((skill) => skillIds.includes(skill.id))
        : assignmentModalSkills
      if (selectedSkills.length === 0) return null
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        if (
          projectBatchReplacesCurrentTools && selectedProjectDetail
        ) {
          const projectId = selectedProjectDetail.project.id
          const projectToolKeys =
            draft.projects.find((project) => project.project_id === projectId)
              ?.tool_keys ?? []
          const items: AssignmentPreviewDto['items'] = []
          for (const skill of selectedSkills) {
            const preview = await invokeTauri<AssignmentPreviewDto>(
              'preview_skill_assignments',
              {
                skillIds: [skill.id],
                draft: assignmentDraftReplacingProjectTools(
                  skill,
                  projectId,
                  projectToolKeys,
                ),
                additive: false,
              },
            )
            items.push(...preview.items)
          }
          return { items }
        }
        return await invokeTauri<AssignmentPreviewDto>('preview_skill_assignments', {
          skillIds: selectedSkills.map((skill) => skill.id),
          draft,
          additive: additiveOverride ?? assignmentModalAdditive,
        })
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        return null
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [
      assignmentModalAdditive,
      assignmentModalSkills,
      invokeTauri,
      managedSkills,
      projectBatchReplacesCurrentTools,
      selectedProjectDetail,
    ],
  )

  const handleApplyAssignments = useCallback(
    async (
      draft: AssignmentDraftDto,
      replaceConflictKeys: string[],
      additiveOverride?: boolean,
      skillIds?: string[],
    ): Promise<AssignmentApplyResultDto | null> => {
      const selectedSkills = skillIds
        ? managedSkills.filter((skill) => skillIds.includes(skill.id))
        : assignmentModalSkills
      if (selectedSkills.length === 0) return null
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      setActionMessage(t('assignment.applying'))
      try {
        let result: AssignmentApplyResultDto
        if (
          projectBatchReplacesCurrentTools && selectedProjectDetail
        ) {
          const projectId = selectedProjectDetail.project.id
          const projectToolKeys =
            draft.projects.find((project) => project.project_id === projectId)
              ?.tool_keys ?? []
          const results: AssignmentApplyResultDto['skills'] = []
          for (const skill of selectedSkills) {
            const applied = await invokeTauri<AssignmentApplyResultDto>(
              'apply_skill_assignments',
              {
                skillIds: [skill.id],
                draft: assignmentDraftReplacingProjectTools(
                  skill,
                  projectId,
                  projectToolKeys,
                ),
                replaceConflictKeys,
                additive: false,
              },
            )
            results.push(...applied.skills)
          }
          result = {
            succeeded: results.reduce((sum, item) => sum + item.succeeded, 0),
            failed: results.reduce((sum, item) => sum + item.failed, 0),
            skipped: results.reduce((sum, item) => sum + item.skipped, 0),
            skills: results,
          }
        } else {
          result = await invokeTauri<AssignmentApplyResultDto>(
              'apply_skill_assignments',
              {
              skillIds: selectedSkills.map((skill) => skill.id),
              draft,
              replaceConflictKeys,
              additive: additiveOverride ?? assignmentModalAdditive,
            },
          )
        }
        await loadManagedSkills()
        if (selectedProjectDetail) {
          const detail = await invokeTauri<ProjectDetailDto>('scan_project', {
            projectId: selectedProjectDetail.project.id,
          })
          setSelectedProjectDetail(detail)
        }
        await loadProjects()
        setScopeModalSkill(null)
        if (!showBatchAssignmentModal) {
          setShowBatchAssignmentModal(false)
          setProjectAssignmentBatch(null)
          setAssignmentInitialDraft(null)
        }
        if (showBatchAssignmentModal) {
          return result
        }
        if (result.failed > 0) {
          showActionErrors(
            result.skills
              .filter((skill) => skill.errors.length > 0)
              .map((skill) => ({
                title: t('assignment.applyFailedTitle', { name: skill.skill_name }),
                message: skill.errors.join('\n'),
              })),
          )
        } else {
          setSuccessToastMessage(
            t('assignment.applied', {
              succeeded: result.succeeded,
              skipped: result.skipped,
            }),
          )
        }
        return result
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
        return null
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
        setActionMessage(null)
      }
    },
    [
      assignmentModalSkills,
      assignmentModalAdditive,
      invokeTauri,
      loadManagedSkills,
      loadProjects,
      managedSkills,
      projectBatchReplacesCurrentTools,
      selectedProjectDetail,
      showActionErrors,
      showBatchAssignmentModal,
      t,
    ],
  )

  const handleIgnoreAssignmentIssue = useCallback(
    async (skillId: string, issueKey: string, fingerprint: string) => {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        await invokeTauri('ignore_assignment_issue', {
          skillId,
          issueKey,
          fingerprint,
        })
        await loadManagedSkills()
        setSuccessToastMessage(t('assignment.issueIgnored'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [invokeTauri, loadManagedSkills, t],
  )

  const handleIgnoreAllAssignmentIssues = useCallback(
    async (skillIds: string[]) => {
      setLoading(true)
      setLoadingStartAt(Date.now())
      setError(null)
      try {
        await invokeTauri<number>('ignore_all_assignment_issues', { skillIds })
        await loadManagedSkills()
        setSuccessToastMessage(t('assignment.issuesIgnored'))
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
        setLoadingStartAt(null)
      }
    },
    [invokeTauri, loadManagedSkills, t],
  )

  const handleUpdateManaged = useCallback(
    async (skill: ManagedSkill) => {
    setLoading(true)
    setLoadingStartAt(Date.now())
    setError(null)
    try {
      setActionMessage(t('actions.updating', { name: skill.name }))
      await invokeTauri<UpdateResultDto>('update_managed_skill', { skillId: skill.id })
      const updatedText = t('status.updated', { name: skill.name })
      setActionMessage(updatedText)
      setSuccessToastMessage(updatedText)
      setActionMessage(null)
      await loadManagedSkills()
    } catch (err) {
      const raw = err instanceof Error ? err.message : String(err)
      setError(raw)
    } finally {
      setLoading(false)
      setLoadingStartAt(null)
    }
    },
    [invokeTauri, loadManagedSkills, t],
  )

  const handleUpdateSkill = useCallback(
    (skill: ManagedSkill) => {
      void handleUpdateManaged(skill)
    },
    [handleUpdateManaged],
  )

  const handleReimportAssignmentSkill = useCallback((skill: ManagedSkill) => {
    setScopeModalSkill(null)
    setShowBatchAssignmentModal(false)
    setAssignmentInitialDraft(null)
    setReimportSkill(skill)
    setAddModalTagIds([])
    const source = skill.source_ref?.trim() ?? ''
    if (skill.source_type.toLowerCase().includes('git') || /^https?:\/\//.test(source)) {
      setAddModalTab('git')
      setGitUrl(source)
      setGitName(skill.name)
    } else {
      setAddModalTab('local')
      setLocalPath(source)
      setLocalName(skill.name)
    }
    setShowAddModal(true)
  }, [])

  const handleRemoveAssignmentSkill = useCallback((skill: ManagedSkill) => {
    setScopeModalSkill(null)
    setShowBatchAssignmentModal(false)
    setAssignmentInitialDraft(null)
    setPendingDeleteId(skill.id)
  }, [])

  const assignmentContextProjectId =
    activeView === 'projectDetail'
      ? selectedProjectDetail?.project.id
      : undefined

  return (
    <div className="skills-app">
      <Toaster
        position="top-right"
        richColors
        toastOptions={{ duration: 1800 }}
      />
      <LoadingOverlay
        loading={loading}
        actionMessage={actionMessage}
        loadingStartAt={loadingStartAt}
        onCancel={handleCancelLoading}
        t={t}
      />

      <Header
        language={language}
        loading={loading}
        activeView={activeView}
        onToggleLanguage={toggleLanguage}
        onOpenSettings={handleOpenSettings}
        onViewChange={handleViewChange}
        t={t}
      />

      <main className="skills-main">
        {activeView === 'projectDetail' && selectedProjectDetail ? (
          <ProjectDetailPage
            key={selectedProjectDetail.project.id}
            detail={selectedProjectDetail}
            loading={loading}
            onBack={handleBackToProjects}
            onRefresh={handleRefreshProject}
            onToolsChange={handleProjectToolsChange}
            onGitExcludeChange={handleProjectGitExcludeChange}
            onAddLibrarySkill={handleAddProjectLibrarySkill}
            onAddLibrarySkills={handleAddProjectLibrarySkills}
            onHandleLibraryConflict={handleInjectProjectSkill}
            onAssignSkills={handleAssignCurrentProjectSkills}
            onRemoveSkill={handleRemoveProjectSkill}
            onRemoveSkills={(skillIds) => void handleRemoveProjectSkills(skillIds)}
            onCollectSkill={(discoveredSkillId, name) =>
              void handleCollectProjectSkill(discoveredSkillId, name)
            }
            onResolveIssue={handleRepairProjectIssue}
            onIgnoreIssue={handleIgnoreProjectIssue}
            onIgnoreAllIssues={handleIgnoreAllProjectIssues}
            t={t}
          />
        ) : activeView === 'projects' ? (
          <ProjectsPage
            projects={projects}
            importPath={projectImportPath}
            loading={loading}
            onImportPathChange={setProjectImportPath}
            onPickProject={() => void handlePickProjectRoot()}
            onImportProject={handleImportProject}
            onOpenProject={handleOpenProject}
            formatRelative={formatRelative}
            t={t}
          />
        ) : activeView === 'detail' && detailSkill ? (
          <SkillDetailView
            skill={detailSkill}
            onBack={handleBackToList}
            onOpenAssignment={handleOpenScope}
            invokeTauri={invokeTauri}
            formatRelative={formatRelative}
            t={t}
          />
        ) : activeView === 'myskills' ? (
          <div className="dashboard-stack">
            <FilterBar
              listMode={skillListMode}
              allCount={managedSkills.length}
              pendingCount={pendingAssignmentCount}
              sortBy={sortBy}
              searchQuery={searchQuery}
              scopeFilter={scopeFilter}
              tags={tags}
              selectedTagIds={selectedTagIds}
              includeUntagged={includeUntagged}
              untaggedCount={untaggedCount}
              bulkMode={bulkMode}
              bulkSelectedCount={bulkSelectedIds.length}
              onSortChange={handleSortChange}
              onListModeChange={setSkillListMode}
              onSearchChange={handleSearchChange}
              onScopeFilterChange={handleScopeFilterChange}
              onToggleTag={handleToggleTagFilter}
              onToggleUntagged={handleToggleUntaggedFilter}
              onClearTags={handleClearTagFilters}
              onManageTags={handleOpenTagsPage}
              onToggleBulkMode={handleToggleBulkMode}
              t={t}
            />
            <SkillsList
              plan={plan}
              visibleSkills={visibleSkills}
              installedTools={installedTools}
              loading={loading}
              bulkMode={bulkMode}
              selectedSkillIds={bulkSelectedIds}
              highlightedSkillIds={highlightedSkillIds}
              getGithubInfo={getGithubInfo}
              getSkillSourceLabel={getSkillSourceLabel}
              formatRelative={formatRelative}
              onReviewImport={handleReviewImport}
              onUpdateSkill={handleUpdateSkill}
              onDeleteSkill={handleDeletePrompt}
              onToggleSkillEnabled={handleToggleSkillEnabled}
              onOpenScope={handleOpenScope}
              onOpenDetail={handleOpenDetail}
              onEditTags={handleOpenEditTags}
              onToggleBulkSelection={handleToggleBulkSelection}
              getSkillScope={getSkillScope}
              getSkillProjects={getSkillProjects}
              t={t}
            />
            {bulkMode ? (
              <div className="bulk-action-bar">
                <div className="bulk-action-copy">
                  <strong>{t('bulk.selected', { count: bulkSelectedIds.length })}</strong>
                  <span>
                    {allLibraryBulkSelected
                      ? t('bulk.entireLibrarySelected', {
                          count: managedSkills.length,
                        })
                      : t('bulk.helper')}
                  </span>
                </div>
                <div className="bulk-action-buttons">
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleSelectVisibleSkills}
                    disabled={loading || visibleSkills.length === 0}
                  >
                    {allVisibleBulkSelected
                      ? t('bulk.unselectVisible')
                      : t('bulk.selectVisible')}
                  </button>
                  {allVisibleBulkSelected && !allLibraryBulkSelected ? (
                    <button
                      className="btn btn-secondary"
                      type="button"
                      onClick={handleSelectEntireLibrary}
                      disabled={loading || managedSkills.length === 0}
                    >
                      {t('bulk.selectEntireLibrary', {
                        count: managedSkills.length,
                      })}
                    </button>
                  ) : null}
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleOpenBulkTags}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {t('bulk.tags')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={handleOpenBatchAssignment}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {t('assignment.batchAction')}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={() => void handleToggleBulkEnabled()}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {bulkShouldEnable ? t('bulk.enable') : t('bulk.disable')}
                  </button>
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={handleOpenBulkDelete}
                    disabled={loading || bulkSelectedIds.length === 0}
                  >
                    {t('bulk.delete')}
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        ) : activeView === 'manage' ? (
          <div className="management-page">
            <div className="management-header">
              <div className="management-heading">
                <h1>{t('manageCenterTitle')}</h1>
                <p>{t('manageCenterHelp')}</p>
              </div>
              <div className="management-tab-list" role="tablist" aria-label={t('manageCenterTitle')}>
                <button
                  className={`management-tab${managementTab === 'tags' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'tags'}
                  onClick={() => setManagementTab('tags')}
                >
                  {t('manageTabs.tags')}
                </button>
                <button
                  className={`management-tab${managementTab === 'tools' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'tools'}
                  onClick={() => setManagementTab('tools')}
                >
                  {t('manageTabs.tools')}
                </button>
                <button
                  className={`management-tab${managementTab === 'updates' ? ' active' : ''}`}
                  type="button"
                  role="tab"
                  aria-selected={managementTab === 'updates'}
                  onClick={() => setManagementTab('updates')}
                >
                  {t('manageTabs.updates')}
                </button>
              </div>
            </div>
            <div className="management-panel">
              {managementTab === 'tags' ? (
                <TagsPage
                  embedded
                  tags={tags}
                  untaggedCount={untaggedCount}
                  loading={loading}
                  formatRelative={formatRelative}
                  onReviewUntagged={handleReviewUntagged}
                  onViewTag={handleViewTag}
                  onCreateTag={handleCreateTag}
                  onRenameTag={handleRenameTag}
                  onDeleteTag={handleDeleteTag}
                  t={t}
                />
              ) : managementTab === 'tools' ? (
                <ToolsPage
                  embedded
                  toolStatus={toolStatus}
                  toolConfig={toolConfig}
                  onToolConfigChange={handleToolConfigChange}
                  t={t}
                />
              ) : (
                <UpdatesPage
                  autoUpdateConfig={autoUpdateConfig}
                  onAutoUpdateConfigChange={handleAutoUpdateConfigChange}
                  onRunAutoUpdateNow={handleTriggerAutoUpdateTaskNow}
                  autoUpdateTriggering={autoUpdateTriggering}
                  skills={managedSkills}
                  onRetrySkills={handleRetryAutoUpdateSkills}
                  t={t}
                />
              )}
            </div>
          </div>
        ) : activeView === 'settings' ? (
          <SettingsPage
            isTauri={isTauri}
            language={language}
            storagePath={storagePath}
            gitCacheCleanupDays={gitCacheCleanupDays}
            gitCacheTtlSecs={gitCacheTtlSecs}
            themePreference={themePreference}
            onPickStoragePath={handlePickStoragePath}
            onToggleLanguage={toggleLanguage}
            onThemeChange={handleThemeChange}
            onGitCacheCleanupDaysChange={handleGitCacheCleanupDaysChange}
            onGitCacheTtlSecsChange={handleGitCacheTtlSecsChange}
            onClearGitCacheNow={handleClearGitCacheNow}
            githubToken={githubToken}
            onGithubTokenChange={handleGithubTokenChange}
            githubProxyConfig={githubProxyConfig}
            onGithubProxyConfigChange={handleGithubProxyConfigChange}
            onBack={handleCloseSettings}
            t={t}
          />
        ) : (
          <ExplorePage
            featuredSkills={featuredSkills}
            featuredLoading={featuredLoading}
            exploreFilter={exploreFilter}
            searchResults={searchResults}
            searchLoading={searchLoading}
            managedSkills={managedSkills}
            loading={loading}
            onExploreFilterChange={handleExploreFilterChange}
            onInstallSkill={handleExploreInstall}
            onOpenManualAdd={handleOpenAdd}
            t={t}
          />
        )}
      </main>

      <AddSkillModal
        open={showAddModal}
        loading={loading}
        canClose={!loading}
        addModalTab={addModalTab}
        localPath={localPath}
        localName={localName}
        gitUrl={gitUrl}
        gitName={gitName}
        tags={tags}
        selectedTagIds={addModalTagIds}
        reimportSkillName={reimportSkill?.name}
        onRequestClose={handleCloseAdd}
        onTabChange={setAddModalTab}
        onLocalPathChange={setLocalPath}
        onPickLocalPath={handlePickLocalPath}
        onLocalNameChange={setLocalName}
        onGitUrlChange={setGitUrl}
        onGitNameChange={setGitName}
        onToggleTag={handleToggleAddModalTag}
        onSubmit={addModalTab === 'local' ? handleCreateLocal : handleCreateGit}
        t={t}
      />

      <EditSkillTagsModal
        key={
          tagEditorSkill
            ? `${tagEditorSkill.id}-${tagEditorSkill.tags.map((tag) => tag.id).join('-')}`
            : 'edit-tags'
        }
        open={Boolean(tagEditorSkill)}
        loading={loading}
        skill={
          tagEditorSkill
            ? managedSkills.find((skill) => skill.id === tagEditorSkill.id) ?? tagEditorSkill
            : null
        }
        tags={tags}
        onRequestClose={handleCloseEditTags}
        onSave={handleSaveSkillTags}
        t={t}
      />

      <BulkTagsModal
        open={showBulkTagsModal}
        loading={loading}
        selectedSkills={bulkSelectedSkills}
        tags={tags}
        onRequestClose={handleCloseBulkTags}
        onConfirm={handleConfirmBulkTags}
        t={t}
      />

      <BulkDeleteModal
        open={showBulkDeleteModal}
        loading={loading}
        skillNames={bulkSelectedNames}
        onRequestClose={handleCloseBulkDelete}
        onConfirm={handleConfirmBulkDelete}
        t={t}
      />

      {showImportModal && plan ? (
        <ImportModal
          open={showImportModal}
          loading={loading}
          plan={plan}
          selected={selected}
          variantChoice={variantChoice}
          hubResolutions={hubResolutions}
          reservedSkillNames={managedSkills.map((skill) => skill.name)}
          onRequestClose={handleCloseImport}
          onToggleGroup={handleToggleGroup}
          onSelectVariant={handleSelectVariant}
          onResolveHubConflict={handleResolveHubConflict}
          onImport={handleImport}
          t={t}
        />
      ) : null}

      <AssignmentModal
        key={
          assignmentModalSkills.length > 0
            ? `${assignmentModalSkills.map((skill) => skill.id).join('-')}-${assignmentInitialDraft?.global_tools.join('-') ?? 'current'}-${assignmentContextProjectId ?? 'all'}-${projectAssignmentBatch?.mode ?? 'standard'}`
            : 'assignment-modal'
        }
        open={Boolean(
          scopeModalSkill || showBatchAssignmentModal || projectAssignmentBatch,
        )}
        loading={loading}
        skills={assignmentModalSkills}
        tools={toolStatus?.tools ?? []}
        projects={projects}
        contextProjectId={assignmentContextProjectId}
        additive={assignmentModalAdditive}
        libraryBatch={showBatchAssignmentModal}
        projectBatchReplace={projectBatchReplacesCurrentTools}
        requireProjectTools={Boolean(projectAssignmentBatch)}
        initialDraft={assignmentInitialDraft}
        onRequestClose={
          projectAssignmentBatch
            ? handleCloseProjectAssignmentBatch
            : showBatchAssignmentModal
              ? handleCloseBatchAssignment
              : handleCloseScope
        }
        onPreview={handlePreviewAssignments}
        onApply={handleApplyAssignments}
        onBatchFinished={handleFinishBatchAssignment}
        onIgnoreIssue={handleIgnoreAssignmentIssue}
        onIgnoreAllIssues={handleIgnoreAllAssignmentIssues}
        onRepairSkill={handleUpdateManaged}
        onReimportSkill={handleReimportAssignmentSkill}
        onRemoveSkill={handleRemoveAssignmentSkill}
        t={t}
      />

      <NewToolsModal
        open={Boolean(showNewToolsModal && newlyInstalledToolsText)}
        loading={loading}
        toolsLabelText={newlyInstalledToolsText}
        onLater={handleCloseNewTools}
        onSyncAll={handleSyncAllNewTools}
        t={t}
      />

      <DeleteModal
        open={Boolean(pendingDeleteId)}
        loading={loading}
        skillName={pendingDeleteSkill?.name ?? null}
        onRequestClose={handleCloseDelete}
        onConfirm={() => {
          if (pendingDeleteSkill) void handleDeleteManaged(pendingDeleteSkill)
        }}
        t={t}
      />

      {pendingDeleteTag ? (
        <div className="modal-backdrop" onClick={loading ? undefined : handleCloseDeleteTag}>
          <div
            className="modal modal-delete tag-delete-modal"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="modal-header">
              <div className="modal-title">{t('deleteTagTitle')}</div>
              <button
                className="modal-close"
                type="button"
                onClick={handleCloseDeleteTag}
                disabled={loading}
              >
                ×
              </button>
            </div>
            <div className="modal-body tag-delete-body">
              {t('deleteTagConfirm', {
                name: pendingDeleteTag.name,
                count: pendingDeleteTag.skill_count,
              })}
            </div>
            <div className="modal-footer">
              <button
                className="btn btn-secondary"
                type="button"
                onClick={handleCloseDeleteTag}
                disabled={loading}
              >
                {t('cancel')}
              </button>
              <button
                className="btn btn-danger"
                type="button"
                onClick={() => void handleConfirmDeleteTag()}
                disabled={loading}
              >
                {t('deleteAction')}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {showLocalPickModal ? (
        <LocalPickModal
          open={showLocalPickModal}
          loading={loading}
          localCandidates={localCandidates}
          localCandidateSelected={localCandidateSelected}
          reimporting={Boolean(reimportSkill)}
          onRequestClose={handleCloseLocalPick}
          onCancel={handleCancelLocalPick}
          onToggleCandidate={handleToggleLocalCandidate}
          onInstall={handleInstallSelectedLocalCandidates}
          t={t}
        />
      ) : null}

      {showGitPickModal ? (
        <GitPickModal
          open={showGitPickModal}
          loading={loading}
          gitCandidates={gitCandidates}
          gitCandidateSelected={gitCandidateSelected}
          customName={gitName}
          reimporting={Boolean(reimportSkill)}
          onRequestClose={handleCloseGitPick}
          onCancel={handleCancelGitPick}
          onToggleCandidate={handleToggleGitCandidate}
          onInstall={handleInstallSelectedCandidates}
          t={t}
        />
      ) : null}

      </div>
  )
}

export default App
