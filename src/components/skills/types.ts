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
