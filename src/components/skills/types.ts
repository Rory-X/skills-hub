export type OnboardingVariant = {
  tool: string
  name: string
  path: string
  fingerprint?: string | null
  is_link: boolean
  link_target?: string | null
  plugin_name?: string | null
  plugin_version?: string | null
  plugin_scope?: string | null
  hub_relation?: 'same' | 'different' | 'unknown' | null
}

export type OnboardingHubSkill = {
  id: string
  name: string
  central_path: string
  fingerprint?: string | null
}

export type OnboardingGroup = {
  name: string
  variants: OnboardingVariant[]
  has_conflict: boolean
  hub_skill?: OnboardingHubSkill | null
}

export type OnboardingPlan = {
  total_tools_scanned: number
  total_skills_found: number
  groups: OnboardingGroup[]
}

export type ToolOption = {
  id: string
  label: string
  supports_project_scope?: boolean
}

export type TagDto = {
  id: number
  name: string
}

export type TagWithCountDto = TagDto & {
  skill_count: number
  updated_at: number
}

export type ManagedSkill = {
  id: string
  name: string
  description?: string | null
  source_type: string
  source_ref?: string | null
  central_path: string
  created_at: number
  updated_at: number
  last_sync_at?: number | null
  enabled: boolean
  status: string
  tags: TagDto[]
  targets: {
    tool: string
    scope: 'global' | 'project' | string
    project_path?: string | null
    mode: string
    status: string
    target_path: string
    synced_at?: number | null
  }[]
  assignments: SkillAssignmentDto[]
  source_issue?: 'source_missing' | 'source_invalid' | null
  pending_assignment: boolean
  assignment_issue_count: number
}

export type SkillAssignmentDto = {
  tool: string
  scope: 'global' | 'project' | string
  project_id?: string | null
  project_name?: string | null
  project_path?: string | null
  ownership: 'hub' | 'native' | string
  state: 'active' | 'suspended' | 'error' | string
  last_error?: string | null
  valid: boolean
  issue_kind?: string | null
  issue_key?: string | null
  issue_fingerprint?: string | null
  can_ignore: boolean
  issue_ignored: boolean
}

export type AssignmentDraftDto = {
  global_tools: string[]
  projects: {
    project_id: string
    tool_keys: string[]
  }[]
}

export type AssignmentPreviewItemDto = {
  key: string
  skill_id: string
  skill_name: string
  tool: string
  scope: 'global' | 'project' | string
  project_id?: string | null
  project_name?: string | null
  target_path?: string | null
  outcome:
    | 'ready'
    | 'assigned'
    | 'same_content'
    | 'conflict'
    | 'unavailable'
    | 'remove'
    | 'unsafe'
    | string
  blocking: boolean
}

export type AssignmentPreviewDto = {
  items: AssignmentPreviewItemDto[]
}

export type AssignmentApplyResultDto = {
  skills: {
    skill_id: string
    skill_name: string
    succeeded: number
    failed: number
    skipped: number
    errors: string[]
  }[]
  succeeded: number
  failed: number
  skipped: number
}

export type GitSkillCandidate = {
  name: string
  description?: string | null
  subpath: string
}

export type LocalSkillCandidate = {
  name: string
  description?: string | null
  subpath: string
  valid: boolean
  reason?: string | null
}

export type InstallResultDto = {
  skill_id: string
  name: string
  central_path: string
  content_hash?: string | null
}

export type ToolInfoDto = {
  key: string
  label: string
  installed: boolean
  enabled: boolean
  is_custom: boolean
  skills_dir: string
  project_skills_dir: string
  supports_project_scope: boolean
}

export type ToolStatusDto = {
  tools: ToolInfoDto[]
  installed: string[]
  newly_installed: string[]
}

export type CustomToolConfigDto = {
  key: string
  label: string
  skills_dir: string
  project_skills_dir?: string | null
  enabled: boolean
}

export type ToolConfigDto = {
  disabled_builtin_tools: string[]
  custom_tools: CustomToolConfigDto[]
}

export type UpdateResultDto = {
  skill_id: string
  name: string
  content_hash?: string | null
  source_revision?: string | null
  updated_targets: string[]
}

