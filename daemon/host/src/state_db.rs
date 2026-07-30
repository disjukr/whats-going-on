use std::fs;
use std::path::{Path, PathBuf};

use crate::db_schema::{MIGRATIONS, SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use rieul_daemon_core::rpc::{
    JobInfo, JobOutputState, JobOutputStream, JobRunReason, JobStatus, ScheduleInfo,
};
use rusqlite::{Connection, OptionalExtension, Transaction};

pub struct DaemonStateDb {
    path: PathBuf,
    connection: Connection,
}

pub struct StoredJobOutput {
    pub state: JobOutputState,
    pub chunks: Vec<StoredJobOutputChunk>,
}

pub struct StoredJobOutputChunk {
    pub seq: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentProject {
    pub project_id: String,
    pub title: String,
    pub root_path: String,
    pub created_at_ms: u64,
    pub last_opened_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentTaskWorkspace {
    pub task_workspace_id: String,
    pub root_path: String,
    pub source_kind: String,
    pub source_project_id: Option<String>,
    pub git_base_ref: Option<String>,
    pub copy_include_untracked: Option<bool>,
    pub state_kind: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentSession {
    pub session_id: String,
    pub provider_id: String,
    pub title: Option<String>,
    pub workspace_kind: String,
    pub project_id: Option<String>,
    pub task_workspace_id: Option<String>,
    pub cwd: String,
    pub creation_request_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentSession {
    pub session_id: String,
    pub provider_id: String,
    pub provider_session_id: Option<String>,
    pub title: Option<String>,
    pub workspace_kind: String,
    pub project_id: Option<String>,
    pub task_workspace_id: Option<String>,
    pub task_source_project_id: Option<String>,
    pub task_state_kind: Option<String>,
    pub cwd: String,
    pub archived: bool,
    pub latest_seq: u64,
    pub last_message_preview: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub active_turn_state_kind: Option<String>,
}

pub struct UpdatedAgentSession {
    pub session: StoredAgentSession,
    pub seq: u64,
    pub catalog_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredAgentWorkspaceFilter {
    Any,
    Project(Option<String>),
    Task(Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredAgentArchiveFilter {
    ActiveOnly,
    ArchivedOnly,
    All,
}

pub struct StoredAgentSessionQuery {
    pub workspace: StoredAgentWorkspaceFilter,
    pub archived: StoredAgentArchiveFilter,
    pub query: Option<String>,
    pub cursor: Option<(u64, String)>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentTurn {
    pub turn_id: String,
    pub session_id: String,
    pub state_kind: String,
    pub stop_reason_kind: Option<String>,
    pub stop_reason_other: Option<String>,
    pub failure_message: Option<String>,
    pub failure_code: Option<String>,
    pub failure_retryable: Option<bool>,
    pub created_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentTurnRecord {
    pub turn: StoredAgentTurn,
    pub completed_seq: u64,
    pub messages: Vec<StoredAgentMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentMessage {
    pub message_id: String,
    pub turn_id: Option<String>,
    pub role_kind: String,
    pub role_other: Option<String>,
    pub state_kind: String,
    pub created_at_ms: u64,
    pub content: Vec<StoredAgentMessageContent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredAgentMessageContent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data: Vec<u8>,
    },
    ResourceLink {
        uri: String,
        name: Option<String>,
        mime_type: Option<String>,
    },
    EmbeddedText {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
}

pub struct CreatedAgentTurn {
    pub turn_seq: u64,
    pub user_message_seq: u64,
    pub assistant_message_seq: u64,
    pub catalog_revision: u64,
}

pub struct FinishedAgentTurn {
    pub assistant_message_seq: u64,
    pub turn_seq: u64,
    pub catalog_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredAgentConfigValue {
    String { config_id: String, value: String },
    Boolean { config_id: String, value: bool },
}

impl DaemonStateDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create daemon state directory {}", parent.display()))?;
        }

        let mut connection = Connection::open(path)
            .with_context(|| format!("open daemon state database {}", path.display()))?;
        configure_connection(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            path: path.to_path_buf(),
            connection,
        })
    }

    pub fn open_in_memory_for_tests() -> Result<Self> {
        let mut connection =
            Connection::open_in_memory().context("open in-memory daemon state database")?;
        configure_connection(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            path: PathBuf::from(":memory:"),
            connection,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i32> {
        schema_version(&self.connection)
    }

    pub fn load_schedules(&self) -> Result<Vec<ScheduleInfo>> {
        let mut statement = self
            .connection
            .prepare("SELECT model FROM command_schedules ORDER BY created_at_ms")
            .context("prepare schedule load")?;
        let rows = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("load schedule rows")?;
        rows.into_iter()
            .map(|bytes| ScheduleInfo::decode(&bytes).context("decode stored schedule"))
            .collect()
    }

    pub fn load_jobs(&self) -> Result<Vec<JobInfo>> {
        let mut statement = self
            .connection
            .prepare("SELECT model FROM command_jobs ORDER BY created_at_ms")
            .context("prepare job load")?;
        let rows = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("load job rows")?;
        rows.into_iter()
            .map(|bytes| JobInfo::decode(&bytes).context("decode stored job"))
            .collect()
    }

    pub fn load_job_output(
        &self,
        job_id: &str,
        stream: JobOutputStream,
    ) -> Result<Option<StoredJobOutput>> {
        let stream = job_output_stream_name(stream);
        let state = self
            .connection
            .query_row(
                r#"
                SELECT enabled, oldest_seq, latest_seq, retained_bytes, truncated
                FROM command_job_output_state
                WHERE job_id = ?1 AND stream = ?2
                "#,
                (job_id, stream),
                |row| {
                    Ok(JobOutputState {
                        enabled: i64_to_bool(row.get::<_, i64>(0)?),
                        oldest_seq: row.get::<_, i64>(1)? as u64,
                        latest_seq: row.get::<_, i64>(2)? as u64,
                        retained_bytes: row.get::<_, i64>(3)? as u64,
                        truncated: i64_to_bool(row.get::<_, i64>(4)?),
                    })
                },
            )
            .optional()
            .context("load job output state")?;
        let Some(state) = state else {
            return Ok(None);
        };

        let mut statement = self
            .connection
            .prepare(
                r#"
                SELECT seq, bytes
                FROM command_job_output_chunks
                WHERE job_id = ?1 AND stream = ?2
                ORDER BY seq
                "#,
            )
            .context("prepare job output chunks load")?;
        let chunks = statement
            .query_map((job_id, stream), |row| {
                Ok(StoredJobOutputChunk {
                    seq: row.get::<_, i64>(0)? as u64,
                    bytes: row.get::<_, Vec<u8>>(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("load job output chunks")?;

        Ok(Some(StoredJobOutput { state, chunks }))
    }

    pub fn save_schedule(&self, schedule: &ScheduleInfo) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO command_schedules (
                  schedule_id, model, rrule_set, enabled, archived,
                  created_at_ms, updated_at_ms
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(schedule_id) DO UPDATE SET
                  model = excluded.model,
                  rrule_set = excluded.rrule_set,
                  enabled = excluded.enabled,
                  archived = excluded.archived,
                  updated_at_ms = excluded.updated_at_ms
                "#,
                (
                    &schedule.schedule_id,
                    schedule.encode(),
                    &schedule.rrule_set,
                    bool_to_i64(schedule.enabled),
                    bool_to_i64(schedule.archived),
                    schedule.created_at_ms as i64,
                    schedule.updated_at_ms as i64,
                ),
            )
            .context("save schedule")?;
        Ok(())
    }

    pub fn delete_schedule(&self, schedule_id: &str) -> Result<bool> {
        let removed = self
            .connection
            .execute(
                "DELETE FROM command_schedules WHERE schedule_id = ?1",
                [schedule_id],
            )
            .context("delete schedule")?;
        Ok(removed > 0)
    }

    pub fn save_job(&self, job: &JobInfo) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO command_jobs (
                  job_id, model, reason_kind, schedule_id, status_kind,
                  created_at_ms, started_at_ms, finished_at_ms
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(job_id) DO UPDATE SET
                  model = excluded.model,
                  reason_kind = excluded.reason_kind,
                  schedule_id = excluded.schedule_id,
                  status_kind = excluded.status_kind,
                  started_at_ms = excluded.started_at_ms,
                  finished_at_ms = excluded.finished_at_ms
                "#,
                (
                    &job.job_id,
                    job.encode(),
                    job_reason_kind(&job.reason),
                    job_schedule_id(&job.reason),
                    job_status_kind(&job.status),
                    job.created_at_ms as i64,
                    job.started_at_ms.map(|value| value as i64),
                    job.finished_at_ms.map(|value| value as i64),
                ),
            )
            .context("save job")?;
        Ok(())
    }

    pub fn delete_job(&self, job_id: &str) -> Result<bool> {
        let removed = self
            .connection
            .execute("DELETE FROM command_jobs WHERE job_id = ?1", [job_id])
            .context("delete job")?;
        Ok(removed > 0)
    }

    pub fn save_job_output_state(
        &self,
        job_id: &str,
        stream: JobOutputStream,
        state: &JobOutputState,
    ) -> Result<()> {
        let stream = job_output_stream_name(stream);
        self.connection
            .execute(
                r#"
                INSERT INTO command_job_output_state (
                  job_id, stream, enabled, oldest_seq, latest_seq,
                  retained_bytes, truncated, max_bytes
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
                ON CONFLICT(job_id, stream) DO UPDATE SET
                  enabled = excluded.enabled,
                  oldest_seq = excluded.oldest_seq,
                  latest_seq = excluded.latest_seq,
                  retained_bytes = excluded.retained_bytes,
                  truncated = excluded.truncated
                "#,
                (
                    job_id,
                    stream,
                    bool_to_i64(state.enabled),
                    state.oldest_seq as i64,
                    state.latest_seq as i64,
                    state.retained_bytes as i64,
                    bool_to_i64(state.truncated),
                ),
            )
            .context("save job output state")?;
        if state.oldest_seq > 0 {
            self.connection
                .execute(
                    r#"
                    DELETE FROM command_job_output_chunks
                    WHERE job_id = ?1 AND stream = ?2 AND seq < ?3
                    "#,
                    (job_id, stream, state.oldest_seq as i64),
                )
                .context("prune old job output chunks")?;
        } else {
            self.connection
                .execute(
                    r#"
                    DELETE FROM command_job_output_chunks
                    WHERE job_id = ?1 AND stream = ?2
                    "#,
                    (job_id, stream),
                )
                .context("prune disabled job output chunks")?;
        }
        Ok(())
    }

    pub fn append_job_output_chunk(
        &self,
        job_id: &str,
        stream: JobOutputStream,
        seq: u64,
        bytes: &[u8],
        created_at_ms: u64,
    ) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT OR REPLACE INTO command_job_output_chunks
                  (job_id, stream, seq, bytes, created_at_ms)
                SELECT ?1, ?2, ?3, ?4, ?5
                WHERE EXISTS (
                  SELECT 1
                  FROM command_job_output_state
                  WHERE job_id = ?1
                    AND stream = ?2
                    AND enabled != 0
                    AND oldest_seq > 0
                    AND ?3 >= oldest_seq
                    AND ?3 <= latest_seq
                )
                "#,
                (
                    job_id,
                    job_output_stream_name(stream),
                    seq as i64,
                    bytes,
                    created_at_ms as i64,
                ),
            )
            .context("append job output chunk")?;
        Ok(())
    }

    pub fn load_agent_projects(&self) -> Result<Vec<StoredAgentProject>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT project_id, title, root_path, created_at_ms, last_opened_at_ms
                 FROM agent_projects
                 ORDER BY COALESCE(last_opened_at_ms, created_at_ms) DESC, project_id",
            )
            .context("prepare agent projects load")?;
        let projects = statement
            .query_map([], agent_project_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("load agent projects")?;
        Ok(projects)
    }

    pub fn find_agent_project_by_id(&self, project_id: &str) -> Result<Option<StoredAgentProject>> {
        self.connection
            .query_row(
                "SELECT project_id, title, root_path, created_at_ms, last_opened_at_ms
                 FROM agent_projects WHERE project_id = ?1",
                [project_id],
                agent_project_from_row,
            )
            .optional()
            .context("find agent project by id")
    }

    pub fn find_agent_project_by_root_path(
        &self,
        root_path: &str,
    ) -> Result<Option<StoredAgentProject>> {
        self.connection
            .query_row(
                "SELECT project_id, title, root_path, created_at_ms, last_opened_at_ms
                 FROM agent_projects WHERE root_path = ?1",
                [root_path],
                agent_project_from_row,
            )
            .optional()
            .context("find agent project by root path")
    }

    pub fn insert_agent_project(&self, project: &StoredAgentProject) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO agent_projects (
                   project_id, title, root_path, created_at_ms, last_opened_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    &project.project_id,
                    &project.title,
                    &project.root_path,
                    project.created_at_ms as i64,
                    project.last_opened_at_ms.map(|value| value as i64),
                ),
            )
            .context("insert agent project")?;
        Ok(())
    }

    pub fn remove_agent_project(&self, project_id: &str) -> Result<bool> {
        let removed = self
            .connection
            .execute(
                "DELETE FROM agent_projects WHERE project_id = ?1",
                [project_id],
            )
            .context("remove agent project")?;
        Ok(removed > 0)
    }

    pub fn find_agent_session_by_creation_request(
        &self,
        creation_request_id: &str,
    ) -> Result<Option<StoredAgentSession>> {
        self.connection
            .query_row(
                &format!(
                    "{} WHERE s.creation_request_id = ?1",
                    agent_session_select()
                ),
                [creation_request_id],
                agent_session_from_row,
            )
            .optional()
            .context("find agent session by client request")
    }

    pub fn find_agent_session_by_id(&self, session_id: &str) -> Result<Option<StoredAgentSession>> {
        self.connection
            .query_row(
                &format!("{} WHERE s.session_id = ?1", agent_session_select()),
                [session_id],
                agent_session_from_row,
            )
            .optional()
            .context("find agent session by id")
    }

    pub fn create_agent_session(
        &mut self,
        session: &NewAgentSession,
        task_workspace: Option<&NewAgentTaskWorkspace>,
    ) -> Result<u64> {
        let tx = self
            .connection
            .transaction()
            .context("start agent session creation")?;
        if let Some(workspace) = task_workspace {
            tx.execute(
                "INSERT INTO agent_task_workspaces (
                   task_workspace_id, root_path, source_kind, source_project_id,
                   git_base_ref, copy_include_untracked, state_kind,
                   created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    &workspace.task_workspace_id,
                    &workspace.root_path,
                    &workspace.source_kind,
                    &workspace.source_project_id,
                    &workspace.git_base_ref,
                    workspace.copy_include_untracked.map(bool_to_i64),
                    &workspace.state_kind,
                    workspace.created_at_ms as i64,
                    workspace.updated_at_ms as i64,
                ),
            )
            .context("insert agent task workspace")?;
        }
        tx.execute(
            "INSERT INTO agent_sessions (
               session_id, provider_id, title, workspace_kind, project_id,
               task_workspace_id, cwd, archived, latest_seq,
               creation_request_id, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9, ?10)",
            (
                &session.session_id,
                &session.provider_id,
                &session.title,
                &session.workspace_kind,
                &session.project_id,
                &session.task_workspace_id,
                &session.cwd,
                &session.creation_request_id,
                session.created_at_ms as i64,
                session.updated_at_ms as i64,
            ),
        )
        .context("insert agent session")?;
        tx.execute(
            "UPDATE agent_session_catalog_state SET revision = revision + 1 WHERE singleton = 1",
            [],
        )
        .context("advance agent session catalog revision")?;
        let revision = tx
            .query_row(
                "SELECT revision FROM agent_session_catalog_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .context("read agent session catalog revision")? as u64;
        tx.execute(
            "INSERT INTO agent_session_catalog_changes (
               revision, session_id, change_kind, changed_at_ms
             ) VALUES (?1, ?2, 'changed', ?3)",
            (
                revision as i64,
                &session.session_id,
                session.updated_at_ms as i64,
            ),
        )
        .context("record agent session catalog change")?;
        tx.commit().context("commit agent session creation")?;
        Ok(revision)
    }

    pub fn list_agent_sessions(
        &self,
        query: &StoredAgentSessionQuery,
    ) -> Result<Vec<StoredAgentSession>> {
        let (workspace_kind, project_id, source_project_id) = match &query.workspace {
            StoredAgentWorkspaceFilter::Any => ("any", None, None),
            StoredAgentWorkspaceFilter::Project(project_id) => {
                ("project", project_id.as_deref(), None)
            }
            StoredAgentWorkspaceFilter::Task(source_project_id) => {
                ("task", None, source_project_id.as_deref())
            }
        };
        let archive_kind = match query.archived {
            StoredAgentArchiveFilter::ActiveOnly => "active",
            StoredAgentArchiveFilter::ArchivedOnly => "archived",
            StoredAgentArchiveFilter::All => "all",
        };
        let search = query.query.as_deref().map(agent_search_pattern);
        let (cursor_updated_at, cursor_session_id) = query
            .cursor
            .as_ref()
            .map(|(updated_at, session_id)| (Some(*updated_at as i64), Some(session_id.as_str())))
            .unwrap_or((None, None));
        let sql = format!(
            "{}
             WHERE (
               ?1 = 'any'
               OR (?1 = 'project' AND s.workspace_kind = 'project'
                   AND (?2 IS NULL OR s.project_id = ?2))
               OR (?1 = 'task' AND s.workspace_kind = 'task'
                   AND (?3 IS NULL OR tw.source_project_id = ?3))
             )
             AND (
               ?4 = 'all'
               OR (?4 = 'active' AND s.archived = 0)
               OR (?4 = 'archived' AND s.archived = 1)
             )
             AND (
               ?5 IS NULL
               OR s.title LIKE ?5 ESCAPE '\\'
               OR s.last_message_preview LIKE ?5 ESCAPE '\\'
             )
             AND (
               ?6 IS NULL
               OR s.updated_at_ms < ?6
               OR (s.updated_at_ms = ?6 AND s.session_id < ?7)
             )
             ORDER BY s.updated_at_ms DESC, s.session_id DESC
             LIMIT ?8",
            agent_session_select()
        );
        let mut statement = self
            .connection
            .prepare(&sql)
            .context("prepare agent sessions list")?;
        let sessions = statement
            .query_map(
                rusqlite::params![
                    workspace_kind,
                    project_id,
                    source_project_id,
                    archive_kind,
                    search,
                    cursor_updated_at,
                    cursor_session_id,
                    query.limit as i64,
                ],
                agent_session_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("list agent sessions")?;
        Ok(sessions)
    }

    pub fn update_agent_session(
        &mut self,
        session_id: &str,
        title: Option<Option<&str>>,
        archived: Option<bool>,
        now_ms: u64,
    ) -> Result<UpdatedAgentSession> {
        let tx = self
            .connection
            .transaction()
            .context("start agent session update")?;
        let updated = tx
            .execute(
                "UPDATE agent_sessions
                 SET title = CASE WHEN ?2 = 1 THEN ?3 ELSE title END,
                     archived = COALESCE(?4, archived)
                 WHERE session_id = ?1",
                rusqlite::params![
                    session_id,
                    bool_to_i64(title.is_some()),
                    title.flatten(),
                    archived.map(bool_to_i64),
                ],
            )
            .context("update agent session metadata")?;
        if updated == 0 {
            bail!("agent session was not found");
        }
        let seq = advance_agent_session_seq(&tx, session_id, 1, now_ms)?;
        let catalog_revision = record_agent_catalog_change(&tx, session_id, now_ms)?;
        tx.commit().context("commit agent session update")?;
        let session = self
            .find_agent_session_by_id(session_id)?
            .context("agent session disappeared after update")?;
        Ok(UpdatedAgentSession {
            session,
            seq,
            catalog_revision,
        })
    }

    pub fn agent_session_catalog_revision(&self) -> Result<u64> {
        self.connection
            .query_row(
                "SELECT revision FROM agent_session_catalog_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|revision| revision as u64)
            .context("read agent session catalog revision")
    }

    pub fn set_agent_provider_session_id(
        &self,
        session_id: &str,
        provider_session_id: &str,
        updated_at_ms: u64,
    ) -> Result<()> {
        let updated = self
            .connection
            .execute(
                "UPDATE agent_sessions
                 SET provider_session_id = ?2, updated_at_ms = ?3
                 WHERE session_id = ?1",
                (session_id, provider_session_id, updated_at_ms as i64),
            )
            .context("set agent provider session id")?;
        if updated == 0 {
            bail!("agent session was not found");
        }
        Ok(())
    }

    pub fn find_agent_turn_by_client_request(
        &self,
        session_id: &str,
        client_request_id: &str,
    ) -> Result<Option<StoredAgentTurn>> {
        self.connection
            .query_row(
                "SELECT turn_id, session_id, state_kind, stop_reason_kind,
                        stop_reason_other, failure_message, failure_code,
                        failure_retryable, created_at_ms, started_at_ms, finished_at_ms
                 FROM agent_session_turns
                 WHERE session_id = ?1 AND client_request_id = ?2",
                (session_id, client_request_id),
                stored_agent_turn_from_row,
            )
            .optional()
            .context("find agent turn by client request")
    }

    pub fn list_completed_agent_turns(
        &self,
        session_id: &str,
        through_seq: u64,
        cursor: Option<(u64, String)>,
        limit: usize,
    ) -> Result<Vec<StoredAgentTurnRecord>> {
        let cursor_seq = cursor.as_ref().map(|(seq, _)| *seq as i64);
        let cursor_turn_id = cursor.as_ref().map(|(_, turn_id)| turn_id.as_str());
        let mut statement = self
            .connection
            .prepare(
                "SELECT turn_id, session_id, state_kind, stop_reason_kind,
                        stop_reason_other, failure_message, failure_code,
                        failure_retryable, created_at_ms, started_at_ms, finished_at_ms,
                        completed_seq
                 FROM agent_session_turns
                 WHERE session_id = ?1
                   AND completed_seq IS NOT NULL
                   AND completed_seq <= ?2
                   AND (
                     ?3 IS NULL
                     OR completed_seq < ?3
                     OR (completed_seq = ?3 AND turn_id < ?4)
                   )
                 ORDER BY completed_seq DESC, turn_id DESC
                 LIMIT ?5",
            )
            .context("prepare completed agent turns list")?;
        let turns = statement
            .query_map(
                rusqlite::params![
                    session_id,
                    through_seq as i64,
                    cursor_seq,
                    cursor_turn_id,
                    limit as i64,
                ],
                |row| {
                    Ok((
                        stored_agent_turn_from_row(row)?,
                        row.get::<_, i64>(11)? as u64,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("list completed agent turns")?;

        turns
            .into_iter()
            .map(|(turn, completed_seq)| {
                let messages = self.load_agent_turn_messages(session_id, &turn.turn_id)?;
                Ok(StoredAgentTurnRecord {
                    turn,
                    completed_seq,
                    messages,
                })
            })
            .collect()
    }

    fn load_agent_turn_messages(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<Vec<StoredAgentMessage>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT message_id, turn_id, role_kind, role_other, state_kind, created_at_ms
                 FROM agent_messages
                 WHERE session_id = ?1 AND turn_id = ?2
                 ORDER BY created_at_ms,
                          CASE role_kind
                            WHEN 'user' THEN 0
                            WHEN 'assistant' THEN 1
                            ELSE 2
                          END,
                          message_id",
            )
            .context("prepare agent turn messages")?;
        let messages = statement
            .query_map((session_id, turn_id), |row| {
                Ok(StoredAgentMessage {
                    message_id: row.get(0)?,
                    turn_id: row.get(1)?,
                    role_kind: row.get(2)?,
                    role_other: row.get(3)?,
                    state_kind: row.get(4)?,
                    created_at_ms: row.get::<_, i64>(5)? as u64,
                    content: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("load agent turn messages")?;

        messages
            .into_iter()
            .map(|mut message| {
                message.content =
                    self.load_agent_message_content(session_id, &message.message_id)?;
                Ok(message)
            })
            .collect()
    }

    fn load_agent_message_content(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<Vec<StoredAgentMessageContent>> {
        type ContentRow = (
            String,
            Option<String>,
            Option<String>,
            Option<Vec<u8>>,
            Option<String>,
            Option<String>,
        );
        let mut statement = self
            .connection
            .prepare(
                "SELECT content_kind, text_value, mime_type, data, uri, name
                 FROM agent_message_contents
                 WHERE session_id = ?1 AND message_id = ?2
                 ORDER BY content_index",
            )
            .context("prepare agent message content")?;
        let rows = statement
            .query_map((session_id, message_id), |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<ContentRow>>>()
            .context("load agent message content")?;
        rows.into_iter()
            .map(
                |(kind, text, mime_type, data, uri, name)| match kind.as_str() {
                    "text" => Ok(StoredAgentMessageContent::Text {
                        text: text.context("text agent content has no text")?,
                    }),
                    "image" => Ok(StoredAgentMessageContent::Image {
                        mime_type: mime_type.context("image agent content has no MIME type")?,
                        data: data.context("image agent content has no data")?,
                    }),
                    "resource_link" => Ok(StoredAgentMessageContent::ResourceLink {
                        uri: uri.context("resource-link agent content has no URI")?,
                        name,
                        mime_type,
                    }),
                    "embedded_text" => Ok(StoredAgentMessageContent::EmbeddedText {
                        uri: uri.context("embedded-text agent content has no URI")?,
                        mime_type,
                        text: text.context("embedded-text agent content has no text")?,
                    }),
                    _ => bail!("unknown agent message content kind '{kind}'"),
                },
            )
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_text_turn(
        &mut self,
        session_id: &str,
        client_request_id: &str,
        turn_id: &str,
        user_message_id: &str,
        assistant_message_id: &str,
        text: &str,
        now_ms: u64,
    ) -> Result<CreatedAgentTurn> {
        let tx = self
            .connection
            .transaction()
            .context("start agent turn creation")?;
        tx.execute(
            "INSERT INTO agent_session_turns (
               turn_id, session_id, client_request_id, state_kind,
               created_at_ms, started_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, 'running', ?4, ?4, ?4)",
            (turn_id, session_id, client_request_id, now_ms as i64),
        )
        .context("insert agent turn")?;
        tx.execute(
            "INSERT INTO agent_messages (
               session_id, message_id, turn_id, role_kind, state_kind, created_at_ms
             ) VALUES (?1, ?2, ?3, 'user', 'complete', ?4)",
            (session_id, user_message_id, turn_id, now_ms as i64),
        )
        .context("insert agent user message")?;
        tx.execute(
            "INSERT INTO agent_message_contents (
               session_id, message_id, content_index, content_kind, text_value
             ) VALUES (?1, ?2, 0, 'text', ?3)",
            (session_id, user_message_id, text),
        )
        .context("insert agent user message content")?;
        tx.execute(
            "INSERT INTO agent_messages (
               session_id, message_id, turn_id, role_kind, state_kind, created_at_ms
             ) VALUES (?1, ?2, ?3, 'assistant', 'streaming', ?4)",
            (session_id, assistant_message_id, turn_id, now_ms as i64),
        )
        .context("insert streaming agent assistant message")?;
        let latest_seq = advance_agent_session_seq(&tx, session_id, 3, now_ms)?;
        let catalog_revision = record_agent_catalog_change(&tx, session_id, now_ms)?;
        tx.commit().context("commit agent turn creation")?;
        Ok(CreatedAgentTurn {
            turn_seq: latest_seq - 2,
            user_message_seq: latest_seq - 1,
            assistant_message_seq: latest_seq,
            catalog_revision,
        })
    }

    pub fn append_agent_text_chunk(
        &mut self,
        session_id: &str,
        message_id: &str,
        text: &str,
        now_ms: u64,
    ) -> Result<u64> {
        let tx = self
            .connection
            .transaction()
            .context("start agent message append")?;
        let content_index = tx
            .query_row(
                "SELECT COALESCE(MAX(content_index) + 1, 0)
                 FROM agent_message_contents
                 WHERE session_id = ?1 AND message_id = ?2",
                (session_id, message_id),
                |row| row.get::<_, i64>(0),
            )
            .context("read next agent message content index")?;
        tx.execute(
            "INSERT INTO agent_message_contents (
               session_id, message_id, content_index, content_kind, text_value
             ) VALUES (?1, ?2, ?3, 'text', ?4)",
            (session_id, message_id, content_index, text),
        )
        .context("append agent message text")?;
        let seq = advance_agent_session_seq(&tx, session_id, 1, now_ms)?;
        tx.commit().context("commit agent message append")?;
        Ok(seq)
    }

    pub fn advance_agent_session_sequence(&mut self, session_id: &str, now_ms: u64) -> Result<u64> {
        let tx = self
            .connection
            .transaction()
            .context("start agent session sequence advance")?;
        let seq = advance_agent_session_seq(&tx, session_id, 1, now_ms)?;
        tx.commit()
            .context("commit agent session sequence advance")?;
        Ok(seq)
    }

    pub fn replace_agent_session_config_values(
        &mut self,
        session_id: &str,
        values: &[StoredAgentConfigValue],
        now_ms: u64,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction()
            .context("start agent session config replacement")?;
        replace_agent_session_config_values(&tx, session_id, values, now_ms)?;
        tx.commit()
            .context("commit agent session config replacement")
    }

    pub fn replace_agent_session_config_values_and_advance(
        &mut self,
        session_id: &str,
        values: &[StoredAgentConfigValue],
        now_ms: u64,
    ) -> Result<u64> {
        let tx = self
            .connection
            .transaction()
            .context("start agent session config update")?;
        replace_agent_session_config_values(&tx, session_id, values, now_ms)?;
        let seq = advance_agent_session_seq(&tx, session_id, 1, now_ms)?;
        tx.commit().context("commit agent session config update")?;
        Ok(seq)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_agent_turn(
        &mut self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        state_kind: &str,
        stop_reason_kind: Option<&str>,
        stop_reason_other: Option<&str>,
        failure_message: Option<&str>,
        failure_code: Option<&str>,
        failure_retryable: Option<bool>,
        preview: Option<&str>,
        now_ms: u64,
    ) -> Result<FinishedAgentTurn> {
        let tx = self
            .connection
            .transaction()
            .context("start agent turn completion")?;
        let latest_seq = advance_agent_session_seq(&tx, session_id, 2, now_ms)?;
        tx.execute(
            "UPDATE agent_messages SET state_kind = 'complete'
             WHERE session_id = ?1 AND message_id = ?2",
            (session_id, assistant_message_id),
        )
        .context("complete agent assistant message")?;
        tx.execute(
            "UPDATE agent_session_turns
             SET state_kind = ?3, completed_seq = ?4,
                 stop_reason_kind = ?5, stop_reason_other = ?6,
                 failure_message = ?7, failure_code = ?8, failure_retryable = ?9,
                 finished_at_ms = ?10, updated_at_ms = ?10
             WHERE session_id = ?1 AND turn_id = ?2",
            rusqlite::params![
                session_id,
                turn_id,
                state_kind,
                latest_seq as i64,
                stop_reason_kind,
                stop_reason_other,
                failure_message,
                failure_code,
                failure_retryable.map(bool_to_i64),
                now_ms as i64,
            ],
        )
        .context("finish agent turn")?;
        tx.execute(
            "UPDATE agent_sessions SET last_message_preview = ?2 WHERE session_id = ?1",
            (session_id, preview),
        )
        .context("update agent session message preview")?;
        let catalog_revision = record_agent_catalog_change(&tx, session_id, now_ms)?;
        tx.commit().context("commit agent turn completion")?;
        Ok(FinishedAgentTurn {
            assistant_message_seq: latest_seq - 1,
            turn_seq: latest_seq,
            catalog_revision,
        })
    }
}

fn advance_agent_session_seq(
    tx: &Transaction<'_>,
    session_id: &str,
    count: u64,
    now_ms: u64,
) -> Result<u64> {
    let updated = tx
        .execute(
            "UPDATE agent_sessions
             SET latest_seq = latest_seq + ?2, updated_at_ms = ?3
             WHERE session_id = ?1",
            (session_id, count as i64, now_ms as i64),
        )
        .context("advance agent session sequence")?;
    if updated == 0 {
        bail!("agent session was not found");
    }
    tx.query_row(
        "SELECT latest_seq FROM agent_sessions WHERE session_id = ?1",
        [session_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|seq| seq as u64)
    .context("read agent session sequence")
}

fn replace_agent_session_config_values(
    tx: &Transaction<'_>,
    session_id: &str,
    values: &[StoredAgentConfigValue],
    now_ms: u64,
) -> Result<()> {
    tx.execute(
        "DELETE FROM agent_session_config_values WHERE session_id = ?1",
        [session_id],
    )
    .context("clear agent session config values")?;
    for value in values {
        match value {
            StoredAgentConfigValue::String { config_id, value } => {
                tx.execute(
                    "INSERT INTO agent_session_config_values (
                       session_id, config_id, value_kind, string_value, updated_at_ms
                     ) VALUES (?1, ?2, 'string', ?3, ?4)",
                    rusqlite::params![session_id, config_id, value, now_ms as i64],
                )
                .context("store agent session string config value")?;
            }
            StoredAgentConfigValue::Boolean { config_id, value } => {
                tx.execute(
                    "INSERT INTO agent_session_config_values (
                       session_id, config_id, value_kind, boolean_value, updated_at_ms
                     ) VALUES (?1, ?2, 'boolean', ?3, ?4)",
                    rusqlite::params![session_id, config_id, bool_to_i64(*value), now_ms as i64],
                )
                .context("store agent session boolean config value")?;
            }
        }
    }
    Ok(())
}

fn record_agent_catalog_change(tx: &Transaction<'_>, session_id: &str, now_ms: u64) -> Result<u64> {
    tx.execute(
        "UPDATE agent_session_catalog_state SET revision = revision + 1 WHERE singleton = 1",
        [],
    )
    .context("advance agent session catalog revision")?;
    let revision = tx
        .query_row(
            "SELECT revision FROM agent_session_catalog_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .context("read agent session catalog revision")? as u64;
    tx.execute(
        "INSERT INTO agent_session_catalog_changes (
           revision, session_id, change_kind, changed_at_ms
         ) VALUES (?1, ?2, 'changed', ?3)",
        (revision as i64, session_id, now_ms as i64),
    )
    .context("record agent session catalog change")?;
    Ok(revision)
}

fn stored_agent_turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAgentTurn> {
    Ok(StoredAgentTurn {
        turn_id: row.get(0)?,
        session_id: row.get(1)?,
        state_kind: row.get(2)?,
        stop_reason_kind: row.get(3)?,
        stop_reason_other: row.get(4)?,
        failure_message: row.get(5)?,
        failure_code: row.get(6)?,
        failure_retryable: row.get::<_, Option<i64>>(7)?.map(i64_to_bool),
        created_at_ms: row.get::<_, i64>(8)? as u64,
        started_at_ms: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
        finished_at_ms: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
    })
}

fn agent_project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAgentProject> {
    Ok(StoredAgentProject {
        project_id: row.get(0)?,
        title: row.get(1)?,
        root_path: row.get(2)?,
        created_at_ms: row.get::<_, i64>(3)? as u64,
        last_opened_at_ms: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
    })
}

fn agent_session_select() -> &'static str {
    "SELECT
       s.session_id, s.provider_id, s.provider_session_id, s.title,
       s.workspace_kind, s.project_id, s.task_workspace_id,
       tw.source_project_id, tw.state_kind, s.cwd, s.archived, s.latest_seq,
       s.last_message_preview, s.created_at_ms, s.updated_at_ms,
       active_turn.state_kind
     FROM agent_sessions s
     LEFT JOIN agent_task_workspaces tw
       ON tw.task_workspace_id = s.task_workspace_id
     LEFT JOIN agent_session_turns active_turn
       ON active_turn.session_id = s.session_id
      AND active_turn.state_kind IN ('queued', 'running', 'awaiting_permission')"
}

fn agent_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAgentSession> {
    Ok(StoredAgentSession {
        session_id: row.get(0)?,
        provider_id: row.get(1)?,
        provider_session_id: row.get(2)?,
        title: row.get(3)?,
        workspace_kind: row.get(4)?,
        project_id: row.get(5)?,
        task_workspace_id: row.get(6)?,
        task_source_project_id: row.get(7)?,
        task_state_kind: row.get(8)?,
        cwd: row.get(9)?,
        archived: i64_to_bool(row.get(10)?),
        latest_seq: row.get::<_, i64>(11)? as u64,
        last_message_preview: row.get(12)?,
        created_at_ms: row.get::<_, i64>(13)? as u64,
        updated_at_ms: row.get::<_, i64>(14)? as u64,
        active_turn_state_kind: row.get(15)?,
    })
}

fn agent_search_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            PRAGMA journal_mode = WAL;
            "#,
        )
        .context("configure daemon state database")
}

fn migrate(connection: &mut Connection) -> Result<()> {
    let version = schema_version(connection)?;
    if version > SCHEMA_VERSION {
        bail!(
            "daemon state database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        );
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }

    let tx = connection
        .transaction()
        .context("start daemon state database migration")?;
    for migration in MIGRATIONS.iter().skip(version as usize) {
        tx.execute_batch(migration.sql)
            .with_context(|| format!("apply daemon state database migration {}", migration.name))?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .context("set daemon state database schema version")?;
    tx.commit()
        .context("commit daemon state database migration")?;
    Ok(())
}

fn schema_version(connection: &Connection) -> Result<i32> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("read daemon state database schema version")
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn job_reason_kind(reason: &JobRunReason) -> &'static str {
    match reason {
        JobRunReason::Manual => "manual",
        JobRunReason::Schedule { .. } => "schedule",
    }
}

fn job_schedule_id(reason: &JobRunReason) -> Option<&str> {
    match reason {
        JobRunReason::Manual => None,
        JobRunReason::Schedule { schedule_id, .. } => Some(schedule_id.as_str()),
    }
}

fn job_status_kind(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Running => "running",
        JobStatus::Exited { .. } => "exited",
    }
}

fn job_output_stream_name(stream: JobOutputStream) -> &'static str {
    match stream {
        JobOutputStream::Stdout => "stdout",
        JobOutputStream::Stderr => "stderr",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_database_and_creates_schema() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();

        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(table_exists(&db.connection, "command_schedules"));
        assert!(table_exists(&db.connection, "command_jobs"));
        assert!(table_exists(&db.connection, "command_job_output_state"));
        assert!(table_exists(&db.connection, "command_job_output_chunks"));
        assert!(table_exists(&db.connection, "agent_projects"));
        assert!(table_exists(&db.connection, "agent_task_workspaces"));
        assert!(table_exists(&db.connection, "agent_sessions"));
        assert!(column_exists(
            &db.connection,
            "agent_sessions",
            "creation_request_id"
        ));
        assert!(table_exists(&db.connection, "agent_session_catalog_state"));
        assert!(table_exists(
            &db.connection,
            "agent_session_catalog_changes"
        ));
        assert!(table_exists(&db.connection, "agent_session_turns"));
        assert!(table_exists(&db.connection, "agent_turn_contexts"));
        assert!(table_exists(&db.connection, "agent_turn_context_entities"));
        assert!(table_exists(&db.connection, "agent_turn_context_resources"));
        assert!(table_exists(&db.connection, "agent_messages"));
        assert!(table_exists(&db.connection, "agent_message_contents"));
        assert!(table_exists(&db.connection, "agent_tool_calls"));
        assert!(table_exists(&db.connection, "agent_tool_call_locations"));
        assert!(table_exists(&db.connection, "agent_tool_call_contents"));
        assert!(table_exists(&db.connection, "agent_permission_requests"));
        assert!(table_exists(&db.connection, "agent_permission_options"));
        assert!(table_exists(&db.connection, "agent_turn_plans"));
        assert!(table_exists(&db.connection, "agent_plan_entries"));
        assert!(table_exists(&db.connection, "agent_session_config_values"));
        assert!(table_exists(&db.connection, "agent_terminals"));
        assert!(table_exists(&db.connection, "agent_terminal_args"));
        assert!(table_exists(&db.connection, "agent_terminal_output_chunks"));
    }

    #[test]
    fn creates_parent_directory_for_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("daemon-state.sqlite3");

        let db = DaemonStateDb::open(&path).unwrap();

        assert_eq!(db.path(), path.as_path());
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(path.exists());
    }

    #[test]
    fn enables_foreign_keys() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        let enabled: i32 = db
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();

        assert_eq!(enabled, 1);
    }

    #[test]
    fn rejects_newer_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon-state.sqlite3");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }

        let err = match DaemonStateDb::open(&path) {
            Ok(_) => panic!("newer schema version should be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("newer than supported"));
    }

    #[test]
    fn migrates_existing_v1_database_to_agent_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon-state.sqlite3");
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(MIGRATIONS[0].sql).unwrap();
            connection.pragma_update(None, "user_version", 1).unwrap();
        }

        let db = DaemonStateDb::open(&path).unwrap();

        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(table_exists(&db.connection, "agent_sessions"));
        assert!(table_exists(&db.connection, "agent_session_turns"));
    }

    #[test]
    fn removing_agent_project_preserves_session_reference() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        db.connection
            .execute(
                "INSERT INTO agent_projects (
                    project_id, title, root_path, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4)",
                ("project-1", "Project", "/project", 1_i64),
            )
            .unwrap();
        db.connection
            .execute(
                "INSERT INTO agent_sessions (
                    session_id, provider_id, workspace_kind, project_id, cwd, creation_request_id,
                    archived, created_at_ms, updated_at_ms
                ) VALUES (?1, ?2, 'project', ?3, ?4, ?5, 0, ?6, ?6)",
                (
                    "session-1",
                    "agent-1",
                    "project-1",
                    "/project",
                    "creation-1",
                    1_i64,
                ),
            )
            .unwrap();

        db.connection
            .execute(
                "DELETE FROM agent_projects WHERE project_id = ?1",
                ["project-1"],
            )
            .unwrap();

        let project_id = db
            .connection
            .query_row(
                "SELECT project_id FROM agent_sessions WHERE session_id = ?1",
                ["session-1"],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(project_id, "project-1");
    }

    #[test]
    fn persists_streamed_agent_text_turn_as_normalized_rows() {
        let mut db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        db.create_agent_session(
            &NewAgentSession {
                session_id: "session-text".to_string(),
                provider_id: "codex".to_string(),
                title: Some("Synthetic session".to_string()),
                workspace_kind: "task".to_string(),
                project_id: None,
                task_workspace_id: Some("workspace-text".to_string()),
                cwd: "<temporary-workspace>".to_string(),
                creation_request_id: "create-synthetic".to_string(),
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            Some(&NewAgentTaskWorkspace {
                task_workspace_id: "workspace-text".to_string(),
                root_path: "<temporary-workspace>".to_string(),
                source_kind: "empty".to_string(),
                source_project_id: None,
                git_base_ref: None,
                copy_include_untracked: None,
                state_kind: "ready".to_string(),
                created_at_ms: 1,
                updated_at_ms: 1,
            }),
        )
        .unwrap();

        let created = db
            .create_agent_text_turn(
                "session-text",
                "turn-request-synthetic",
                "turn-text",
                "message-user",
                "message-assistant",
                "Say hello",
                2,
            )
            .unwrap();
        assert_eq!(
            (
                created.turn_seq,
                created.user_message_seq,
                created.assistant_message_seq
            ),
            (1, 2, 3)
        );
        assert_eq!(
            db.append_agent_text_chunk("session-text", "message-assistant", "Hello", 3)
                .unwrap(),
            4
        );
        let finished = db
            .finish_agent_turn(
                "session-text",
                "turn-text",
                "message-assistant",
                "completed",
                Some("end_turn"),
                None,
                None,
                None,
                None,
                Some("Hello"),
                4,
            )
            .unwrap();
        assert_eq!((finished.assistant_message_seq, finished.turn_seq), (5, 6));

        let turn = db
            .find_agent_turn_by_client_request("session-text", "turn-request-synthetic")
            .unwrap()
            .unwrap();
        assert_eq!(turn.state_kind, "completed");
        assert_eq!(turn.stop_reason_kind.as_deref(), Some("end_turn"));
        let session = db
            .find_agent_session_by_id("session-text")
            .unwrap()
            .unwrap();
        assert_eq!(session.latest_seq, 6);
        assert_eq!(session.last_message_preview.as_deref(), Some("Hello"));
        let contents = db
            .connection
            .query_row(
                "SELECT group_concat(text_value, '')
                 FROM agent_message_contents
                 WHERE session_id = 'session-text' AND message_id = 'message-assistant'
                 ORDER BY content_index",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(contents, "Hello");

        let history = db
            .list_completed_agent_turns("session-text", session.latest_seq, None, 10)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].turn.turn_id, "turn-text");
        assert_eq!(history[0].completed_seq, 6);
        assert_eq!(
            history[0].messages,
            vec![
                StoredAgentMessage {
                    message_id: "message-user".to_string(),
                    turn_id: Some("turn-text".to_string()),
                    role_kind: "user".to_string(),
                    role_other: None,
                    state_kind: "complete".to_string(),
                    created_at_ms: 2,
                    content: vec![StoredAgentMessageContent::Text {
                        text: "Say hello".to_string(),
                    }],
                },
                StoredAgentMessage {
                    message_id: "message-assistant".to_string(),
                    turn_id: Some("turn-text".to_string()),
                    role_kind: "assistant".to_string(),
                    role_other: None,
                    state_kind: "complete".to_string(),
                    created_at_ms: 2,
                    content: vec![StoredAgentMessageContent::Text {
                        text: "Hello".to_string(),
                    }],
                },
            ]
        );
        assert!(db
            .list_completed_agent_turns("session-text", 5, None, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn replaces_agent_config_values_and_advances_session_sequence() {
        let mut db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        db.create_agent_session(
            &NewAgentSession {
                session_id: "session-config".to_string(),
                provider_id: "codex".to_string(),
                title: None,
                workspace_kind: "task".to_string(),
                project_id: None,
                task_workspace_id: Some("workspace-config".to_string()),
                cwd: "<temporary-workspace>".to_string(),
                creation_request_id: "create-config".to_string(),
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            Some(&NewAgentTaskWorkspace {
                task_workspace_id: "workspace-config".to_string(),
                root_path: "<temporary-workspace>".to_string(),
                source_kind: "empty".to_string(),
                source_project_id: None,
                git_base_ref: None,
                copy_include_untracked: None,
                state_kind: "ready".to_string(),
                created_at_ms: 1,
                updated_at_ms: 1,
            }),
        )
        .unwrap();

        db.replace_agent_session_config_values(
            "session-config",
            &[
                StoredAgentConfigValue::String {
                    config_id: "reasoning_effort".to_string(),
                    value: "medium".to_string(),
                },
                StoredAgentConfigValue::Boolean {
                    config_id: "fast_mode".to_string(),
                    value: false,
                },
            ],
            2,
        )
        .unwrap();
        let seq = db
            .replace_agent_session_config_values_and_advance(
                "session-config",
                &[StoredAgentConfigValue::String {
                    config_id: "reasoning_effort".to_string(),
                    value: "high".to_string(),
                }],
                3,
            )
            .unwrap();

        assert_eq!(seq, 1);
        let rows = db
            .connection
            .prepare(
                "SELECT config_id, value_kind, string_value, boolean_value
                 FROM agent_session_config_values
                 WHERE session_id = ?1
                 ORDER BY config_id",
            )
            .unwrap()
            .query_map(["session-config"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![(
                "reasoning_effort".to_string(),
                "string".to_string(),
                Some("high".to_string()),
                None,
            )]
        );
    }

    #[test]
    fn normalized_agent_state_cascades_with_session() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        db.connection
            .execute_batch(
                r#"
                INSERT INTO agent_sessions (
                  session_id, provider_id, workspace_kind, project_id, cwd, creation_request_id,
                  archived, created_at_ms, updated_at_ms
                ) VALUES (
                  'session-1', 'agent-1', 'project', 'project-1', '/project',
                  'creation-1', 0, 1, 1
                );

                INSERT INTO agent_session_turns (
                  turn_id, session_id, client_request_id, state_kind,
                  completed_seq, stop_reason_kind, created_at_ms,
                  finished_at_ms, updated_at_ms
                ) VALUES (
                  'turn-1', 'session-1', 'request-1', 'completed',
                  1, 'end_turn', 1, 2, 2
                );

                INSERT INTO agent_turn_contexts (
                  turn_id, captured_at_ms, daemon_instance_id, surface_id, truncated
                ) VALUES ('turn-1', 1, 'daemon-1', 'daemon.process.detail', 0);

                INSERT INTO agent_turn_context_entities (
                  turn_id, entity_index, role_kind, entity_kind, filesystem_path
                ) VALUES ('turn-1', 0, 'primary', 'filesystem_path', '/project');

                INSERT INTO agent_turn_context_resources (
                  turn_id, resource_index, role_kind, uri, name,
                  snapshot_mime_type, snapshot_json
                ) VALUES (
                  'turn-1', 0, 'primary', 'rieul://process/1', 'Process 1',
                  'application/json', '{"pid":1}'
                );

                INSERT INTO agent_messages (
                  session_id, message_id, turn_id, role_kind, state_kind, created_at_ms
                ) VALUES ('session-1', 'message-1', 'turn-1', 'assistant', 'complete', 1);

                INSERT INTO agent_message_contents (
                  session_id, message_id, content_index, content_kind, text_value
                ) VALUES ('session-1', 'message-1', 0, 'text', 'done');

                INSERT INTO agent_tool_calls (
                  session_id, tool_call_id, turn_id, title, kind, status_kind
                ) VALUES ('session-1', 'tool-1', 'turn-1', 'Read file', 'read', 'completed');

                INSERT INTO agent_tool_call_locations (
                  session_id, tool_call_id, location_index, path, line
                ) VALUES ('session-1', 'tool-1', 0, '/project/file.txt', 1);

                INSERT INTO agent_tool_call_contents (
                  session_id, tool_call_id, content_index, content_type,
                  diff_path, diff_kind, diff_patch
                ) VALUES (
                  'session-1', 'tool-1', 0, 'diff',
                  '/project/file.txt', 'modify', '@@ -1 +1 @@'
                );

                INSERT INTO agent_permission_requests (
                  session_id, permission_request_id, turn_id, subject_kind,
                  action_title, state_kind, selected_option_id, created_at_ms
                ) VALUES (
                  'session-1', 'permission-1', 'turn-1', 'action',
                  'Continue', 'selected', 'allow', 1
                );

                INSERT INTO agent_permission_options (
                  session_id, permission_request_id, option_index,
                  option_id, title, kind
                ) VALUES (
                  'session-1', 'permission-1', 0, 'allow', 'Allow', 'allow_once'
                );

                INSERT INTO agent_turn_plans (session_id, turn_id)
                VALUES ('session-1', 'turn-1');

                INSERT INTO agent_plan_entries (
                  session_id, turn_id, entry_id, entry_index,
                  content, priority_kind, status_kind
                ) VALUES (
                  'session-1', 'turn-1', 'entry-1', 0,
                  'Inspect state', 'medium', 'completed'
                );

                INSERT INTO agent_session_config_values (
                  session_id, config_id, value_kind, boolean_value, updated_at_ms
                ) VALUES ('session-1', 'thinking', 'boolean', 1, 1);

                INSERT INTO agent_terminals (
                  session_id, terminal_id, turn_id, command, truncated,
                  oldest_output_seq, latest_output_seq, retained_bytes,
                  exited, exit_code
                ) VALUES (
                  'session-1', 'terminal-1', 'turn-1', 'echo', 0,
                  1, 1, 3, 1, 0
                );

                INSERT INTO agent_terminal_args (
                  session_id, terminal_id, arg_index, value
                ) VALUES ('session-1', 'terminal-1', 0, 'hi');

                INSERT INTO agent_terminal_output_chunks (
                  session_id, terminal_id, seq, text, created_at_ms
                ) VALUES ('session-1', 'terminal-1', 1, 'hi\n', 1);

                DELETE FROM agent_sessions WHERE session_id = 'session-1';
                "#,
            )
            .unwrap();

        for table in [
            "agent_session_turns",
            "agent_turn_contexts",
            "agent_turn_context_entities",
            "agent_turn_context_resources",
            "agent_messages",
            "agent_message_contents",
            "agent_tool_calls",
            "agent_tool_call_locations",
            "agent_tool_call_contents",
            "agent_permission_requests",
            "agent_permission_options",
            "agent_turn_plans",
            "agent_plan_entries",
            "agent_session_config_values",
            "agent_terminals",
            "agent_terminal_args",
            "agent_terminal_output_chunks",
        ] {
            assert_eq!(table_row_count(&db.connection, table), 0, "{table}");
        }
    }

    #[test]
    fn prunes_output_chunks_older_than_retained_state() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        let job = test_job();
        db.save_job(&job).unwrap();
        db.save_job_output_state(
            &job.job_id,
            JobOutputStream::Stdout,
            &JobOutputState {
                enabled: true,
                oldest_seq: 1,
                latest_seq: 3,
                retained_bytes: 3,
                truncated: false,
            },
        )
        .unwrap();
        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 1, b"a", 1)
            .unwrap();
        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 2, b"b", 2)
            .unwrap();
        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 3, b"c", 3)
            .unwrap();

        db.save_job_output_state(
            &job.job_id,
            JobOutputStream::Stdout,
            &JobOutputState {
                enabled: true,
                oldest_seq: 3,
                latest_seq: 3,
                retained_bytes: 1,
                truncated: true,
            },
        )
        .unwrap();

        let remaining = output_chunk_seqs(&db, &job.job_id, "stdout");
        assert_eq!(remaining, vec![3]);
    }

    #[test]
    fn ignores_output_chunks_not_retained_by_state() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        let job = test_job();
        db.save_job(&job).unwrap();
        db.save_job_output_state(
            &job.job_id,
            JobOutputStream::Stdout,
            &JobOutputState {
                enabled: true,
                oldest_seq: 0,
                latest_seq: 1,
                retained_bytes: 0,
                truncated: true,
            },
        )
        .unwrap();

        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 1, b"evicted", 1)
            .unwrap();

        let remaining = output_chunk_seqs(&db, &job.job_id, "stdout");
        assert!(remaining.is_empty());
    }

    #[test]
    fn loads_persisted_output_state_and_chunks() {
        let db = DaemonStateDb::open_in_memory_for_tests().unwrap();
        let job = test_job();
        db.save_job(&job).unwrap();
        db.save_job_output_state(
            &job.job_id,
            JobOutputStream::Stdout,
            &JobOutputState {
                enabled: true,
                oldest_seq: 2,
                latest_seq: 3,
                retained_bytes: 2,
                truncated: true,
            },
        )
        .unwrap();
        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 2, b"b", 2)
            .unwrap();
        db.append_job_output_chunk(&job.job_id, JobOutputStream::Stdout, 3, b"c", 3)
            .unwrap();

        let output = db
            .load_job_output(&job.job_id, JobOutputStream::Stdout)
            .unwrap()
            .expect("stored output");

        assert_eq!(output.state.oldest_seq, 2);
        assert_eq!(output.state.latest_seq, 3);
        assert!(output.state.truncated);
        assert_eq!(
            output
                .chunks
                .into_iter()
                .map(|chunk| (chunk.seq, chunk.bytes))
                .collect::<Vec<_>>(),
            vec![(2, b"b".to_vec()), (3, b"c".to_vec())]
        );
    }

    fn table_exists(connection: &Connection, table_name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = ?1
                )",
                [table_name],
                |row| row.get::<_, i32>(0),
            )
            .unwrap()
            == 1
    }

    fn column_exists(connection: &Connection, table_name: &str, column_name: &str) -> bool {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .unwrap();
        let exists = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .any(|name| name.is_ok_and(|name| name == column_name));
        exists
    }

    fn output_chunk_seqs(db: &DaemonStateDb, job_id: &str, stream: &str) -> Vec<i64> {
        let mut statement = db
            .connection
            .prepare(
                "SELECT seq FROM command_job_output_chunks
                WHERE job_id = ?1 AND stream = ?2
                ORDER BY seq",
            )
            .unwrap();
        statement
            .query_map((job_id, stream), |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn table_row_count(connection: &Connection, table_name: &str) -> i64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table_name}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn test_job() -> JobInfo {
        JobInfo {
            job_id: "job-test".to_string(),
            title: None,
            launch: rieul_daemon_core::rpc::CommandLaunchSpec {
                command: "test".to_string(),
                args: Vec::new(),
                cwd: None,
                env: None,
                stdin: None,
                elevated: None,
            },
            created_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            status: JobStatus::Running,
            reason: JobRunReason::Manual,
            log: rieul_daemon_core::rpc::JobLogState {
                stdout: JobOutputState {
                    enabled: true,
                    oldest_seq: 0,
                    latest_seq: 0,
                    retained_bytes: 0,
                    truncated: false,
                },
                stderr: JobOutputState {
                    enabled: false,
                    oldest_seq: 0,
                    latest_seq: 0,
                    retained_bytes: 0,
                    truncated: false,
                },
            },
        }
    }
}
