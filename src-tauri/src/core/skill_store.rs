use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::Manager;

const DB_FILE_NAME: &str = "skills_hub.db";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.tauri.dev", "com.tauri.dev.skillshub"];

// Schema versioning: bump when making changes and add a migration step.
const SCHEMA_VERSION: i32 = 7;

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
"#;

#[derive(Clone, Debug)]
pub struct SkillStore {
    db_path: PathBuf,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if user_version > SCHEMA_VERSION {
                anyhow::bail!(
                    "database schema version {} is newer than app supports {}",
                    user_version,
                    SCHEMA_VERSION
                );
            }

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

    pub fn delete_skill(&self, skill_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM skills WHERE id = ?1", params![skill_id])?;
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

    pub fn update_skill_target_status(
        &self,
        skill_id: &str,
        tool: &str,
        scope: &str,
        project_path: Option<&str>,
        status: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE skill_targets
                 SET status = ?5
                 WHERE skill_id = ?1
                   AND tool = ?2
                   AND scope = ?3
                   AND ((?4 IS NULL AND project_path IS NULL) OR project_path = ?4)",
                params![skill_id, tool, scope, project_path, status],
            )?;
            Ok(())
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