export type AutoUpdateConfigDto = {
  enabled: boolean
  interval_hours: number
  schedule_type: 'interval' | 'daily'
  interval_value: number
  interval_unit: 'minutes' | 'hours'
  daily_time: string
  local_skill_count: number
  protected_local_skill_count: number
  task_registered: boolean
  task_status_detail: string
  last_run_at?: number | null
  last_started_at?: number | null
  last_finished_at?: number | null
  last_status?: string | null
  last_error?: string | null
  last_checked: number
  last_updated: number
  last_failed: number
  progress: AutoUpdateProgressSnapshotDto
}

export type AutoUpdateRunResultDto = {
  checked: number
  updated: number
  failed: number
  errors: string[]
  progress: AutoUpdateProgressSnapshotDto
}

export type AutoUpdateRetryResultDto = {
  updated: number
  failed: AutoUpdateSkillProgressDto[]
  config: AutoUpdateConfigDto
}

export type GithubProxyConfigDto = {
  enabled: boolean
  port: number
  url: string
  auto_detected: boolean
}

export type AutoUpdateSkillProgressDto = {
  skill_id: string
  name: string
  reason?: string | null
}

export type AutoUpdateProgressSnapshotDto = {
  total: number
  succeeded: AutoUpdateSkillProgressDto[]
  failed: AutoUpdateSkillProgressDto[]
  running?: AutoUpdateSkillProgressDto | null
  pending: AutoUpdateSkillProgressDto[]
}

export type FeaturedSkillDto = {
  slug: string
  name: string
  summary: string
  downloads: number
  stars: number
  source_url: string
}

export type OnlineSkillDto = {
  name: string
  installs: number
  source: string
  source_url: string
}

export type SkillFileEntry = {
  path: string
  size: number
}

export type SkillPreviewSource =
  | { kind: 'managed'; skillId: string }
  | { kind: 'local'; basePath: string; subpath: string }
  | { kind: 'git'; repoUrl: string; subpath: string }

export type SkillPreviewFile = SkillFileEntry

export type SkillPreviewManifest = {
  files: SkillPreviewFile[]
  truncated: boolean
  revision?: string | null
}

export type SkillPreviewContent = {
  kind: 'text' | 'image' | 'unsupported'
  content?: string | null
  mediaType?: string | null
  bytes?: number[] | null
  reason?: string | null
}

export type ProjectSummaryDto = {
  id: string
  name: string
  root_path: string
  enabled_tool_keys: string[]
  local_git_exclude_enabled: boolean
  created_at: number
  updated_at: number
  last_scanned_at?: number | null
  skill_count: number
  attention_count: number
}

export type ProjectToolDto = {
  key: string
  label: string
  project_skills_dir: string
  enabled: boolean
  installed: boolean
}

export type ProjectSkillPathDto = {
  discovered_skill_id: string
  relative_path: string
  absolute_path: string
  tool_keys: string[]
  content_hash?: string | null
  status: 'valid' | 'matched' | 'invalid_missing_skill_md'
}

export type ProjectSkillDto = {
  name: string
  description?: string | null
  source: 'project' | 'library' | 'matched_library'
  status:
    | 'healthy'
    | 'needs_attention'
    | 'ok'
    | 'pending'
    | 'partial'
    | 'error'
    | 'conflict'
  linked_skill_id?: string | null
  matched_skill_id?: string | null
  tool_keys: string[]
  paths: ProjectSkillPathDto[]
}

export type ProjectLibrarySkillDto = {
  skill_id: string
  name: string
  description?: string | null
  availability:
    | 'addable'
    | 'enabled'
    | 'project_same'
    | 'conflict'
    | 'needs_repair'
    | 'disabled'
}

export type ProjectIssueKind =
  | 'invalid_skill'
  | 'same_project_name_content_mismatch'
  | 'same_name_content_conflict'
  | 'tool_added_pending_sync'
  | 'injection_failed'
  | 'target_missing'
  | 'tool_removed_pending_unsync'
  | 'git_exclude_drift'
  | 'git_exclude_unavailable'
  | 'library_skill_unavailable'

export type ProjectIssueActionKind =
  | 'delete_invalid_skill'
  | 'keep_project_variant'
  | 'replace_with_library'
  | 'keep_project_version'
  | 'retry_injection'
  | 'remove_stale_target'
  | 'repair_git_exclude'
  | 'disable_git_exclude'
  | 'initialize_git_repository'
  | 'enable_library_skill'
  | 'remove_unavailable_library_skill'

