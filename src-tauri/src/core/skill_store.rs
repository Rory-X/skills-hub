use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use uuid::Uuid;

use super::device_sync::types::{
    DeviceSyncConfig, DeviceSyncDevice, SyncConflict, SyncHistoryEntry, TrashEntry,
};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::Manager;

const DB_FILE_NAME: &str = "skills_hub.db";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.tauri.dev", "com.tauri.dev.skillshub"];

// Schema versioning: bump when making changes and add a migration step.
const SCHEMA_VERSION: i32 = 10;

const DEVICE_SYNC_SCHEMA_VERSION_KEY: &str = "schema.device_sync";
const DEVICE_SYNC_SCHEMA_VERSION: &str = "1";
const RECYCLE_BIN_SCHEMA_VERSION_KEY: &str = "schema.recycle_bin";
const RECYCLE_BIN_SCHEMA_VERSION: &str = "1";
pub const DEVICE_SYNC_HISTORY_LIMIT: usize = 100;
const DEVICE_SYNC_STARTUP_CREDENTIAL_CONSENT_MIGRATION: &str =
    "migration.device_sync_startup_credential_consent_v1";

const DEVICE_SYNC_SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS device_sync_config (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  config_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS device_sync_runs (
  id TEXT PRIMARY KEY,
  started_at INTEGER NOT NULL,
  finished_at INTEGER NULL,
  status TEXT NOT NULL,
  added INTEGER NOT NULL DEFAULT 0,
  updated INTEGER NOT NULL DEFAULT 0,
  deleted INTEGER NOT NULL DEFAULT 0,
  conflicted INTEGER NOT NULL DEFAULT 0,
  commit_hash TEXT NULL,
  error TEXT NULL
);

CREATE TABLE IF NOT EXISTS device_sync_conflicts (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL,
  skill_name TEXT NOT NULL,
  base_commit TEXT NULL,
  local_commit TEXT NOT NULL,
  remote_commit TEXT NOT NULL,
  files_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device_sync_devices (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  last_commit TEXT NULL,
  last_seen_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS device_sync_tombstones (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL,
  skill_name TEXT NOT NULL,
  trash_path TEXT NOT NULL,
  deleted_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  deletion_source TEXT NOT NULL DEFAULT 'sync',
  metadata_json TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_device_sync_runs_started_at
ON device_sync_runs(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_device_sync_conflicts_status
ON device_sync_conflicts(status, created_at DESC);
"#;

fn prune_device_sync_history(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM settings
         WHERE key IN (
           SELECT 'device_sync.run_items.' || id
           FROM device_sync_runs
           WHERE finished_at IS NOT NULL
           ORDER BY started_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         )",
        params![DEVICE_SYNC_HISTORY_LIMIT as i64],
    )?;
    tx.execute(
        "DELETE FROM device_sync_runs
         WHERE id IN (
           SELECT id FROM device_sync_runs
           WHERE finished_at IS NOT NULL
           ORDER BY started_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         )",
        params![DEVICE_SYNC_HISTORY_LIMIT as i64],
    )?;
    Ok(())
}

// Minimal schema for MVP: skills, skill_targets, settings, discovered_skills(optional).
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS skills (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source_type TEXT NOT NULL,
  source_ref TEXT NULL,
  source_revision TEXT NULL,
  central_path TEXT NOT NULL UNIQUE,
  content_hash TEXT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_sync_at INTEGER NULL,
  last_seen_at INTEGER NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS skill_targets (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  scope TEXT NOT NULL DEFAULT 'global',
  project_path TEXT NULL,
  target_path TEXT NOT NULL,
  mode TEXT NOT NULL,
  status TEXT NOT NULL,
  last_error TEXT NULL,
  synced_at INTEGER NULL,
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_targets_unique_scope
ON skill_targets(skill_id, tool, scope, COALESCE(project_path, ''));

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS discovered_skills (
  id TEXT PRIMARY KEY,
  tool TEXT NOT NULL,
  found_path TEXT NOT NULL,
  name_guess TEXT NULL,
  fingerprint TEXT NULL,
  found_at INTEGER NOT NULL,
  imported_skill_id TEXT NULL,
  FOREIGN KEY(imported_skill_id) REFERENCES skills(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
CREATE INDEX IF NOT EXISTS idx_skills_updated_at ON skills(updated_at);

CREATE TABLE IF NOT EXISTS skill_tags (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS skill_tag_links (
  skill_id TEXT NOT NULL,
  tag_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (skill_id, tag_id),
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE,
  FOREIGN KEY(tag_id) REFERENCES skill_tags(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  enabled_tool_keys TEXT NOT NULL,
  local_git_exclude_enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_scanned_at INTEGER NULL
);

CREATE TABLE IF NOT EXISTS project_skill_links (
  project_id TEXT NOT NULL,
  skill_id TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  desired_tools TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (project_id, skill_id),
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_discovered_skills (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  name TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  absolute_path TEXT NOT NULL,
  content_hash TEXT NULL,
  tool_keys TEXT NOT NULL,
  status TEXT NOT NULL,
  matched_skill_id TEXT NULL,
  first_seen_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
  FOREIGN KEY(matched_skill_id) REFERENCES skills(id) ON DELETE SET NULL,
  UNIQUE(project_id, absolute_path)
);

CREATE INDEX IF NOT EXISTS idx_project_discovered_project_name
ON project_discovered_skills(project_id, name);

CREATE TABLE IF NOT EXISTS skill_assignments (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  scope TEXT NOT NULL,
  project_id TEXT NULL,
  ownership TEXT NOT NULL,
  state TEXT NOT NULL,
  last_error TEXT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_assignments_unique_scope
ON skill_assignments(skill_id, tool, scope, COALESCE(project_id, ''));

CREATE TABLE IF NOT EXISTS ignored_issues (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL,
  scope_id TEXT NULL,
  issue_key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  ignored_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ignored_issues_instance
ON ignored_issues(scope, COALESCE(scope_id, ''), issue_key, fingerprint);

CREATE TABLE IF NOT EXISTS prompt_revisions (
  id TEXT PRIMARY KEY,
  logical_path TEXT NOT NULL,
  real_path TEXT NULL,
  tool TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  project_id TEXT NULL,
  operation TEXT NOT NULL,
  content BLOB NOT NULL,
  content_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prompt_revisions_path_created
ON prompt_revisions(logical_path, created_at DESC);
"#;

#[derive(Clone, Debug)]
pub struct SkillStore {
    db_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_subpath: Option<String>,
    pub source_revision: Option<String>,
    pub central_path: String,
    pub content_hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_sync_at: Option<i64>,
    pub last_seen_at: i64,
    pub enabled: bool,
    pub status: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct TrashMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourceBaseline {
    source_type: String,
    source_ref: String,
    source_subpath: Option<String>,
    content_hash: String,
}

impl SourceBaseline {
    pub(crate) fn from_skill(skill: &SkillRecord, content_hash: String) -> Option<Self> {
        let source_ref = skill
            .source_ref
            .as_deref()
            .filter(|value| !value.trim().is_empty())?
            .to_string();
        Some(Self {
            source_type: skill.source_type.clone(),
            source_ref,
            source_subpath: skill.source_subpath.clone(),
            content_hash,
        })
    }

    pub(crate) fn matches(&self, skill: &SkillRecord, content_hash: &str) -> bool {
        self.source_type == skill.source_type
            && skill.source_ref.as_deref() == Some(self.source_ref.as_str())
            && self.source_subpath == skill.source_subpath
            && self.content_hash == content_hash
    }
}

impl SkillRecord {
    pub fn has_unbound_local_source(&self) -> bool {
        self.source_type == "local"
            && self
                .source_ref
                .as_deref()
                .map_or(true, |source| source.trim().is_empty())
    }

    pub fn external_local_source(&self) -> Option<&str> {
        self.source_ref.as_deref().filter(|source| {
            self.source_type == "local"
                && !source.trim().is_empty()
                && Path::new(source) != Path::new(&self.central_path)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillTargetRecord {
    pub id: String,
    pub skill_id: String,
    pub tool: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub target_path: String,
    pub mode: String,
    pub status: String,
    pub last_error: Option<String>,
    pub synced_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillAssignmentRecord {
    pub id: String,
    pub skill_id: String,
    pub tool: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub ownership: String,
    pub state: String,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoredIssueRecord {
    pub id: String,
    pub scope: String,
    pub scope_id: Option<String>,
    pub issue_key: String,
    pub fingerprint: String,
    pub ignored_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub enabled_tool_keys: Vec<String>,
    pub local_git_exclude_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_scanned_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSkillLinkRecord {
    pub project_id: String,
    pub skill_id: String,
    pub enabled: bool,
    pub desired_tools: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRevisionRecord {
    pub id: String,
    pub logical_path: String,
    pub real_path: Option<String>,
    pub tool: String,
    pub profile_id: String,
    pub project_id: Option<String>,
    pub operation: String,
    pub content: Vec<u8>,
    pub content_hash: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTemplateRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub mode: String,
    pub archived: bool,
    pub current_revision_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTemplateRevisionRecord {
    pub id: String,
    pub template_id: String,
    pub revision_number: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTemplateRevisionItemRecord {
    pub revision_id: String,
    pub skill_id: String,
    pub skill_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectTemplateBindingRecord {
    pub project_id: String,
    pub template_id: String,
    pub applied_revision_id: String,
    pub sync_error: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub synced_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSkillOriginRecord {
    pub project_id: String,
    pub skill_id: String,
    pub source_type: String,
    pub source_id: String,
    pub desired_tools: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectTemplateOverrideRecord {
    pub project_id: String,
    pub skill_id: String,
    pub kind: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectTemplateFailureRecord {
    pub project_id: String,
    pub template_id: String,
    pub skill_id: String,
    pub tool: String,
    pub kind: String,
    pub error: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectDiscoveredSkillRecord {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub content_hash: Option<String>,
    pub tool_keys: Vec<String>,
    pub status: String,
    pub matched_skill_id: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagRecord {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagWithCountRecord {
    pub id: i64,
    pub name: String,
    pub skill_count: i64,
    pub updated_at: i64,
}

impl SkillStore {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    #[allow(dead_code)]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn ensure_schema(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;

            let user_version: i32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
            if user_version == 0 {
                conn.execute_batch(SCHEMA_V1)?;
                // V2: add description column
                conn.execute_batch("ALTER TABLE skills ADD COLUMN description TEXT NULL;")?;
                // V3: add source_subpath column
                conn.execute_batch("ALTER TABLE skills ADD COLUMN source_subpath TEXT NULL;")?;
                migrate_skill_targets_to_v4(conn)?;
                migrate_assignments_to_v8(conn)?;
                migrate_prompt_revisions_to_v9(conn)?;
                migrate_skill_templates_to_v10(conn)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if user_version < SCHEMA_VERSION {
                // Incremental migrations
                if user_version < 2 {
                    conn.execute_batch("ALTER TABLE skills ADD COLUMN description TEXT NULL;")?;
                }
                if user_version < 3 {
                    conn.execute_batch("ALTER TABLE skills ADD COLUMN source_subpath TEXT NULL;")?;
                }
                if user_version < 4 {
                    migrate_skill_targets_to_v4(conn)?;
                }
                if user_version < 5 {
                    migrate_tags_to_v5(conn)?;
                }
                if user_version < 6 {
                    migrate_skill_enabled_to_v6(conn)?;
                }
                if user_version < 7 {
                    migrate_projects_to_v7(conn)?;
                }
                if user_version < 8 {
                    migrate_assignments_to_v8(conn)?;
                }
                if user_version < 9 {
                    migrate_prompt_revisions_to_v9(conn)?;
                }
                if user_version < 10 {
                    migrate_skill_templates_to_v10(conn)?;
                }
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if user_version > SCHEMA_VERSION {
                anyhow::bail!(
                    "database schema version {} is newer than app supports {}",
                    user_version,
                    SCHEMA_VERSION
                );
            }

            conn.execute_batch(DEVICE_SYNC_SCHEMA_V1)?;
            ensure_recycle_bin_schema(conn)?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS skill_source_checks (
                    skill_id TEXT PRIMARY KEY REFERENCES skills(id) ON DELETE CASCADE,
                    error_code TEXT NULL,
                    checked_at INTEGER NOT NULL
                );",
            )?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO NOTHING",
                params![DEVICE_SYNC_SCHEMA_VERSION_KEY, DEVICE_SYNC_SCHEMA_VERSION],
            )?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO NOTHING",
                params![RECYCLE_BIN_SCHEMA_VERSION_KEY, RECYCLE_BIN_SCHEMA_VERSION],
            )?;

            Ok(())
        })
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = stmt.query(params![key])?;
            Ok(rows
                .next()?
                .map(|row| row.get::<_, String>(0))
                .transpose()?)
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    #[allow(dead_code)]
    pub fn set_onboarding_completed(&self, completed: bool) -> Result<()> {
        self.set_setting(
            "onboarding_completed",
            if completed { "true" } else { "false" },
        )
    }

    pub fn upsert_skill(&self, record: &SkillRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO skills (
          id, name, description, source_type, source_ref, source_subpath, source_revision, central_path, content_hash,
          created_at, updated_at, last_sync_at, last_seen_at, enabled, status
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
          ?10, ?11, ?12, ?13, ?14, ?15
        )
        ON CONFLICT(id) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          source_type = excluded.source_type,
          source_ref = excluded.source_ref,
          source_subpath = excluded.source_subpath,
          source_revision = excluded.source_revision,
          central_path = excluded.central_path,
          content_hash = excluded.content_hash,
          created_at = excluded.created_at,
          updated_at = excluded.updated_at,
          last_sync_at = excluded.last_sync_at,
          last_seen_at = excluded.last_seen_at,
          enabled = excluded.enabled,
          status = excluded.status",
                params![
                    record.id,
                    record.name,
                    record.description,
                    record.source_type,
                    record.source_ref,
                    record.source_subpath,
                    record.source_revision,
                    record.central_path,
                    record.content_hash,
                    record.created_at,
                    record.updated_at,
                    record.last_sync_at,
                    record.last_seen_at,
                    record.enabled as i32,
                    record.status
                ],
            )?;
            Ok(())
        })
    }

    pub fn upsert_skill_target(&self, record: &SkillTargetRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO skill_targets (
          id, skill_id, tool, scope, project_path, target_path, mode, status, last_error, synced_at
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
        )
        ON CONFLICT DO UPDATE SET
          target_path = excluded.target_path,
          mode = excluded.mode,
          status = excluded.status,
          last_error = excluded.last_error,
          synced_at = excluded.synced_at",
                params![
                    record.id,
                    record.skill_id,
                    record.tool,
                    record.scope,
                    record.project_path,
                    record.target_path,
                    record.mode,
                    record.status,
                    record.last_error,
                    record.synced_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_skills(&self) -> Result<Vec<SkillRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
        "SELECT id, name, description, source_type, source_ref, source_subpath, source_revision, central_path, content_hash,
                created_at, updated_at, last_sync_at, last_seen_at, enabled, status
         FROM skills
         ORDER BY updated_at DESC",
      )?;
            let rows = stmt.query_map([], |row| {
                Ok(SkillRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    source_type: row.get(3)?,
                    source_ref: row.get(4)?,
                    source_subpath: row.get(5)?,
                    source_revision: row.get(6)?,
                    central_path: row.get(7)?,
                    content_hash: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                    last_sync_at: row.get(11)?,
                    last_seen_at: row.get(12)?,
                    enabled: row.get::<_, i32>(13)? != 0,
                    status: row.get(14)?,
                })
            })?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn get_skill_by_id(&self, skill_id: &str) -> Result<Option<SkillRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
        "SELECT id, name, description, source_type, source_ref, source_subpath, source_revision, central_path, content_hash,
                created_at, updated_at, last_sync_at, last_seen_at, enabled, status
         FROM skills
         WHERE id = ?1
         LIMIT 1",
      )?;
            let mut rows = stmt.query(params![skill_id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(SkillRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    source_type: row.get(3)?,
                    source_ref: row.get(4)?,
                    source_subpath: row.get(5)?,
                    source_revision: row.get(6)?,
                    central_path: row.get(7)?,
                    content_hash: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                    last_sync_at: row.get(11)?,
                    last_seen_at: row.get(12)?,
                    enabled: row.get::<_, i32>(13)? != 0,
                    status: row.get(14)?,
                }))
            } else {
                Ok(None)
            }
        })
    }

    pub fn update_skill_description(
        &self,
        skill_id: &str,
        description: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE skills SET description = ?1 WHERE id = ?2",
                params![description, skill_id],
            )?;
            Ok(())
        })
    }

    pub fn update_skill_content_hash(&self, skill_id: &str, content_hash: &str) -> Result<bool> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE skills SET content_hash = ?1
                 WHERE id = ?2 AND (content_hash IS NULL OR content_hash <> ?1)",
                params![content_hash, skill_id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn clear_skill_content_hash(&self, skill_id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE skills SET content_hash = NULL
                 WHERE id = ?1 AND content_hash IS NOT NULL",
                params![skill_id],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn set_skill_enabled(&self, skill_id: &str, enabled: bool) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE skills SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i32, now_ms(), skill_id],
            )?;
            if changed == 0 {
                anyhow::bail!("skill not found: {}", skill_id);
            }
            Ok(())
        })
    }

    pub fn suspend_skill_atomically(&self, skill_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result = (|| -> Result<()> {
                let changed = conn.execute(
                    "UPDATE skills SET enabled = 0, updated_at = ?2 WHERE id = ?1",
                    params![skill_id, now_ms()],
                )?;
                if changed == 0 {
                    anyhow::bail!("skill not found: {skill_id}");
                }
                conn.execute(
                    "UPDATE skill_assignments
                     SET state = 'suspended', last_error = NULL, updated_at = ?2
                     WHERE skill_id = ?1",
                    params![skill_id, now_ms()],
                )?;
                conn.execute(
                    "UPDATE skill_targets
                     SET status = 'disabled', last_error = NULL
                     WHERE skill_id = ?1",
                    params![skill_id],
                )?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn delete_skill(&self, skill_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM skills WHERE id = ?1", params![skill_id])?;
            tx.execute(
                "DELETE FROM settings WHERE key = ?1",
                params![format!("device_sync.shared_source.{skill_id}")],
            )?;
            tx.execute(
                "DELETE FROM settings WHERE key = ?1",
                params![format!("device_sync.source_baseline.{skill_id}")],
            )?;
            tx.execute(
                "DELETE FROM settings WHERE key = ?1",
                params![format!("device_sync.source_origin.{skill_id}")],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn create_tag(&self, name: &str) -> Result<TagRecord> {
        let normalized = normalize_tag_name(name)?;
        self.with_conn(|conn| {
            let now = now_ms();
            conn.execute(
                "INSERT INTO skill_tags (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
                params![normalized, now],
            )
            .with_context(|| format!("tag already exists: {}", normalized))?;
            let id = conn.last_insert_rowid();
            Ok(TagRecord {
                id,
                name: normalized,
            })
        })
    }

    pub fn rename_tag(&self, tag_id: i64, name: &str) -> Result<TagRecord> {
        let normalized = normalize_tag_name(name)?;
        self.with_conn(|conn| {
            let changed = conn
                .execute(
                    "UPDATE skill_tags SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![normalized, now_ms(), tag_id],
                )
                .with_context(|| format!("tag already exists: {}", normalized))?;
            if changed == 0 {
                anyhow::bail!("tag not found: {}", tag_id);
            }
            Ok(TagRecord {
                id: tag_id,
                name: normalized,
            })
        })
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM skill_tags WHERE id = ?1", params![tag_id])?;
            Ok(())
        })
    }

    pub fn list_tags_with_counts(&self) -> Result<Vec<TagWithCountRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name, COUNT(l.skill_id) AS skill_count,
                        COALESCE(MAX(l.created_at), t.updated_at) AS last_used_at
                 FROM skill_tags t
                 LEFT JOIN skill_tag_links l ON l.tag_id = t.id
                 GROUP BY t.id, t.name, t.updated_at
                 ORDER BY LOWER(t.name) ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(TagWithCountRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    skill_count: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn get_skill_tags(&self, skill_id: &str) -> Result<Vec<TagRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name
                 FROM skill_tags t
                 INNER JOIN skill_tag_links l ON l.tag_id = t.id
                 WHERE l.skill_id = ?1
                 ORDER BY LOWER(t.name) ASC",
            )?;
            let rows = stmt.query_map(params![skill_id], |row| {
                Ok(TagRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn set_skill_tags(&self, skill_id: &str, tag_ids: &[i64]) -> Result<()> {
        self.with_conn(|conn| {
            let now = now_ms();
            conn.execute_batch("BEGIN;")?;
            let result = (|| -> Result<()> {
                conn.execute(
                    "DELETE FROM skill_tag_links WHERE skill_id = ?1",
                    params![skill_id],
                )?;
                for tag_id in tag_ids {
                    conn.execute(
                        "INSERT OR IGNORE INTO skill_tag_links (skill_id, tag_id, created_at)
                         VALUES (?1, ?2, ?3)",
                        params![skill_id, tag_id, now],
                    )?;
                }
                Ok(())
            })();

            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn list_untagged_skill_ids(&self) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.id
                 FROM skills s
                 WHERE NOT EXISTS (
                   SELECT 1 FROM skill_tag_links l WHERE l.skill_id = s.id
                 )
                 ORDER BY s.updated_at DESC",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn list_skill_targets(&self, skill_id: &str) -> Result<Vec<SkillTargetRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, tool, scope, project_path, target_path, mode, status, last_error, synced_at
         FROM skill_targets
         WHERE skill_id = ?1
         ORDER BY tool ASC, scope ASC, project_path ASC",
            )?;
            let rows = stmt.query_map(params![skill_id], |row| {
                Ok(SkillTargetRecord {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    tool: row.get(2)?,
                    scope: row.get(3)?,
                    project_path: row.get(4)?,
                    target_path: row.get(5)?,
                    mode: row.get(6)?,
                    status: row.get(7)?,
                    last_error: row.get(8)?,
                    synced_at: row.get(9)?,
                })
            })?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn list_all_skill_target_paths(&self) -> Result<Vec<(String, String)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT tool, target_path
         FROM skill_targets",
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
    }

    pub fn get_skill_target(
        &self,
        skill_id: &str,
        tool: &str,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<Option<SkillTargetRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, tool, scope, project_path, target_path, mode, status, last_error, synced_at
         FROM skill_targets
         WHERE skill_id = ?1
           AND tool = ?2
           AND scope = ?3
           AND ((?4 IS NULL AND project_path IS NULL) OR project_path = ?4)",
            )?;
            let mut rows = stmt.query(params![skill_id, tool, scope, project_path])?;
            if let Some(row) = rows.next()? {
                Ok(Some(SkillTargetRecord {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    tool: row.get(2)?,
                    scope: row.get(3)?,
                    project_path: row.get(4)?,
                    target_path: row.get(5)?,
                    mode: row.get(6)?,
                    status: row.get(7)?,
                    last_error: row.get(8)?,
                    synced_at: row.get(9)?,
                }))
            } else {
                Ok(None)
            }
        })
    }

    pub fn delete_skill_target(
        &self,
        skill_id: &str,
        tool: &str,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM skill_targets
                 WHERE skill_id = ?1
                   AND tool = ?2
                   AND scope = ?3
                   AND ((?4 IS NULL AND project_path IS NULL) OR project_path = ?4)",
                params![skill_id, tool, scope, project_path],
            )?;
            Ok(())
        })
    }

    pub fn upsert_skill_assignment(&self, record: &SkillAssignmentRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO skill_assignments (
                   id, skill_id, tool, scope, project_id, ownership, state,
                   last_error, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT DO UPDATE SET
                   ownership = excluded.ownership,
                   state = excluded.state,
                   last_error = excluded.last_error,
                   updated_at = excluded.updated_at",
                params![
                    record.id,
                    record.skill_id,
                    record.tool,
                    record.scope,
                    record.project_id,
                    record.ownership,
                    record.state,
                    record.last_error,
                    record.created_at,
                    record.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_skill_assignment(
        &self,
        skill_id: &str,
        tool: &str,
        scope: &str,
        project_id: Option<&str>,
    ) -> Result<Option<SkillAssignmentRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, skill_id, tool, scope, project_id, ownership, state,
                        last_error, created_at, updated_at
                 FROM skill_assignments
                 WHERE skill_id = ?1 AND tool = ?2 AND scope = ?3
                   AND ((?4 IS NULL AND project_id IS NULL) OR project_id = ?4)",
                params![skill_id, tool, scope, project_id],
                skill_assignment_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn list_skill_assignments(&self, skill_id: &str) -> Result<Vec<SkillAssignmentRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, tool, scope, project_id, ownership, state,
                        last_error, created_at, updated_at
                 FROM skill_assignments
                 WHERE skill_id = ?1
                 ORDER BY scope ASC, project_id ASC, tool ASC",
            )?;
            let rows = stmt.query_map(params![skill_id], skill_assignment_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn delete_skill_assignment(
        &self,
        skill_id: &str,
        tool: &str,
        scope: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM skill_assignments
                 WHERE skill_id = ?1 AND tool = ?2 AND scope = ?3
                   AND ((?4 IS NULL AND project_id IS NULL) OR project_id = ?4)",
                params![skill_id, tool, scope, project_id],
            )?;
            Ok(())
        })
    }

    pub fn upsert_ignored_issue(&self, record: &IgnoredIssueRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM ignored_issues
                 WHERE scope = ?1
                   AND ((?2 IS NULL AND scope_id IS NULL) OR scope_id = ?2)
                   AND issue_key = ?3
                   AND fingerprint <> ?4",
                params![
                    record.scope,
                    record.scope_id,
                    record.issue_key,
                    record.fingerprint
                ],
            )?;
            conn.execute(
                "INSERT INTO ignored_issues (
                   id, scope, scope_id, issue_key, fingerprint, ignored_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT DO UPDATE SET ignored_at = excluded.ignored_at",
                params![
                    record.id,
                    record.scope,
                    record.scope_id,
                    record.issue_key,
                    record.fingerprint,
                    record.ignored_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn is_issue_ignored(
        &self,
        scope: &str,
        scope_id: Option<&str>,
        issue_key: &str,
        fingerprint: &str,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ignored_issues
                 WHERE scope = ?1
                   AND ((?2 IS NULL AND scope_id IS NULL) OR scope_id = ?2)
                   AND issue_key = ?3 AND fingerprint = ?4",
                params![scope, scope_id, issue_key, fingerprint],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    pub fn get_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, name, root_path, enabled_tool_keys, local_git_exclude_enabled,
                        created_at, updated_at, last_scanned_at
                 FROM projects WHERE id = ?1",
                params![project_id],
                project_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn get_project_by_root(&self, root_path: &str) -> Result<Option<ProjectRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, name, root_path, enabled_tool_keys, local_git_exclude_enabled,
                        created_at, updated_at, last_scanned_at
                 FROM projects WHERE root_path = ?1",
                params![root_path],
                project_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, root_path, enabled_tool_keys, local_git_exclude_enabled,
                        created_at, updated_at, last_scanned_at
                 FROM projects
                 ORDER BY updated_at DESC, name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map([], project_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn insert_project(&self, project: &ProjectRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO projects (
                   id, name, root_path, enabled_tool_keys, local_git_exclude_enabled,
                   created_at, updated_at, last_scanned_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project.id,
                    project.name,
                    project.root_path,
                    string_vec_to_json(&project.enabled_tool_keys)?,
                    project.local_git_exclude_enabled as i32,
                    project.created_at,
                    project.updated_at,
                    project.last_scanned_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_project(&self, project_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result = (|| -> Result<()> {
                conn.execute(
                    "DELETE FROM ignored_issues WHERE scope = 'project' AND scope_id = ?1",
                    params![project_id],
                )?;
                let changed =
                    conn.execute("DELETE FROM projects WHERE id = ?1", params![project_id])?;
                if changed == 0 {
                    anyhow::bail!("project not found: {project_id}");
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn update_project(&self, project: &ProjectRecord) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE projects
                 SET name = ?2, root_path = ?3, enabled_tool_keys = ?4,
                     local_git_exclude_enabled = ?5, updated_at = ?6, last_scanned_at = ?7
                 WHERE id = ?1",
                params![
                    project.id,
                    project.name,
                    project.root_path,
                    string_vec_to_json(&project.enabled_tool_keys)?,
                    project.local_git_exclude_enabled as i32,
                    project.updated_at,
                    project.last_scanned_at
                ],
            )?;
            if changed == 0 {
                anyhow::bail!("project not found: {}", project.id);
            }
            Ok(())
        })
    }

    pub fn upsert_project_skill_link(&self, link: &ProjectSkillLinkRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO project_skill_links (
                   project_id, skill_id, enabled, desired_tools, status, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(project_id, skill_id) DO UPDATE SET
                   enabled = excluded.enabled,
                   desired_tools = excluded.desired_tools,
                   status = excluded.status,
                   updated_at = excluded.updated_at",
                params![
                    link.project_id,
                    link.skill_id,
                    link.enabled as i32,
                    string_vec_to_json(&link.desired_tools)?,
                    link.status,
                    link.created_at,
                    link.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_project_skill_link(
        &self,
        project_id: &str,
        skill_id: &str,
    ) -> Result<Option<ProjectSkillLinkRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT project_id, skill_id, enabled, desired_tools, status, created_at, updated_at
                 FROM project_skill_links WHERE project_id = ?1 AND skill_id = ?2",
                params![project_id, skill_id],
                project_link_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn list_project_skill_links(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectSkillLinkRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, skill_id, enabled, desired_tools, status, created_at, updated_at
                 FROM project_skill_links WHERE project_id = ?1
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map(params![project_id], project_link_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_project_discovered_skills(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectDiscoveredSkillRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, name, relative_path, absolute_path, content_hash,
                        tool_keys, status, matched_skill_id, first_seen_at, last_seen_at
                 FROM project_discovered_skills
                 WHERE project_id = ?1
                 ORDER BY name COLLATE NOCASE ASC, relative_path ASC",
            )?;
            let rows = stmt.query_map(params![project_id], project_discovered_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn replace_project_discovered_skills(
        &self,
        project_id: &str,
        scanned_at: i64,
        records: &[ProjectDiscoveredSkillRecord],
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result = (|| -> Result<()> {
                for record in records {
                    conn.execute(
                        "INSERT INTO project_discovered_skills (
                           id, project_id, name, relative_path, absolute_path, content_hash,
                           tool_keys, status, matched_skill_id, first_seen_at, last_seen_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                         ON CONFLICT(project_id, absolute_path) DO UPDATE SET
                           name = excluded.name,
                           relative_path = excluded.relative_path,
                           content_hash = excluded.content_hash,
                           tool_keys = excluded.tool_keys,
                           status = excluded.status,
                           matched_skill_id = excluded.matched_skill_id,
                           last_seen_at = excluded.last_seen_at",
                        params![
                            record.id,
                            record.project_id,
                            record.name,
                            record.relative_path,
                            record.absolute_path,
                            record.content_hash,
                            string_vec_to_json(&record.tool_keys)?,
                            record.status,
                            record.matched_skill_id,
                            record.first_seen_at,
                            record.last_seen_at
                        ],
                    )?;
                }
                conn.execute(
                    "DELETE FROM project_discovered_skills
                     WHERE project_id = ?1 AND last_seen_at <> ?2",
                    params![project_id, scanned_at],
                )?;
                conn.execute(
                    "UPDATE projects SET last_scanned_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![project_id, scanned_at],
                )?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn list_project_skill_targets(&self, project_root: &str) -> Result<Vec<SkillTargetRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, tool, scope, project_path, target_path, mode, status,
                        last_error, synced_at
                 FROM skill_targets
                 WHERE scope = 'project' AND project_path = ?1
                 ORDER BY target_path ASC, tool ASC",
            )?;
            let rows = stmt.query_map(params![project_root], |row| {
                Ok(SkillTargetRecord {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    tool: row.get(2)?,
                    scope: row.get(3)?,
                    project_path: row.get(4)?,
                    target_path: row.get(5)?,
                    mode: row.get(6)?,
                    status: row.get(7)?,
                    last_error: row.get(8)?,
                    synced_at: row.get(9)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
            Ok(())
        })
    }

    pub fn get_device_sync_config(&self) -> Result<Option<DeviceSyncConfig>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT config_json FROM device_sync_config WHERE id = 1 LIMIT 1")?;
            let mut rows = stmt.query([])?;
            rows.next()?
                .map(|row| row.get::<_, String>(0))
                .transpose()?
                .map(|json| serde_json::from_str(&json).context("decode device sync config"))
                .transpose()
        })
    }

    pub fn save_device_sync_config(&self, config: &DeviceSyncConfig) -> Result<()> {
        if let Some(schedule) = &config.auto_sync_schedule {
            schedule.validate()?;
        }
        let json = serde_json::to_string(config)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO device_sync_config (id, config_json, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET
                   config_json = excluded.config_json,
                   updated_at = excluded.updated_at",
                params![json, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn migrate_device_sync_startup_credential_consent(&self) -> Result<()> {
        if self
            .get_setting(DEVICE_SYNC_STARTUP_CREDENTIAL_CONSENT_MIGRATION)?
            .is_some()
        {
            return Ok(());
        }

        if let Some(mut config) = self.get_device_sync_config()? {
            config.auto_check = false;
            self.save_device_sync_config(&config)?;
        }
        self.set_setting(DEVICE_SYNC_STARTUP_CREDENTIAL_CONSENT_MIGRATION, "complete")
    }

    pub fn clear_device_sync_config(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM device_sync_config WHERE id = 1", [])?;
            Ok(())
        })
    }

    pub fn clear_device_sync_repository_state(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                "DELETE FROM device_sync_devices;
                 DELETE FROM device_sync_conflicts;
                 DELETE FROM device_sync_runs;
                 DELETE FROM settings WHERE key GLOB 'device_sync.run_items.*';
                 DELETE FROM settings WHERE key GLOB 'device_sync.shared_source.*';",
            )?;
            Ok(())
        })
    }

    pub fn start_device_sync_run(&self, id: &str, started_at: i64) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM settings
                 WHERE key IN (
                   SELECT 'device_sync.run_items.' || id
                   FROM device_sync_runs
                   WHERE finished_at IS NULL
                 )",
                [],
            )?;
            tx.execute("DELETE FROM device_sync_runs WHERE finished_at IS NULL", [])?;
            tx.execute(
                "INSERT INTO device_sync_runs (id, started_at, status)
                 VALUES (?1, ?2, 'running')",
                params![id, started_at],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_device_sync_run(
        &self,
        id: &str,
        finished_at: i64,
        status: &str,
        added: usize,
        updated: usize,
        deleted: usize,
        conflicted: usize,
        commit: Option<&str>,
        error: Option<&str>,
        items: Option<&[crate::core::device_sync::types::SyncChangeItem]>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let unchanged = status == "success"
                && added == 0
                && updated == 0
                && deleted == 0
                && conflicted == 0;
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('device_sync_last_run', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![serde_json::to_string(&(
                    if unchanged { "unchanged" } else { status },
                    finished_at
                ))?],
            )?;
            if unchanged {
                tx.execute("DELETE FROM device_sync_runs WHERE id = ?1", params![id])?;
                prune_device_sync_history(&tx)?;
                tx.commit()?;
                return Ok(());
            }
            tx.execute(
                "UPDATE device_sync_runs SET
                   finished_at = ?2, status = ?3, added = ?4, updated = ?5,
                   deleted = ?6, conflicted = ?7, commit_hash = ?8, error = ?9
                 WHERE id = ?1",
                params![
                    id,
                    finished_at,
                    status,
                    added as i64,
                    updated as i64,
                    deleted as i64,
                    conflicted as i64,
                    commit,
                    error.map(crate::core::device_sync::errors::safe_message)
                ],
            )?;
            if let Some(items) = items {
                tx.execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("device_sync.run_items.{id}"), serde_json::to_string(items)?])?;
            }
            prune_device_sync_history(&tx)?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn list_device_sync_history(&self, limit: usize) -> Result<Vec<SyncHistoryEntry>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, started_at, finished_at, status, added, updated, deleted,
                        conflicted, commit_hash, error
                 FROM device_sync_runs
                 WHERE finished_at IS NOT NULL
                 ORDER BY started_at DESC, id DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], |row| {
                let id: String = row.get(0)?;
                let raw: Option<String> = conn
                    .query_row(
                        "SELECT value FROM settings WHERE key=?1",
                        params![format!("device_sync.run_items.{id}")],
                        |row| row.get(0),
                    )
                    .optional()?;
                let items = raw
                    .map(|raw| {
                        serde_json::from_str(&raw).map_err(|err| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(err),
                            )
                        })
                    })
                    .transpose()?;
                Ok(SyncHistoryEntry {
                    items,
                    id,
                    started_at: row.get(1)?,
                    finished_at: row.get(2)?,
                    status: row.get(3)?,
                    added: row.get::<_, i64>(4)? as usize,
                    updated: row.get::<_, i64>(5)? as usize,
                    deleted: row.get::<_, i64>(6)? as usize,
                    conflicted: row.get::<_, i64>(7)? as usize,
                    commit: row.get(8)?,
                    error: row
                        .get::<_, Option<String>>(9)?
                        .map(|error| crate::core::device_sync::errors::safe_message(&error)),
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
    }

    pub fn upsert_device_sync_device(&self, device: &DeviceSyncDevice) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO device_sync_devices (id, name, last_commit, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   last_commit = excluded.last_commit,
                   last_seen_at = MAX(device_sync_devices.last_seen_at, excluded.last_seen_at)",
                params![
                    device.id,
                    device.name,
                    device.last_commit,
                    device.last_seen_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_device_sync_devices(&self, current_id: &str) -> Result<Vec<DeviceSyncDevice>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT device.id, COALESCE(alias.value, device.name), NULL, device.last_commit, device.last_seen_at
                 FROM device_sync_devices AS device
                 LEFT JOIN settings AS alias
                   ON alias.key = 'device_sync.device_alias.' || device.id AND device.id = ?1
                 ORDER BY CASE WHEN id = ?1 THEN 0 ELSE 1 END, last_seen_at DESC",
            )?;
            let rows = stmt.query_map(params![current_id], |row| {
                let id: String = row.get(0)?;
                Ok(DeviceSyncDevice {
                    is_current: id == current_id,
                    id,
                    name: row.get(1)?,
                    alias: row.get(2)?,
                    last_commit: row.get(3)?,
                    last_seen_at: row.get(4)?,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
    }

    pub fn set_device_sync_device_alias(&self, device_id: &str, alias: Option<&str>) -> Result<()> {
        if self.get_setting("device_sync.local_device_id")?.as_deref() != Some(device_id) {
            bail!("only the current device can be renamed");
        }
        self.with_conn(|conn| {
            let exists = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM device_sync_devices WHERE id = ?1)",
                params![device_id],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                bail!("device sync device not found");
            }
            let key = format!("device_sync.device_alias.{device_id}");
            let normalized = alias.map(str::trim).filter(|value| !value.is_empty());
            if normalized.is_some_and(|value| value.chars().count() > 80) {
                bail!("device alias must be 80 characters or fewer");
            }
            match normalized {
                Some(value) => {
                    conn.execute(
                        "INSERT INTO settings (key, value) VALUES (?1, ?2)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        params![key, value],
                    )?;
                }
                None => {
                    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
                }
            }
            Ok(())
        })
    }

    pub fn upsert_device_sync_conflict(&self, conflict: &SyncConflict) -> Result<()> {
        let files_json = serde_json::to_string(&conflict.files)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO device_sync_conflicts (
                   id, skill_id, skill_name, base_commit, local_commit, remote_commit,
                   files_json, created_at, status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                   files_json = excluded.files_json,
                   status = excluded.status",
                params![
                    conflict.id,
                    conflict.skill_id,
                    conflict.skill_name,
                    conflict.base_commit,
                    conflict.local_commit,
                    conflict.remote_commit,
                    files_json,
                    conflict.created_at,
                    conflict.status
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_device_sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, skill_name, base_commit, local_commit, remote_commit,
                        files_json, created_at, status
                 FROM device_sync_conflicts
                 WHERE status = 'pending'
                 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                let files_json: String = row.get(6)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    files_json,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?;
            let mut items = Vec::new();
            for row in rows {
                let (
                    id,
                    skill_id,
                    skill_name,
                    base_commit,
                    local_commit,
                    remote_commit,
                    files,
                    created_at,
                    status,
                ) = row?;
                items.push(SyncConflict {
                    id,
                    skill_id,
                    skill_name,
                    base_commit,
                    local_commit,
                    remote_commit,
                    files: serde_json::from_str(&files).context("decode conflict files")?,
                    created_at,
                    status,
                });
            }
            Ok(items)
        })
    }

    pub fn resolve_device_sync_conflict_with_state(
        &self,
        id: &str,
        key: &str,
        value: &str,
        changes: &[crate::core::device_sync::types::SyncChangeItem],
        commit: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
            let changed = tx.execute("UPDATE device_sync_conflicts SET status = 'resolved' WHERE id = ?1 AND status = 'pending'", params![id])?;
            anyhow::ensure!(changed == 1, "device sync conflict is no longer pending");
            if !changes.is_empty() {
                let previous_run: Option<(String, i64)> = tx.query_row(
                    "SELECT status, COALESCE(finished_at, started_at) FROM device_sync_runs WHERE status != 'resolved' ORDER BY started_at DESC, id DESC LIMIT 1",
                    [], |row| Ok((row.get(0)?, row.get(1)?)),
                ).optional()?;
                if let Some(previous_run) = previous_run {
                    tx.execute("INSERT OR IGNORE INTO settings (key,value) VALUES ('device_sync_last_run',?1)", params![serde_json::to_string(&previous_run)?])?;
                }
                let run_id = format!("resolution-{id}");
                let now = now_ms();
                let count = |kind: &str| changes.iter().filter(|item| item.kind == kind).count() as i64;
                tx.execute("INSERT INTO device_sync_runs (id,started_at,finished_at,status,added,updated,deleted,conflicted,commit_hash) VALUES (?1,?2,?2,'resolved',?3,?4,?5,0,?6)", params![run_id, now, count("added"), count("updated"), count("deleted"), commit])?;
                tx.execute("INSERT INTO settings (key,value) VALUES (?1,?2)", params![format!("device_sync.run_items.{run_id}"), serde_json::to_string(changes)?])?;
            }
            prune_device_sync_history(&tx)?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn resolve_device_sync_conflicts_and_save_config_if_clear(
        &self,
        ids: &[String],
        config: &DeviceSyncConfig,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config)?;
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            for id in ids {
                transaction.execute(
                    "UPDATE device_sync_conflicts SET status = 'resolved'
                     WHERE id = ?1 AND status = 'pending'",
                    params![id],
                )?;
            }
            let pending: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM device_sync_conflicts WHERE status = 'pending'",
                [],
                |row| row.get(0),
            )?;
            if pending == 0 {
                transaction.execute(
                    "INSERT INTO device_sync_config (id, config_json, updated_at)
                     VALUES (1, ?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET
                       config_json = excluded.config_json,
                       updated_at = excluded.updated_at",
                    params![config_json, now_ms()],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    #[allow(dead_code)]
    pub fn add_device_sync_trash(&self, entry: &TrashEntry) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT OR REPLACE INTO device_sync_tombstones
                 (id, skill_id, skill_name, trash_path, deleted_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.id,
                    entry.skill_id,
                    entry.skill_name,
                    entry.trash_path,
                    entry.deleted_at,
                    entry.expires_at
                ],
            )?;
            let description: Option<Option<String>> = tx.query_row("SELECT description FROM skills WHERE id=?1", params![entry.skill_id], |row| row.get(0)).optional()?;
            if let Some(description) = description {
                let mut statement = tx.prepare("SELECT t.name FROM skill_tags t JOIN skill_tag_links l ON t.id=l.tag_id WHERE l.skill_id=?1 ORDER BY t.name")?;
                let tags = statement.query_map(params![entry.skill_id], |row| row.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
                let metadata = TrashMetadata { description, tags };
                tx.execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("device_sync.trash_metadata.{}", entry.id), serde_json::to_string(&metadata)?])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub(crate) fn commit_recycle_bin_archive(
        &self,
        entry: &TrashEntry,
        deletion_source: &str,
        metadata_json: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM skills WHERE id=?1)",
                params![entry.skill_id],
                |row| row.get(0),
            )?;
            anyhow::ensure!(exists, "Skill not found: {}", entry.skill_id);
            tx.execute(
                "INSERT INTO device_sync_tombstones
                 (id, skill_id, skill_name, trash_path, deleted_at, expires_at, deletion_source, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![entry.id, entry.skill_id, entry.skill_name, entry.trash_path,
                    entry.deleted_at, entry.expires_at, deletion_source, metadata_json],
            )?;
            tx.execute("DELETE FROM skills WHERE id=?1", params![entry.skill_id])?;
            for key in ["shared_source", "source_baseline", "source_origin"] {
                tx.execute(
                    "DELETE FROM settings WHERE key=?1",
                    params![format!("device_sync.{key}.{}", entry.skill_id)],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub(crate) fn list_recycle_bin_rows(
        &self,
    ) -> Result<Vec<(TrashEntry, String, Option<String>)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, skill_name, trash_path, deleted_at, expires_at,
                        deletion_source, metadata_json
                 FROM device_sync_tombstones ORDER BY deleted_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    TrashEntry {
                        id: row.get(0)?,
                        skill_id: row.get(1)?,
                        skill_name: row.get(2)?,
                        trash_path: row.get(3)?,
                        deleted_at: row.get(4)?,
                        expires_at: row.get(5)?,
                    },
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub(crate) fn get_recycle_bin_row(
        &self,
        id: &str,
    ) -> Result<Option<(TrashEntry, String, Option<String>)>> {
        Ok(self
            .list_recycle_bin_rows()?
            .into_iter()
            .find(|(entry, _, _)| entry.id == id))
    }

    pub(crate) fn restore_recycle_bin_snapshot(
        &self,
        id: &str,
        skill: &SkillRecord,
        targets: &[SkillTargetRecord],
        tags: &[String],
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let pending: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM device_sync_tombstones WHERE id=?1)",
                params![id], |row| row.get(0),
            )?;
            anyhow::ensure!(pending, "recycle bin item not found");
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM skills WHERE id=?1)",
                params![skill.id], |row| row.get(0),
            )?;
            anyhow::ensure!(!exists, "Skill already exists; cannot restore it");
            upsert_skill_with_conn(&tx, skill)?;
            for target in targets { upsert_skill_target_with_conn(&tx, target)?; }
            for name in tags {
                let normalized = normalize_tag_name(name)?;
                tx.execute("INSERT INTO skill_tags (name,created_at,updated_at) VALUES (?1,?2,?2) ON CONFLICT(name) DO NOTHING", params![normalized, now_ms()])?;
                tx.execute("INSERT OR IGNORE INTO skill_tag_links (skill_id,tag_id,created_at) SELECT ?1,id,?3 FROM skill_tags WHERE name=?2 COLLATE NOCASE", params![skill.id, normalized, now_ms()])?;
            }
            tx.execute("DELETE FROM device_sync_tombstones WHERE id=?1", params![id])?;
            tx.execute("DELETE FROM settings WHERE key=?1", params![format!("device_sync.trash_metadata.{id}")])?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(crate) fn remove_recycle_bin_row(&self, id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM device_sync_tombstones WHERE id=?1",
                params![id],
            )?;
            tx.execute(
                "DELETE FROM settings WHERE key=?1",
                params![format!("device_sync.trash_metadata.{id}")],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(crate) fn device_sync_trash_metadata(&self, id: &str) -> Result<Option<TrashMetadata>> {
        self.get_setting(&format!("device_sync.trash_metadata.{id}"))?
            .map(|value| serde_json::from_str(&value).context("decode recycle bin metadata"))
            .transpose()
    }

    pub(crate) fn restore_device_sync_trash_record(
        &self,
        id: &str,
        skill: &SkillRecord,
        tags: &[String],
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM skills WHERE id=?1)", params![skill.id], |row| row.get(0))?;
            anyhow::ensure!(!exists, "Skill already exists; cannot overwrite it from the recycle bin");
            let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM device_sync_tombstones WHERE id=?1 AND skill_id=?2)", params![id, skill.id], |row| row.get(0))?;
            anyhow::ensure!(pending, "device sync trash entry not found");
            upsert_skill_with_conn(&tx, skill)?;
            for name in tags {
                let normalized = normalize_tag_name(name)?;
                tx.execute("INSERT INTO skill_tags (name,created_at,updated_at) VALUES (?1,?2,?2) ON CONFLICT(name) DO NOTHING", params![normalized, now_ms()])?;
                tx.execute("INSERT OR IGNORE INTO skill_tag_links (skill_id,tag_id,created_at) SELECT ?1,id,?3 FROM skill_tags WHERE name=?2 COLLATE NOCASE", params![skill.id,normalized,now_ms()])?;
            }
            tx.execute("DELETE FROM device_sync_tombstones WHERE id=?1", params![id])?;
            tx.execute("DELETE FROM settings WHERE key=?1", params![format!("device_sync.trash_metadata.{id}")])?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn list_device_sync_trash(&self) -> Result<Vec<TrashEntry>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_id, skill_name, trash_path, deleted_at, expires_at
                 FROM device_sync_tombstones ORDER BY deleted_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(TrashEntry {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    skill_name: row.get(2)?,
                    trash_path: row.get(3)?,
                    deleted_at: row.get(4)?,
                    expires_at: row.get(5)?,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
    }

    #[allow(dead_code)]
    pub fn record_source_failure(&self, skill_id: &str, raw: &str) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("UPDATE skills SET status='error' WHERE id=?1", params![skill_id])?;
            tx.execute("INSERT INTO skill_source_checks (skill_id,error_code,checked_at) VALUES (?1,?2,?3)
                ON CONFLICT(skill_id) DO UPDATE SET error_code=excluded.error_code,checked_at=excluded.checked_at",
                params![skill_id, super::skill_issues::safe_code(raw), now_ms()])?;
            tx.commit()?;
            Ok(())
        })
    }

    #[allow(dead_code)]
    pub fn source_checks(
        &self,
    ) -> Result<std::collections::HashMap<String, (Option<String>, i64)>> {
        self.with_conn(|conn| {
            let mut statement =
                conn.prepare("SELECT skill_id,error_code,checked_at FROM skill_source_checks")?;
            let rows =
                statement.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    #[allow(dead_code)]
    pub fn commit_skill_update(
        &self,
        skill: &SkillRecord,
        targets: &[SkillTargetRecord],
    ) -> Result<()> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            upsert_skill_with_conn(&transaction, skill)?;
            if let Some(baseline) = skill
                .content_hash
                .clone()
                .and_then(|hash| SourceBaseline::from_skill(skill, hash))
            {
                transaction.execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![format!("device_sync.source_baseline.{}", skill.id), serde_json::to_string(&baseline)?])?;
            }
            transaction.execute("INSERT INTO skill_source_checks (skill_id,error_code,checked_at) VALUES (?1,NULL,?2)
                ON CONFLICT(skill_id) DO UPDATE SET error_code=NULL,checked_at=excluded.checked_at", params![skill.id, now_ms()])?;
            for target in targets {
                upsert_skill_target_with_conn(&transaction, target)?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    #[allow(dead_code)]
    pub(crate) fn commit_skill_updates(&self, skills: &[SkillRecord]) -> Result<()> {
        self.with_conn(|conn| {
            let transaction = conn.unchecked_transaction()?;
            for skill in skills {
                upsert_skill_with_conn(&transaction, skill)?;
                if let Some(baseline) = skill
                    .content_hash
                    .clone()
                    .and_then(|hash| SourceBaseline::from_skill(skill, hash))
                {
                    transaction.execute(
                        "INSERT INTO settings (key,value) VALUES (?1,?2)
                         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                        params![
                            format!("device_sync.source_baseline.{}", skill.id),
                            serde_json::to_string(&baseline)?
                        ],
                    )?;
                }
                transaction.execute(
                    "INSERT INTO skill_source_checks (skill_id,error_code,checked_at)
                     VALUES (?1,NULL,?2)
                     ON CONFLICT(skill_id) DO UPDATE SET error_code=NULL,checked_at=excluded.checked_at",
                    params![skill.id, now_ms()],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub(crate) fn was_skill_imported_by_device_sync(
        &self,
        id: &str,
        created_at: i64,
    ) -> Result<bool> {
        if let Some(origin) = self.get_setting(&format!("device_sync.source_origin.{id}"))? {
            return Ok(origin == "imported");
        }
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM skills WHERE id=?1 AND source_type='local' AND source_ref=central_path)
                 OR EXISTS(
                   SELECT 1 FROM device_sync_runs r
                   JOIN settings d ON d.key='device_sync.run_items.' || r.id
                   JOIN json_each(CASE WHEN json_valid(d.value) THEN d.value ELSE '[]' END) item
                   WHERE r.status IN ('success','resolved') AND r.started_at<=?2 AND r.finished_at>=?2
                     AND json_extract(item.value,'$.skill_id')=?1
                     AND json_extract(item.value,'$.kind')='added'
                     AND json_extract(item.value,'$.direction')='download')",
                params![id, created_at], |row| row.get(0),
            ).map_err(Into::into)
        })
    }

    pub(crate) fn commit_device_sync_library(
        &self,
        skills: &[(SkillRecord, Vec<String>)],
        targets: &[(SkillTargetRecord, String)],
    ) -> Result<()> {
        self.commit_device_sync_library_with_sources(
            skills,
            targets,
            &std::collections::BTreeMap::new(),
        )
    }

    pub(crate) fn source_baseline(&self, id: &str) -> Result<Option<SourceBaseline>> {
        self.get_setting(&format!("device_sync.source_baseline.{id}"))?
            .map(|value| serde_json::from_str(&value).context("read Skill source baseline"))
            .transpose()
    }

    pub(crate) fn record_source_check_success(
        &self,
        skill: &SkillRecord,
        source_content_hash: String,
    ) -> Result<()> {
        let baseline = SourceBaseline::from_skill(skill, source_content_hash)
            .context("bound Skill source is missing")?;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE skills SET source_subpath=?2,source_revision=?3,status='ok',last_seen_at=?4 WHERE id=?1",
                params![skill.id, skill.source_subpath, skill.source_revision, now_ms()],
            )?;
            tx.execute(
                "INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![format!("device_sync.source_baseline.{}", skill.id), serde_json::to_string(&baseline)?],
            )?;
            tx.execute("INSERT INTO skill_source_checks (skill_id,error_code,checked_at) VALUES (?1,NULL,?2)
                ON CONFLICT(skill_id) DO UPDATE SET error_code=NULL,checked_at=excluded.checked_at", params![skill.id, now_ms()])?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(crate) fn commit_device_sync_library_with_sources(
        &self,
        skills: &[(SkillRecord, Vec<String>)],
        targets: &[(SkillTargetRecord, String)],
        sources: &std::collections::BTreeMap<String, super::device_sync::manifest::SharedSource>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_ms();
            for (skill, tags) in skills {
                let mut local = skill.clone();
                if let Some((status, source_type)) = tx.query_row("SELECT status,source_type FROM skills WHERE id=?1", params![skill.id], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?))).optional()? {
                    local.status = status;
                    if source_type == "local" && local.source_type == "local"
                        && local.has_unbound_local_source() {
                        let cleared = tx.execute("UPDATE skill_source_checks SET error_code=NULL,checked_at=?2 WHERE skill_id=?1 AND error_code IS NOT NULL", params![skill.id, now])?;
                        if cleared > 0 { local.status = "ok".into(); }
                    }
                }
                upsert_skill_with_conn(&tx, &local)?;
                if local.source_type == "local" {
                    let origin = if local.has_unbound_local_source() { "imported" } else { "local" };
                    tx.execute("INSERT OR IGNORE INTO settings (key,value) VALUES (?1,?2)", params![format!("device_sync.source_origin.{}", local.id), origin])?;
                }
                tx.execute("DELETE FROM skill_tag_links WHERE skill_id = ?1", params![skill.id])?;
                for name in tags {
                    let normalized = normalize_tag_name(name)?;
                    tx.execute("INSERT INTO skill_tags (name, created_at, updated_at) VALUES (?1, ?2, ?2) ON CONFLICT(name) DO NOTHING", params![normalized, now])?;
                    let tag_id: i64 = tx.query_row("SELECT id FROM skill_tags WHERE name = ?1 COLLATE NOCASE", params![normalized], |row| row.get(0))?;
                    tx.execute("INSERT OR IGNORE INTO skill_tag_links (skill_id, tag_id, created_at) VALUES (?1, ?2, ?3)", params![skill.id, tag_id, now])?;
                }
            }
            for (id, source) in sources {
                tx.execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![format!("device_sync.shared_source.{id}"), serde_json::to_string(source)?])?;
            }
            for (target, hash) in targets {
                upsert_skill_target_with_conn(&tx, target)?;
                tx.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![format!("device_sync.target_baseline.{}", target.id), serde_json::to_string(&(&target.target_path, hash))?])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub fn adopt_skill_id(&self, old_id: &str, new_id: &str) -> Result<()> {
        if old_id == new_id || self.get_skill_by_id(new_id)?.is_some() {
            return Ok(());
        }
        self.with_conn(|conn| {
            let original_path: String = conn.query_row(
                "SELECT central_path FROM skills WHERE id = ?1",
                params![old_id],
                |row| row.get(0),
            )?;
            let temporary_path = format!("{}.identity-{}", original_path, Uuid::new_v4());
            conn.execute_batch("BEGIN;")?;
            let result = (|| -> Result<()> {
                conn.execute(
                    "UPDATE skills SET central_path = ?1 WHERE id = ?2",
                    params![temporary_path, old_id],
                )?;
                conn.execute(
                    "INSERT INTO skills (
                       id, name, description, source_type, source_ref, source_subpath,
                       source_revision, central_path, content_hash, created_at, updated_at,
                       last_sync_at, last_seen_at, enabled, status
                     ) SELECT ?1, name, description, source_type, source_ref, source_subpath,
                       source_revision, ?2, content_hash, created_at, updated_at,
                       last_sync_at, last_seen_at, enabled, status
                     FROM skills WHERE id = ?3",
                    params![new_id, original_path, old_id],
                )?;
                conn.execute(
                    "UPDATE skill_targets SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE skill_tag_links SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE skill_source_checks SET skill_id=?1 WHERE skill_id=?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    params![format!("device_sync.source_origin.{new_id}")],
                )?;
                conn.execute(
                    "UPDATE settings SET key = ?1 WHERE key = ?2",
                    params![
                        format!("device_sync.source_origin.{new_id}"),
                        format!("device_sync.source_origin.{old_id}")
                    ],
                )?;
                conn.execute(
                    "DELETE FROM settings WHERE key=?1",
                    params![format!("device_sync.source_baseline.{new_id}")],
                )?;
                conn.execute(
                    "UPDATE settings SET key=?1 WHERE key=?2",
                    params![
                        format!("device_sync.source_baseline.{new_id}"),
                        format!("device_sync.source_baseline.{old_id}")
                    ],
                )?;
                conn.execute(
                    "DELETE FROM settings WHERE key=?1",
                    params![format!("device_sync.shared_source.{new_id}")],
                )?;
                conn.execute(
                    "UPDATE settings SET key=?1 WHERE key=?2",
                    params![
                        format!("device_sync.shared_source.{new_id}"),
                        format!("device_sync.shared_source.{old_id}")
                    ],
                )?;
                conn.execute(
                    "UPDATE skill_assignments SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE project_skill_links SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE project_discovered_skills SET matched_skill_id = ?1 WHERE matched_skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE skill_template_items SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE skill_template_revision_items SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE project_skill_origins SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE project_template_overrides SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute(
                    "UPDATE project_template_failures SET skill_id = ?1 WHERE skill_id = ?2",
                    params![new_id, old_id],
                )?;
                conn.execute("DELETE FROM skills WHERE id = ?1", params![old_id])?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn set_skill_tag_names(&self, skill_id: &str, tag_names: &[String]) -> Result<()> {
        self.with_conn(|conn| {
            let now = now_ms();
            conn.execute_batch("BEGIN;")?;
            let result = (|| -> Result<()> {
                conn.execute(
                    "DELETE FROM skill_tag_links WHERE skill_id = ?1",
                    params![skill_id],
                )?;
                for name in tag_names {
                    let normalized = normalize_tag_name(name)?;
                    conn.execute(
                        "INSERT INTO skill_tags (name, created_at, updated_at)
                         VALUES (?1, ?2, ?2)
                         ON CONFLICT(name) DO NOTHING",
                        params![normalized, now],
                    )?;
                    let tag_id: i64 = conn.query_row(
                        "SELECT id FROM skill_tags WHERE name = ?1 COLLATE NOCASE",
                        params![normalized],
                        |row| row.get(0),
                    )?;
                    conn.execute(
                        "INSERT OR IGNORE INTO skill_tag_links (skill_id, tag_id, created_at)
                         VALUES (?1, ?2, ?3)",
                        params![skill_id, tag_id, now],
                    )?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    #[allow(dead_code)]
    pub fn is_skill_target_path_used_by_another_record(
        &self,
        target_path: &str,
        record_id: &str,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*)
                 FROM skill_targets
                 WHERE target_path = ?1
                   AND id != ?2
                   AND status != 'disabled'",
                params![target_path, record_id],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    pub fn is_target_used_by_other_skill(&self, path: &str, skill_id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM skill_targets WHERE target_path = ?1 AND skill_id != ?2 AND status != 'disabled')",
                params![path, skill_id],
                |row| row.get(0),
            ).map_err(Into::into)
        })
    }

    pub fn list_tool_sync_issues(
        &self,
    ) -> Result<Vec<crate::core::device_sync::types::ToolSyncIssue>> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare("SELECT DISTINCT s.name, t.tool FROM skill_targets t JOIN skills s ON s.id=t.skill_id WHERE s.enabled=1 AND t.status='error' ORDER BY s.name,t.tool")?;
            let rows = statement.query_map([], |row| Ok(crate::core::device_sync::types::ToolSyncIssue {
                skill_name: row.get(0)?, tool: row.get(1)?,
            }))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn prune_prompt_revisions(&self, logical_path: &str, keep: usize) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM prompt_revisions
                 WHERE logical_path = ?1
                   AND id NOT IN (
                     SELECT id FROM prompt_revisions
                     WHERE logical_path = ?1
                     ORDER BY created_at DESC, rowid DESC
                     LIMIT ?2
                   )",
                params![logical_path, keep as i64],
            )?;
            Ok(())
        })
    }

    pub fn get_prompt_revision(&self, revision_id: &str) -> Result<Option<PromptRevisionRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, logical_path, real_path, tool, profile_id, project_id,
                        operation, content, content_hash, created_at
                 FROM prompt_revisions WHERE id = ?1",
                params![revision_id],
                prompt_revision_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn list_prompt_revisions(
        &self,
        logical_path: &str,
        limit: usize,
    ) -> Result<Vec<PromptRevisionRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, logical_path, real_path, tool, profile_id, project_id,
                        operation, content, content_hash, created_at
                 FROM prompt_revisions
                 WHERE logical_path = ?1
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(
                params![logical_path, limit as i64],
                prompt_revision_from_row,
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn insert_prompt_revision(&self, revision: &PromptRevisionRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO prompt_revisions (
                   id, logical_path, real_path, tool, profile_id, project_id,
                   operation, content, content_hash, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    revision.id,
                    revision.logical_path,
                    revision.real_path,
                    revision.tool,
                    revision.profile_id,
                    revision.project_id,
                    revision.operation,
                    revision.content,
                    revision.content_hash,
                    revision.created_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_project_names_for_skill_origin(&self, skill_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT project.name
                 FROM project_skill_origins origin
                 JOIN projects project ON project.id = origin.project_id
                 WHERE origin.skill_id = ?1
                 ORDER BY project.name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map(params![skill_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_skill_template_names_for_skill(&self, skill_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT template.name
                 FROM skill_template_items item
                 JOIN skill_templates template ON template.id = item.template_id
                 WHERE item.skill_id = ?1
                 ORDER BY template.name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map(params![skill_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_template_project_failures(
        &self,
        template_id: &str,
    ) -> Result<Vec<ProjectTemplateFailureRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, template_id, skill_id, tool, kind, error, updated_at
                 FROM project_template_failures WHERE template_id = ?1
                 ORDER BY updated_at DESC, project_id ASC, skill_id ASC, tool ASC",
            )?;
            let rows = stmt.query_map(params![template_id], project_template_failure_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_project_template_failures(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectTemplateFailureRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, template_id, skill_id, tool, kind, error, updated_at
                 FROM project_template_failures WHERE project_id = ?1
                 ORDER BY updated_at DESC, skill_id ASC, tool ASC",
            )?;
            let rows = stmt.query_map(params![project_id], project_template_failure_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn replace_project_template_failures(
        &self,
        project_id: &str,
        template_id: &str,
        failures: &[ProjectTemplateFailureRecord],
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result = (|| -> Result<()> {
                conn.execute(
                    "DELETE FROM project_template_failures
                     WHERE project_id = ?1 AND template_id = ?2",
                    params![project_id, template_id],
                )?;
                for failure in failures {
                    conn.execute(
                        "INSERT INTO project_template_failures (
                           project_id, template_id, skill_id, tool, kind, error, updated_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            failure.project_id,
                            failure.template_id,
                            failure.skill_id,
                            failure.tool,
                            failure.kind,
                            failure.error,
                            failure.updated_at
                        ],
                    )?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    pub fn delete_project_template_override(&self, project_id: &str, skill_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM project_template_overrides WHERE project_id = ?1 AND skill_id = ?2",
                params![project_id, skill_id],
            )?;
            Ok(())
        })
    }

    pub fn get_project_template_override(
        &self,
        project_id: &str,
        skill_id: &str,
    ) -> Result<Option<ProjectTemplateOverrideRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT project_id, skill_id, kind, created_at, updated_at
                 FROM project_template_overrides WHERE project_id = ?1 AND skill_id = ?2",
                params![project_id, skill_id],
                |row| {
                    Ok(ProjectTemplateOverrideRecord {
                        project_id: row.get(0)?,
                        skill_id: row.get(1)?,
                        kind: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn upsert_project_template_override(
        &self,
        record: &ProjectTemplateOverrideRecord,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO project_template_overrides (
                   project_id, skill_id, kind, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project_id, skill_id) DO UPDATE SET
                   kind = excluded.kind, updated_at = excluded.updated_at",
                params![
                    record.project_id,
                    record.skill_id,
                    record.kind,
                    record.created_at,
                    record.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_project_skill_origins_by_source(
        &self,
        project_id: &str,
        source_type: &str,
        source_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM project_skill_origins
                 WHERE project_id = ?1 AND source_type = ?2 AND source_id = ?3",
                params![project_id, source_type, source_id],
            )?;
            Ok(())
        })
    }

    pub fn delete_project_skill_origin(
        &self,
        project_id: &str,
        skill_id: &str,
        source_type: &str,
        source_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM project_skill_origins
                 WHERE project_id = ?1 AND skill_id = ?2
                   AND source_type = ?3 AND source_id = ?4",
                params![project_id, skill_id, source_type, source_id],
            )?;
            Ok(())
        })
    }

    fn list_project_skill_origins_filtered(
        &self,
        project_id: &str,
        skill_id: Option<&str>,
    ) -> Result<Vec<ProjectSkillOriginRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, skill_id, source_type, source_id, desired_tools,
                        created_at, updated_at
                 FROM project_skill_origins
                 WHERE project_id = ?1 AND (?2 IS NULL OR skill_id = ?2)
                 ORDER BY skill_id ASC, source_type ASC, source_id ASC",
            )?;
            let rows =
                stmt.query_map(params![project_id, skill_id], project_skill_origin_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_project_skill_origins_for_skill(
        &self,
        project_id: &str,
        skill_id: &str,
    ) -> Result<Vec<ProjectSkillOriginRecord>> {
        self.list_project_skill_origins_filtered(project_id, Some(skill_id))
    }

    pub fn list_project_skill_origins(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectSkillOriginRecord>> {
        self.list_project_skill_origins_filtered(project_id, None)
    }

    pub fn upsert_project_skill_origin(&self, origin: &ProjectSkillOriginRecord) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO project_skill_origins (
                   project_id, skill_id, source_type, source_id, desired_tools,
                   created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(project_id, skill_id, source_type, source_id) DO UPDATE SET
                   desired_tools = excluded.desired_tools,
                   updated_at = excluded.updated_at",
                params![
                    origin.project_id,
                    origin.skill_id,
                    origin.source_type,
                    origin.source_id,
                    string_vec_to_json(&origin.desired_tools)?,
                    origin.created_at,
                    origin.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_project_template_binding(&self, project_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM project_template_bindings WHERE project_id = ?1",
                params![project_id],
            )?;
            Ok(())
        })
    }

    pub fn list_all_project_template_bindings(&self) -> Result<Vec<ProjectTemplateBindingRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, template_id, applied_revision_id, sync_error,
                        last_attempt_at, synced_at, created_at, updated_at
                 FROM project_template_bindings ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], project_template_binding_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_project_template_bindings(
        &self,
        template_id: &str,
    ) -> Result<Vec<ProjectTemplateBindingRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, template_id, applied_revision_id, sync_error,
                        last_attempt_at, synced_at, created_at, updated_at
                 FROM project_template_bindings WHERE template_id = ?1
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map(params![template_id], project_template_binding_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn get_project_template_binding(
        &self,
        project_id: &str,
    ) -> Result<Option<ProjectTemplateBindingRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT project_id, template_id, applied_revision_id, sync_error,
                        last_attempt_at, synced_at, created_at, updated_at
                 FROM project_template_bindings WHERE project_id = ?1",
                params![project_id],
                project_template_binding_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn upsert_project_template_binding(
        &self,
        binding: &ProjectTemplateBindingRecord,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO project_template_bindings (
                   project_id, template_id, applied_revision_id, sync_error,
                   last_attempt_at, synced_at, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(project_id) DO UPDATE SET
                   template_id = excluded.template_id,
                   applied_revision_id = excluded.applied_revision_id,
                   sync_error = excluded.sync_error,
                   last_attempt_at = excluded.last_attempt_at,
                   synced_at = excluded.synced_at,
                   updated_at = excluded.updated_at",
                params![
                    binding.project_id,
                    binding.template_id,
                    binding.applied_revision_id,
                    binding.sync_error,
                    binding.last_attempt_at,
                    binding.synced_at,
                    binding.created_at,
                    binding.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_skill_template(&self, template_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let binding_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM project_template_bindings WHERE template_id = ?1",
                params![template_id],
                |row| row.get(0),
            )?;
            if binding_count > 0 {
                anyhow::bail!("skill template is still linked to {binding_count} project(s)");
            }
            conn.execute(
                "DELETE FROM project_skill_origins
                 WHERE source_type = 'template' AND source_id = ?1",
                params![template_id],
            )?;
            let changed = conn.execute(
                "DELETE FROM skill_templates WHERE id = ?1",
                params![template_id],
            )?;
            if changed == 0 {
                anyhow::bail!("skill template not found: {template_id}");
            }
            Ok(())
        })
    }

    pub fn set_skill_template_archived(&self, template_id: &str, archived: bool) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE skill_templates SET archived = ?2, updated_at = ?3 WHERE id = ?1",
                params![template_id, archived as i32, now_ms()],
            )?;
            if changed == 0 {
                anyhow::bail!("skill template not found: {template_id}");
            }
            Ok(())
        })
    }

    pub fn list_skill_template_revision_items(
        &self,
        revision_id: &str,
    ) -> Result<Vec<SkillTemplateRevisionItemRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT revision_id, skill_id, skill_name
                 FROM skill_template_revision_items WHERE revision_id = ?1
                 ORDER BY skill_name COLLATE NOCASE ASC, skill_id ASC",
            )?;
            let rows = stmt.query_map(params![revision_id], |row| {
                Ok(SkillTemplateRevisionItemRecord {
                    revision_id: row.get(0)?,
                    skill_id: row.get(1)?,
                    skill_name: row.get(2)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn get_skill_template_revision(
        &self,
        revision_id: &str,
    ) -> Result<Option<SkillTemplateRevisionRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, template_id, revision_number, name, description, created_at
                 FROM skill_template_revisions WHERE id = ?1",
                params![revision_id],
                skill_template_revision_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn list_skill_template_revisions(
        &self,
        template_id: &str,
    ) -> Result<Vec<SkillTemplateRevisionRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, template_id, revision_number, name, description, created_at
                 FROM skill_template_revisions WHERE template_id = ?1
                 ORDER BY revision_number DESC",
            )?;
            let rows = stmt.query_map(params![template_id], skill_template_revision_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn next_skill_template_revision_number(&self, template_id: &str) -> Result<i64> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COALESCE(MAX(revision_number), 0) + 1
                 FROM skill_template_revisions WHERE template_id = ?1",
                params![template_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
    }

    pub fn list_skill_template_item_ids(&self, template_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT skill_id FROM skill_template_items
                 WHERE template_id = ?1 ORDER BY skill_id ASC",
            )?;
            let rows = stmt.query_map(params![template_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn list_skill_templates(&self, include_archived: bool) -> Result<Vec<SkillTemplateRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, mode, archived, current_revision_id,
                        created_at, updated_at
                 FROM skill_templates
                 WHERE ?1 = 1 OR archived = 0
                 ORDER BY archived ASC, updated_at DESC, name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map(params![include_archived as i32], skill_template_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })
    }

    pub fn get_skill_template_by_name(&self, name: &str) -> Result<Option<SkillTemplateRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, name, description, mode, archived, current_revision_id,
                        created_at, updated_at
                 FROM skill_templates WHERE name = ?1 COLLATE NOCASE",
                params![name],
                skill_template_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn get_skill_template(&self, template_id: &str) -> Result<Option<SkillTemplateRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, name, description, mode, archived, current_revision_id,
                        created_at, updated_at
                 FROM skill_templates WHERE id = ?1",
                params![template_id],
                skill_template_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn save_skill_template_revision(
        &self,
        template: &SkillTemplateRecord,
        revision: &SkillTemplateRevisionRecord,
        items: &[SkillTemplateRevisionItemRecord],
    ) -> Result<()> {
        if items.is_empty() {
            anyhow::bail!("skill template must contain at least one skill");
        }
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            let result = (|| -> Result<()> {
                let existing_mode = conn
                    .query_row(
                        "SELECT mode FROM skill_templates WHERE id = ?1",
                        params![template.id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if existing_mode
                    .as_deref()
                    .is_some_and(|mode| mode != template.mode)
                {
                    anyhow::bail!("skill template mode cannot be changed");
                }

                conn.execute(
                    "INSERT INTO skill_templates (
                       id, name, description, mode, archived, current_revision_id,
                       created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET
                       name = excluded.name,
                       description = excluded.description,
                       archived = excluded.archived,
                       current_revision_id = excluded.current_revision_id,
                       updated_at = excluded.updated_at",
                    params![
                        template.id,
                        template.name,
                        template.description,
                        template.mode,
                        template.archived as i32,
                        template.current_revision_id,
                        template.created_at,
                        template.updated_at
                    ],
                )?;
                conn.execute(
                    "INSERT INTO skill_template_revisions (
                       id, template_id, revision_number, name, description, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        revision.id,
                        revision.template_id,
                        revision.revision_number,
                        revision.name,
                        revision.description,
                        revision.created_at
                    ],
                )?;
                for item in items {
                    conn.execute(
                        "INSERT INTO skill_template_revision_items (
                           revision_id, skill_id, skill_name
                         ) VALUES (?1, ?2, ?3)",
                        params![item.revision_id, item.skill_id, item.skill_name],
                    )?;
                }
                conn.execute(
                    "DELETE FROM skill_template_items WHERE template_id = ?1",
                    params![template.id],
                )?;
                for item in items {
                    conn.execute(
                        "INSERT INTO skill_template_items (template_id, skill_id, created_at)
                         VALUES (?1, ?2, ?3)",
                        params![template.id, item.skill_id, revision.created_at],
                    )?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK;");
                    Err(err)
                }
            }
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("failed to open db at {:?}", self.db_path))?;
        // Enforce foreign key constraints on every connection (rusqlite PRAGMA is per-connection).
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        f(&conn)
    }
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        enabled_tool_keys: string_vec_from_json(row.get::<_, String>(3)?),
        local_git_exclude_enabled: row.get::<_, i32>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_scanned_at: row.get(7)?,
    })
}

fn project_link_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSkillLinkRecord> {
    Ok(ProjectSkillLinkRecord {
        project_id: row.get(0)?,
        skill_id: row.get(1)?,
        enabled: row.get::<_, i32>(2)? != 0,
        desired_tools: string_vec_from_json(row.get::<_, String>(3)?),
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn prompt_revision_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptRevisionRecord> {
    Ok(PromptRevisionRecord {
        id: row.get(0)?,
        logical_path: row.get(1)?,
        real_path: row.get(2)?,
        tool: row.get(3)?,
        profile_id: row.get(4)?,
        project_id: row.get(5)?,
        operation: row.get(6)?,
        content: row.get(7)?,
        content_hash: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn project_template_failure_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectTemplateFailureRecord> {
    Ok(ProjectTemplateFailureRecord {
        project_id: row.get(0)?,
        template_id: row.get(1)?,
        skill_id: row.get(2)?,
        tool: row.get(3)?,
        kind: row.get(4)?,
        error: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn project_skill_origin_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectSkillOriginRecord> {
    Ok(ProjectSkillOriginRecord {
        project_id: row.get(0)?,
        skill_id: row.get(1)?,
        source_type: row.get(2)?,
        source_id: row.get(3)?,
        desired_tools: string_vec_from_json(row.get::<_, String>(4)?),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn project_template_binding_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectTemplateBindingRecord> {
    Ok(ProjectTemplateBindingRecord {
        project_id: row.get(0)?,
        template_id: row.get(1)?,
        applied_revision_id: row.get(2)?,
        sync_error: row.get(3)?,
        last_attempt_at: row.get(4)?,
        synced_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn skill_template_revision_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SkillTemplateRevisionRecord> {
    Ok(SkillTemplateRevisionRecord {
        id: row.get(0)?,
        template_id: row.get(1)?,
        revision_number: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn skill_template_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillTemplateRecord> {
    Ok(SkillTemplateRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        mode: row.get(3)?,
        archived: row.get::<_, i32>(4)? != 0,
        current_revision_id: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn project_discovered_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProjectDiscoveredSkillRecord> {
    Ok(ProjectDiscoveredSkillRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        relative_path: row.get(3)?,
        absolute_path: row.get(4)?,
        content_hash: row.get(5)?,
        tool_keys: string_vec_from_json(row.get::<_, String>(6)?),
        status: row.get(7)?,
        matched_skill_id: row.get(8)?,
        first_seen_at: row.get(9)?,
        last_seen_at: row.get(10)?,
    })
}

fn skill_assignment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillAssignmentRecord> {
    Ok(SkillAssignmentRecord {
        id: row.get(0)?,
        skill_id: row.get(1)?,
        tool: row.get(2)?,
        scope: row.get(3)?,
        project_id: row.get(4)?,
        ownership: row.get(5)?,
        state: row.get(6)?,
        last_error: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn string_vec_from_json(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

fn string_vec_to_json(values: &[String]) -> Result<String> {
    serde_json::to_string(values).map_err(Into::into)
}

fn migrate_skill_targets_to_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN;
         DROP INDEX IF EXISTS idx_skill_targets_unique_scope;
         CREATE TABLE skill_targets_new (
           id TEXT PRIMARY KEY,
           skill_id TEXT NOT NULL,
           tool TEXT NOT NULL,
           scope TEXT NOT NULL DEFAULT 'global',
           project_path TEXT NULL,
           target_path TEXT NOT NULL,
           mode TEXT NOT NULL,
           status TEXT NOT NULL,
           last_error TEXT NULL,
           synced_at INTEGER NULL,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
         );
         INSERT INTO skill_targets_new (
           id, skill_id, tool, scope, project_path, target_path, mode, status, last_error, synced_at
         )
         SELECT id, skill_id, tool, 'global', NULL, target_path, mode, status, last_error, synced_at
         FROM skill_targets;
         DROP TABLE skill_targets;
         ALTER TABLE skill_targets_new RENAME TO skill_targets;
         CREATE UNIQUE INDEX idx_skill_targets_unique_scope
         ON skill_targets(skill_id, tool, scope, COALESCE(project_path, ''));
         COMMIT;",
    )?;
    Ok(())
}

fn migrate_tags_to_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_tags (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           name TEXT NOT NULL UNIQUE COLLATE NOCASE,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS skill_tag_links (
           skill_id TEXT NOT NULL,
           tag_id INTEGER NOT NULL,
           created_at INTEGER NOT NULL,
           PRIMARY KEY (skill_id, tag_id),
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE,
           FOREIGN KEY(tag_id) REFERENCES skill_tags(id) ON DELETE CASCADE
         );",
    )?;
    Ok(())
}

fn migrate_skill_enabled_to_v6(conn: &Connection) -> Result<()> {
    conn.execute_batch("ALTER TABLE skills ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;")?;
    Ok(())
}

fn migrate_projects_to_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL,
           root_path TEXT NOT NULL UNIQUE,
           enabled_tool_keys TEXT NOT NULL,
           local_git_exclude_enabled INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           last_scanned_at INTEGER NULL
         );

         CREATE TABLE IF NOT EXISTS project_skill_links (
           project_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           enabled INTEGER NOT NULL DEFAULT 1,
           desired_tools TEXT NOT NULL,
           status TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY (project_id, skill_id),
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS project_discovered_skills (
           id TEXT PRIMARY KEY,
           project_id TEXT NOT NULL,
           name TEXT NOT NULL,
           relative_path TEXT NOT NULL,
           absolute_path TEXT NOT NULL,
           content_hash TEXT NULL,
           tool_keys TEXT NOT NULL,
           status TEXT NOT NULL,
           matched_skill_id TEXT NULL,
           first_seen_at INTEGER NOT NULL,
           last_seen_at INTEGER NOT NULL,
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(matched_skill_id) REFERENCES skills(id) ON DELETE SET NULL,
           UNIQUE(project_id, absolute_path)
         );

         CREATE INDEX IF NOT EXISTS idx_project_discovered_project_name
         ON project_discovered_skills(project_id, name);",
    )?;
    Ok(())
}

fn migrate_assignments_to_v8(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS skill_assignments (
           id TEXT PRIMARY KEY,
           skill_id TEXT NOT NULL,
           tool TEXT NOT NULL,
           scope TEXT NOT NULL,
           project_id TEXT NULL,
           ownership TEXT NOT NULL,
           state TEXT NOT NULL,
           last_error TEXT NULL,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE,
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_assignments_unique_scope
         ON skill_assignments(skill_id, tool, scope, COALESCE(project_id, ''));

         CREATE TABLE IF NOT EXISTS ignored_issues (
           id TEXT PRIMARY KEY,
           scope TEXT NOT NULL,
           scope_id TEXT NULL,
           issue_key TEXT NOT NULL,
           fingerprint TEXT NOT NULL,
           ignored_at INTEGER NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_ignored_issues_instance
         ON ignored_issues(scope, COALESCE(scope_id, ''), issue_key, fingerprint);

         INSERT OR IGNORE INTO skill_assignments (
           id, skill_id, tool, scope, project_id, ownership, state,
           last_error, created_at, updated_at
         )
         SELECT
           'assignment:target:' || target.id,
           target.skill_id,
           target.tool,
           target.scope,
           CASE WHEN target.scope = 'project' THEN project.id ELSE NULL END,
           'hub',
           CASE
             WHEN target.status = 'disabled' THEN 'suspended'
             WHEN target.status = 'error' THEN 'error'
             ELSE 'active'
           END,
           target.last_error,
           COALESCE(target.synced_at, skill.created_at),
           COALESCE(target.synced_at, skill.updated_at)
         FROM skill_targets target
         JOIN skills skill ON skill.id = target.skill_id
         LEFT JOIN projects project ON project.root_path = target.project_path
         WHERE target.scope = 'global' OR project.id IS NOT NULL;",
    )?;

    let migration_result = (|| -> Result<()> {
        let links = {
            let mut stmt = conn.prepare(
                "SELECT link.project_id, link.skill_id, link.desired_tools, link.status,
                        link.created_at, link.updated_at, project.root_path, skill.enabled
                 FROM project_skill_links link
                 JOIN projects project ON project.id = link.project_id
                 JOIN skills skill ON skill.id = link.skill_id
                 WHERE link.enabled = 1",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i32>(7)? != 0,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        for (
            project_id,
            skill_id,
            desired_tools,
            link_status,
            created_at,
            updated_at,
            root,
            enabled,
        ) in links
        {
            for tool in string_vec_from_json(desired_tools) {
                let has_hub_target: bool = conn.query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM skill_targets
                       WHERE skill_id = ?1 AND tool = ?2
                         AND scope = 'project' AND project_path = ?3
                     )",
                    params![skill_id, tool, root],
                    |row| row.get(0),
                )?;
                let state = if !enabled {
                    "suspended"
                } else if matches!(link_status.as_str(), "error" | "conflict" | "partial") {
                    "error"
                } else {
                    "active"
                };
                let last_error = (state == "error").then_some(link_status.as_str());
                conn.execute(
                    "INSERT OR IGNORE INTO skill_assignments (
                       id, skill_id, tool, scope, project_id, ownership, state,
                       last_error, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, 'project', ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        format!("assignment:project:{project_id}:{skill_id}:{tool}"),
                        skill_id,
                        tool,
                        project_id,
                        if has_hub_target { "hub" } else { "native" },
                        state,
                        last_error,
                        created_at,
                        updated_at
                    ],
                )?;
            }
        }
        Ok(())
    })();

    match migration_result {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(err)
        }
    }
}

fn upsert_skill_with_conn(conn: &Connection, record: &SkillRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO skills (
          id, name, description, source_type, source_ref, source_subpath, source_revision, central_path, content_hash,
          created_at, updated_at, last_sync_at, last_seen_at, enabled, status
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
          ?10, ?11, ?12, ?13, ?14, ?15
        )
        ON CONFLICT(id) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          source_type = excluded.source_type,
          source_ref = excluded.source_ref,
          source_subpath = excluded.source_subpath,
          source_revision = excluded.source_revision,
          central_path = excluded.central_path,
          content_hash = excluded.content_hash,
          created_at = excluded.created_at,
          updated_at = excluded.updated_at,
          last_sync_at = excluded.last_sync_at,
          last_seen_at = excluded.last_seen_at,
          enabled = excluded.enabled,
          status = excluded.status",
        params![
            record.id,
            record.name,
            record.description,
            record.source_type,
            record.source_ref,
            record.source_subpath,
            record.source_revision,
            record.central_path,
            record.content_hash,
            record.created_at,
            record.updated_at,
            record.last_sync_at,
            record.last_seen_at,
            record.enabled as i32,
            record.status
        ],
    )?;
    Ok(())
}

fn upsert_skill_target_with_conn(conn: &Connection, record: &SkillTargetRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO skill_targets (
          id, skill_id, tool, scope, project_path, target_path, mode, status, last_error, synced_at
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
        )
        ON CONFLICT DO UPDATE SET
          target_path = excluded.target_path,
          mode = excluded.mode,
          status = excluded.status,
          last_error = excluded.last_error,
          synced_at = excluded.synced_at",
        params![
            record.id,
            record.skill_id,
            record.tool,
            record.scope,
            record.project_path,
            record.target_path,
            record.mode,
            record.status,
            record.last_error,
            record.synced_at
        ],
    )?;
    Ok(())
}

fn ensure_recycle_bin_schema(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(device_sync_tombstones)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
    drop(stmt);
    if !columns.contains("deletion_source") {
        conn.execute_batch(
            "ALTER TABLE device_sync_tombstones
             ADD COLUMN deletion_source TEXT NOT NULL DEFAULT 'sync';",
        )?;
    }
    if !columns.contains("metadata_json") {
        conn.execute_batch(
            "ALTER TABLE device_sync_tombstones ADD COLUMN metadata_json TEXT NULL;",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_device_sync_tombstones_expires_at
         ON device_sync_tombstones(expires_at);",
    )?;
    Ok(())
}

fn migrate_skill_templates_to_v10(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS skill_templates (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL UNIQUE COLLATE NOCASE,
           description TEXT NULL,
           mode TEXT NOT NULL CHECK(mode IN ('live', 'snapshot')),
           archived INTEGER NOT NULL DEFAULT 0,
           current_revision_id TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS skill_template_items (
           template_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           PRIMARY KEY(template_id, skill_id),
           FOREIGN KEY(template_id) REFERENCES skill_templates(id) ON DELETE CASCADE,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE RESTRICT
         );

         CREATE TABLE IF NOT EXISTS skill_template_revisions (
           id TEXT PRIMARY KEY,
           template_id TEXT NOT NULL,
           revision_number INTEGER NOT NULL,
           name TEXT NOT NULL,
           description TEXT NULL,
           created_at INTEGER NOT NULL,
           UNIQUE(template_id, revision_number),
           FOREIGN KEY(template_id) REFERENCES skill_templates(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS skill_template_revision_items (
           revision_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           skill_name TEXT NOT NULL,
           PRIMARY KEY(revision_id, skill_id),
           FOREIGN KEY(revision_id) REFERENCES skill_template_revisions(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS project_template_bindings (
           project_id TEXT PRIMARY KEY,
           template_id TEXT NOT NULL,
           applied_revision_id TEXT NOT NULL,
           sync_error TEXT NULL,
           last_attempt_at INTEGER NULL,
           synced_at INTEGER NULL,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(template_id) REFERENCES skill_templates(id) ON DELETE RESTRICT,
           FOREIGN KEY(applied_revision_id) REFERENCES skill_template_revisions(id) ON DELETE RESTRICT
         );

         CREATE INDEX IF NOT EXISTS idx_project_template_bindings_template
         ON project_template_bindings(template_id);

         CREATE TABLE IF NOT EXISTS project_skill_origins (
           project_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           source_type TEXT NOT NULL CHECK(source_type IN ('manual', 'template')),
           source_id TEXT NOT NULL,
           desired_tools TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(project_id, skill_id, source_type, source_id),
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE RESTRICT
         );

         CREATE INDEX IF NOT EXISTS idx_project_skill_origins_skill
         ON project_skill_origins(skill_id);

         CREATE INDEX IF NOT EXISTS idx_project_skill_origins_source
         ON project_skill_origins(source_type, source_id);

         CREATE TABLE IF NOT EXISTS project_template_overrides (
           project_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           kind TEXT NOT NULL CHECK(kind = 'native'),
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(project_id, skill_id),
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS project_template_failures (
           project_id TEXT NOT NULL,
           template_id TEXT NOT NULL,
           skill_id TEXT NOT NULL,
           tool TEXT NOT NULL,
           kind TEXT NOT NULL,
           error TEXT NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(project_id, skill_id, tool, kind),
           FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
           FOREIGN KEY(template_id) REFERENCES skill_templates(id) ON DELETE CASCADE,
           FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
         );

         INSERT OR IGNORE INTO project_skill_origins (
           project_id, skill_id, source_type, source_id, desired_tools, created_at, updated_at
         )
         SELECT project_id, skill_id, 'manual', '', desired_tools, created_at, updated_at
         FROM project_skill_links
         WHERE enabled = 1;
         COMMIT;",
    )?;
    Ok(())
}

fn migrate_prompt_revisions_to_v9(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prompt_revisions (
           id TEXT PRIMARY KEY,
           logical_path TEXT NOT NULL,
           real_path TEXT NULL,
           tool TEXT NOT NULL,
           profile_id TEXT NOT NULL,
           project_id TEXT NULL,
           operation TEXT NOT NULL,
           content BLOB NOT NULL,
           content_hash TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_prompt_revisions_path_created
         ON prompt_revisions(logical_path, created_at DESC);",
    )?;
    Ok(())
}

fn normalize_tag_name(name: &str) -> Result<String> {
    let normalized = name.trim().to_string();
    if normalized.is_empty() {
        anyhow::bail!("tag name cannot be empty");
    }
    Ok(normalized)
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

pub fn default_db_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf> {
    let app_dir = app
        .path()
        .app_data_dir()
        .context("failed to resolve app data dir")?;
    std::fs::create_dir_all(&app_dir)
        .with_context(|| format!("failed to create app data dir {:?}", app_dir))?;
    Ok(app_dir.join(DB_FILE_NAME))
}

pub fn migrate_legacy_db_if_needed(target_db_path: &Path) -> Result<()> {
    let Some(data_dir) = dirs::data_dir() else {
        return Ok(());
    };

    if let Ok(true) = db_has_any_skills(target_db_path) {
        return Ok(());
    }

    let legacy_db_path = LEGACY_APP_IDENTIFIERS
        .iter()
        .map(|id| data_dir.join(id).join(DB_FILE_NAME))
        .find(|path| path.exists());

    let Some(legacy_db_path) = legacy_db_path else {
        return Ok(());
    };

    if legacy_db_path == target_db_path {
        return Ok(());
    }

    if let Some(parent) = target_db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create app data dir {:?}", parent))?;
    }

    if target_db_path.exists() {
        let backup = target_db_path.with_extension(format!(
            "bak-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));
        std::fs::rename(target_db_path, &backup).with_context(|| {
            format!(
                "failed to backup existing db {:?} -> {:?}",
                target_db_path, backup
            )
        })?;
    }

    std::fs::copy(&legacy_db_path, target_db_path).with_context(|| {
        format!(
            "failed to migrate legacy db {:?} -> {:?}",
            legacy_db_path, target_db_path
        )
    })?;

    Ok(())
}

fn db_has_any_skills(db_path: &Path) -> Result<bool> {
    if !db_path.exists() {
        return Ok(false);
    }

    let conn =
        Connection::open(db_path).with_context(|| format!("failed to open db at {:?}", db_path))?;
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='skills';",
        [],
        |row| row.get(0),
    )?;
    if has_table == 0 {
        return Ok(false);
    }

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM skills;", [], |row| row.get(0))?;
    Ok(count > 0)
}

#[cfg(test)]
#[path = "tests/skill_store.rs"]
mod tests;