export type ProjectIssueActionDto = {
  kind: ProjectIssueActionKind
  value?: string | null
  label?: string | null
  confirmation_required: boolean
  destructive: boolean
}

export type ProjectIssueDto = {
  id: string
  kind: ProjectIssueKind
  skill_id?: string | null
  skill_name?: string | null
  tool_key?: string | null
  path?: string | null
  actions: ProjectIssueActionDto[]
  fingerprint: string
  can_ignore: boolean
}

export type ProjectDetailDto = {
  project: ProjectSummaryDto
  tools: ProjectToolDto[]
  current_skills: ProjectSkillDto[]
  library_skills: ProjectLibrarySkillDto[]
  issues: ProjectIssueDto[]
}

export type DeviceSyncProvider = 'github' | 'gitlab' | 'gitee'

export type DeviceSyncSchedule =
  | { mode: 'interval'; minutes: number }
  | { mode: 'daily'; time: string }

export type RepositoryVisibility = 'public' | 'private' | 'internal' | 'unknown'

export type DeviceSyncConfigDto = {
  visibility?: RepositoryVisibility
  public_upload_confirmed?: boolean
  provider: DeviceSyncProvider
  remote_url: string
  branch: string
  username?: string | null
  auto_check: boolean
  auto_sync: boolean
  auto_sync_schedule?: DeviceSyncSchedule | null
  has_credential: boolean
}

export type DeviceSyncOAuthAvailability = {
  provider: DeviceSyncProvider
  available: boolean
  reason?: string | null
}

export type DeviceSyncOAuthStart = {
  flow_id: string
  verification_uri: string
  verification_uri_complete?: string | null
  user_code?: string | null
  expires_at: number
  interval_seconds: number
}

export type DeviceSyncProviderAccount = {
  login: string
  display_name?: string | null
}

export type DeviceSyncOAuthPoll = {
  provider: DeviceSyncProvider
  status: 'pending' | 'authorized'
  interval_seconds: number
  credential_key?: string | null
  account?: DeviceSyncProviderAccount | null
}

export type DeviceSyncPendingOAuth = {
  provider: DeviceSyncProvider
  credential_key: string
  account: DeviceSyncProviderAccount
}

export type DeviceSyncRemoteRepository = {
  visibility?: RepositoryVisibility
  name: string
  web_url: string
  clone_url: string
  ssh_url?: string | null
  private: boolean
}

export type DeviceSyncChangeItem = {
  skill_id: string
  name: string
  kind: 'added' | 'updated' | 'deleted' | 'conflicted'
  direction: 'upload' | 'download' | 'merge'
}

export type DeviceSyncChangeSummary = {
  items?: DeviceSyncChangeItem[] | null
  added: number
  updated: number
  deleted: number
  conflicted: number
}

export type DeviceSyncRunResult = {
  status: string
  commit?: string | null
  changes: DeviceSyncChangeSummary
  message: string
}

export type DeviceSyncStatus = {
  tool_issues?: { skill_name: string; tool: string }[]
  schedule_status?: {
    state: 'disabled' | 'initializing' | 'scheduled' | 'backoff' | 'paused' | 'running' | 'waiting' | 'needs_confirmation'
    next_at: number | null
  } | null
  configured: boolean
  is_running: boolean
  provider: DeviceSyncProvider
  remote_url: string
  auto_check: boolean
  auto_sync: boolean
  last_synced_commit?: string | null
  repository_head_commit?: string | null
  pending_local_changes: number
  conflict_count: number
  last_run_status?: string | null
  last_run_at?: number | null
}

export type DeviceSyncDevice = {
  id: string
  name: string
  alias?: string | null
  last_commit?: string | null
  last_seen_at: number
  is_current: boolean
}

export type DeviceSyncHistoryEntry = DeviceSyncChangeSummary & {
  id: string
  started_at: number
  finished_at?: number | null
  status: string
  commit?: string | null
  error?: string | null
}

export type DeviceSyncConflict = {
  id: string
  skill_id: string
  skill_name: string
  files: string[]
  base_commit?: string | null
  created_at: number
  status: string
}

export type DeviceSyncTrashEntry = {
  id: string
  skill_id: string
  skill_name: string
  deleted_at: number
  expires_at: number
}
