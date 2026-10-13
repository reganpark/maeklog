use crate::AppResult;
use crate::config::private_dir;
use crate::model::{
    ContextTiming, CurrentState, EventKind, RebuildRequest, RepoSnapshot, SessionReference,
    SpaceMeta, Summary, TurnEvent, UiState, WorkerStatus,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EVENT_BYTES: usize = 80_000;
const FILE_MODE: u32 = 0o600;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct StaleRevision {
    pub expected: u64,
    pub actual: u64,
}

impl std::fmt::Display for StaleRevision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "stale summary revision: expected {}, found {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for StaleRevision {}

#[derive(Clone, Debug)]
pub struct NewTurnEvent {
    pub kind: EventKind,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub model: Option<String>,
    pub content: String,
    pub cwd: String,
}

#[derive(Clone, Debug)]
pub struct SpaceStore {
    pub state_root: PathBuf,
    pub key: String,
}

impl SpaceStore {
    #[cfg(test)]
    pub fn for_workspace(
        state_root: &Path,
        session_name: &str,
        workspace_id: &str,
        cwd: &str,
        label: Option<&str>,
    ) -> AppResult<Self> {
        Self::for_scope(state_root, session_name, workspace_id, None, cwd, label)
    }
    pub fn for_scope(
        state_root: &Path,
        session_name: &str,
        workspace_id: &str,
        tab_id: Option<&str>,
        cwd: &str,
        label: Option<&str>,
    ) -> AppResult<Self> {
        if workspace_id.is_empty() {
            return Err("workspace id is required".into());
        }
        private_dir(state_root.to_path_buf())?;
        private_dir(state_root.join("spaces"))?;
        let store = Self {
            state_root: state_root.to_path_buf(),
            key: make_scope_key(session_name, workspace_id, tab_id),
        };
        private_dir(store.directory())?;
        let _init = lock_file(&store.path(".init.lock"), false)?;
        let fresh = !store.path("meta.json").exists();
        if fresh && tab_id.is_some() {
            store.set_ui(UiState {
                visible: false,
                ..UiState::default()
            })?;
            if let Some(legacy) = Self::find_for_workspace(state_root, session_name, workspace_id) {
                let preference = crate::participation::load(&legacy)?;
                if !preference.enabled {
                    crate::participation::set_enabled(&store, false)?;
                }
            }
        }
        store.update_scoped_meta(session_name, workspace_id, tab_id, cwd, label)?;
        Ok(store)
    }
    pub fn find_for_workspace(
        state_root: &Path,
        session_name: &str,
        workspace_id: &str,
    ) -> Option<Self> {
        Self::find_for_scope(state_root, session_name, workspace_id, None)
    }
    pub fn find_for_scope(
        state_root: &Path,
        session_name: &str,
        workspace_id: &str,
        tab_id: Option<&str>,
    ) -> Option<Self> {
        let store = Self {
            state_root: state_root.to_path_buf(),
            key: make_scope_key(session_name, workspace_id, tab_id),
        };
        store.directory().is_dir().then_some(store)
    }
    pub fn for_caller(state_root: &Path) -> AppResult<Self> {
        let target = crate::herdr::caller_context(false)?;
        Self::find_for_scope(
            state_root,
            &crate::config::herdr_session_name(),
            &target.workspace_id,
            target.tab_id.as_deref(),
        )
        .ok_or_else(|| {
            crate::tr!(
                "이 탭에서 Maeklog 패널을 먼저 열어 주세요.",
                "Open the Maeklog panel in this tab first."
            )
            .into()
        })
    }

    /// Mixed pre-tab queues remain an inspectable archive, not a live tab.
    pub fn legacy_with_tabs(&self) -> AppResult<bool> {
        let Some(meta) = self.meta()? else {
            return Ok(false);
        };
        if meta.tab_id.is_some() {
            return Ok(false);
        }
        Ok(Self::all(&self.state_root)?.iter().any(|other| {
            other.meta().ok().flatten().is_some_and(|m| {
                m.tab_id.is_some()
                    && m.workspace_id == meta.workspace_id
                    && m.session_name == meta.session_name
            })
        }))
    }
    pub fn all(state_root: &Path) -> AppResult<Vec<Self>> {
        let directory = state_root.join("spaces");
        if !directory.is_dir() {
            return Ok(Vec::new());
        }
        let mut stores = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stores.push(Self {
                    state_root: state_root.to_path_buf(),
                    key: entry.file_name().to_string_lossy().into_owned(),
                });
            }
        }
        stores.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(stores)
    }

    pub fn find_by_panel_pane(state_root: &Path, pane_id: &str) -> AppResult<Option<Self>> {
        for store in Self::all(state_root)? {
            if store.ui()?.panel_pane_id.as_deref() == Some(pane_id) {
                return Ok(Some(store));
            }
        }
        Ok(None)
    }

    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.state_root.join("spaces").join(&self.key)
    }

    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.directory().join(name)
    }

    pub fn meta(&self) -> AppResult<Option<SpaceMeta>> {
        read_json_optional(&self.path("meta.json"))
    }

    fn update_scoped_meta(
        &self,
        session_name: &str,
        workspace_id: &str,
        tab_id: Option<&str>,
        cwd: &str,
        label: Option<&str>,
    ) -> AppResult<SpaceMeta> {
        let _lock = lock_file(&self.path(".meta.lock"), false)?;
        let previous: Option<SpaceMeta> = read_json_optional(&self.path("meta.json"))?;
        let now = unix_now();
        let fallback_label = Path::new(cwd).file_name().map_or_else(
            || workspace_id.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let value = SpaceMeta {
            tab_id: tab_id.map(ToOwned::to_owned),
            space_key: self.key.clone(),
            workspace_id: workspace_id.to_string(),
            session_name: session_name.to_string(),
            cwd: canonical_or_original(cwd),
            label: label
                .map(ToOwned::to_owned)
                .or_else(|| previous.as_ref().map(|meta| meta.label.clone()))
                .unwrap_or(fallback_label),
            created_at_unix: previous.as_ref().map_or(now, |meta| meta.created_at_unix),
            last_seen_at_unix: now,
            archived: false,
            archived_at_unix: None,
        };
        atomic_write_json(&self.path("meta.json"), &value)?;
        Ok(value)
    }

    pub fn append_event(&self, event: NewTurnEvent) -> AppResult<TurnEvent> {
        let _lock = lock_file(&self.path(".event.lock"), false)?;
        crate::legacy::preserve(self)?;
        let sequence: serde_json::Value = read_json_or_default(&self.path("sequence.json"))?;
        let seq = sequence
            .get("next_seq")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1)
            .max(
                crate::branch_context::sources(self)?
                    .last_key_value()
                    .map_or(1, |(seq, _)| seq.saturating_add(1)),
            );
        let (content, truncated) = truncate_utf8(event.content, MAX_EVENT_BYTES);
        let stored = TurnEvent {
            seq,
            kind: event.kind,
            session_id: event.session_id,
            turn_id: event.turn_id,
            model: event.model,
            content,
            truncated,
            recorded_at_unix: unix_now(),
            cwd: event.cwd,
        };
        if !crate::branch_context::capture(self, &stored)? {
            return Ok(stored);
        }
        atomic_write_json(
            &self.path("sequence.json"),
            &serde_json::json!({ "next_seq": seq + 1 }),
        )?;
        if let Err(error) = crate::task_links::capture_event(self, &stored) {
            eprintln!("maeklog task capture: {error}");
        }
        Ok(stored)
    }

    pub fn remember_session(&self, session_id: &str) -> AppResult<()> {
        if session_id.is_empty() {
            return Ok(());
        }
        atomic_write_json(
            &self.path("session.json"),
            &SessionReference {
                session_id: session_id.to_string(),
                updated_at_unix: unix_now(),
            },
        )
    }

    pub fn latest_session_id(&self) -> AppResult<Option<String>> {
        let reference: Option<SessionReference> = read_json_optional(&self.path("session.json"))?;
        if let Some(reference) = reference {
            return Ok(Some(reference.session_id));
        }
        if let Some(session_id) = self
            .events()?
            .into_iter()
            .rev()
            .map(|event| event.session_id)
            .find(|session_id| !session_id.is_empty())
        {
            return Ok(Some(session_id));
        }
        Ok(None)
    }

    #[cfg(test)]
    pub fn request_rebuild(&self, session_id: &str) -> AppResult<()> {
        if session_id.is_empty() {
            return Err("a Codex session id is required to rebuild context".into());
        }
        let _lock = lock_file(&self.path(".rebuild.lock"), false)?;
        atomic_write_json(
            &self.path("rebuild.json"),
            &RebuildRequest {
                session_id: session_id.to_string(),
                requested_at_unix: unix_now(),
            },
        )
    }

    #[cfg(test)]
    pub fn schedule_initial_rebuild(&self, session_id: &str) -> AppResult<bool> {
        if session_id.is_empty() {
            return Ok(false);
        }
        let _lock = lock_file(&self.path(".rebuild.lock"), false)?;
        if self.current()?.revision > 0
            || read_json_optional::<RebuildRequest>(&self.path("rebuild.json"))?.is_some()
        {
            return Ok(false);
        }
        atomic_write_json(
            &self.path("rebuild.json"),
            &RebuildRequest {
                session_id: session_id.to_string(),
                requested_at_unix: unix_now(),
            },
        )?;
        Ok(true)
    }

    pub fn rebuild_request(&self) -> AppResult<Option<RebuildRequest>> {
        read_json_optional(&self.path("rebuild.json"))
    }

    #[cfg(test)]
    pub fn clear_rebuild_request(&self, completed: &RebuildRequest) -> AppResult<()> {
        let _lock = lock_file(&self.path(".rebuild.lock"), false)?;
        let Some(current) = read_json_optional::<RebuildRequest>(&self.path("rebuild.json"))?
        else {
            return Ok(());
        };
        if current != *completed {
            return Ok(());
        }
        match fs::remove_file(self.path("rebuild.json")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn events(&self) -> AppResult<Vec<TurnEvent>> {
        read_json_lines(&self.path("inbox.jsonl"))
    }

    pub fn current(&self) -> AppResult<CurrentState> {
        if let Some(value) =
            read_json_optional::<crate::daily::Selection>(&self.path("selected-current.json"))?
        {
            return Ok(value.current);
        }
        read_json_or_default(&self.path("current.json"))
    }

    pub fn cursor(&self) -> AppResult<u64> {
        let current = self.current()?;
        let value: serde_json::Value = read_json_or_default(&self.path("cursor.json"))?;
        let cursor = value
            .get("last_processed_seq")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        Ok(cursor.max(current.source_seq))
    }

    pub fn unprocessed_events(&self) -> AppResult<Vec<TurnEvent>> {
        let cursor = self.cursor()?;
        let selected: crate::daily::Selection =
            read_json_or_default(&self.path("selected-current.json"))?;
        Ok(self
            .events()?
            .into_iter()
            .filter(|event| event.seq > cursor && !selected.done.contains(&event.seq))
            .collect())
    }

    #[cfg(test)]
    pub fn processable_events(&self) -> AppResult<Vec<TurnEvent>> {
        let events = self.unprocessed_events()?;
        let terminal_index = events.iter().rposition(|event| {
            matches!(
                event.kind,
                EventKind::AssistantFinal | EventKind::Interrupted
            )
        });
        Ok(terminal_index.map_or_else(Vec::new, |index| events[..=index].to_vec()))
    }

    pub fn ui(&self) -> AppResult<UiState> {
        read_json_or_default(&self.path("ui.json"))
    }

    pub fn set_ui(&self, mut value: UiState) -> AppResult<()> {
        let _lock = lock_file(&self.path(".ui.lock"), false)?;
        value.updated_at_unix = Some(unix_now());
        atomic_write_json(&self.path("ui.json"), &value)
    }

    pub fn status(&self) -> AppResult<WorkerStatus> {
        read_json_or_default(&self.path("status.json"))
    }

    pub fn set_status(&self, state: &str, message: &str) -> AppResult<()> {
        atomic_write_json(
            &self.path("status.json"),
            &WorkerStatus {
                state: state.to_string(),
                message: message.to_string(),
                updated_at_unix: Some(unix_now()),
            },
        )
    }

    #[cfg(test)]
    pub fn commit_summary(
        &self,
        base_revision: u64,
        through_seq: u64,
        summary: Summary,
        repo: RepoSnapshot,
        timing: ContextTiming,
    ) -> AppResult<CurrentState> {
        let _lock = lock_file(&self.path(".commit.lock"), false)?;
        // Serialize inbox replacement with hooks appending a new event.
        let _events = lock_file(&self.path(".event.lock"), false)?;
        let current = self.current()?;
        if current.revision != base_revision {
            return Err(Box::new(StaleRevision {
                expected: base_revision,
                actual: current.revision,
            }));
        }
        if through_seq <= current.source_seq {
            return Err(format!("source sequence {through_seq} is already committed").into());
        }

        crate::legacy::preserve(self)?;
        // Merge before consuming raw input; retries are journal-idempotent.
        let included: Vec<_> = self
            .events()?
            .into_iter()
            .filter(|e| e.seq > current.source_seq && e.seq <= through_seq)
            .collect();
        crate::journal::apply(self, &included, &summary)?;
        let scope = crate::branch_context::context_for_events(self, &included)?;
        let next = CurrentState {
            revision: current.revision + 1,
            base_revision: Some(current.revision),
            source_seq: through_seq,
            updated_at_unix: Some(unix_now()),
            timing,
            repo,
            summary,
        };

        atomic_write_json(&self.path("current.json"), &next)?;
        atomic_write_json(&self.path("current-context.json"), &scope.key)?;
        crate::legacy::record_snapshot(self, &next)?;
        append_json_line(
            &self.path("timeline.jsonl"),
            &serde_json::json!({
                "revision": next.revision,
                "base_revision": next.base_revision,
                "source_seq": next.source_seq,
                "updated_at_unix": next.updated_at_unix,
                "repo_head": next.repo.head,
                "summary": next.summary,
                "timing": next.timing,
            }),
        )?;
        atomic_write_json(
            &self.path("cursor.json"),
            &serde_json::json!({ "last_processed_seq": through_seq }),
        )?;
        let remaining: Vec<TurnEvent> = self
            .events()?
            .into_iter()
            .filter(|event| event.seq > through_seq)
            .collect();
        atomic_write_json_lines(&self.path("inbox.jsonl"), &remaining)?;
        self.set_status("idle", "")?;
        Ok(next)
    }

    /// Commit exactly the selected events; a high sequence is not a delete range.
    pub fn commit_selected(
        &self,
        base_revision: u64,
        included: &[TurnEvent],
        summary: Summary,
        repo: RepoSnapshot,
        timing: ContextTiming,
    ) -> AppResult<CurrentState> {
        let _commit = lock_file(&self.path(".commit.lock"), false)?;
        let _events = lock_file(&self.path(".event.lock"), false)?;
        let current = self.current()?;
        if current.revision != base_revision {
            return Err(Box::new(StaleRevision {
                expected: base_revision,
                actual: current.revision,
            }));
        }
        let mut selection: crate::daily::Selection =
            read_json_or_default(&self.path("selected-current.json"))?;
        let queued = self.unprocessed_events()?;
        if included.is_empty() || included.iter().any(|e| !queued.contains(e)) {
            return Err("selected input is no longer pending".into());
        }
        crate::journal::apply(self, included, &summary)?;
        let last = included.iter().map(|e| e.seq).max().unwrap_or(0);
        selection.done.extend(included.iter().map(|e| e.seq));
        let remaining: Vec<_> = self
            .events()?
            .into_iter()
            .filter(|e| e.seq > current.source_seq && !selection.done.contains(&e.seq))
            .collect();
        let cursor = remaining
            .iter()
            .map(|e| e.seq)
            .min()
            .map_or(
                selection
                    .done
                    .last()
                    .copied()
                    .unwrap_or(last)
                    .max(current.source_seq),
                |seq| seq.saturating_sub(1),
            )
            .max(current.source_seq);
        let scope = crate::branch_context::context_for_events(self, included)?;
        let newest = last >= selection.display_seq;
        let mut next = CurrentState {
            revision: current.revision + 1,
            base_revision: Some(current.revision),
            source_seq: cursor,
            updated_at_unix: Some(unix_now()),
            ..current
        };
        if newest {
            next.summary = summary;
            next.repo = repo;
            next.timing = timing;
            selection.display_seq = last;
        }
        selection.current = next.clone();
        // This single authoritative file binds sparse receipts to the snapshot.
        atomic_write_json(&self.path("selected-current.json"), &selection)?;
        atomic_write_json(&self.path("current.json"), &next)?;
        if newest {
            atomic_write_json(&self.path("current-context.json"), &scope.key)?;
        }
        append_json_line(&self.path("timeline.jsonl"), &next)?;
        atomic_write_json_lines(&self.path("inbox.jsonl"), &remaining)?;
        atomic_write_json(
            &self.path("cursor.json"),
            &serde_json::json!({"last_processed_seq":cursor}),
        )?;
        self.set_status("idle", "")?;
        Ok(next)
    }
    #[cfg(test)]
    pub fn commit_rebuild(
        &self,
        base_revision: u64,
        summary: Summary,
        repo: RepoSnapshot,
        timing: ContextTiming,
    ) -> AppResult<CurrentState> {
        let _lock = lock_file(&self.path(".commit.lock"), false)?;
        crate::legacy::preserve(self)?;
        let current = self.current()?;
        if current.revision != base_revision {
            return Err(Box::new(StaleRevision {
                expected: base_revision,
                actual: current.revision,
            }));
        }
        let next = CurrentState {
            revision: current.revision + 1,
            base_revision: Some(current.revision),
            source_seq: current.source_seq,
            updated_at_unix: Some(unix_now()),
            timing,
            repo,
            summary,
        };
        atomic_write_json(&self.path("current.json"), &next)?;
        atomic_write_json(&self.path("current-context.json"), &"unlinked-history")?;
        crate::legacy::record_snapshot(self, &next)?;
        append_json_line(
            &self.path("timeline.jsonl"),
            &serde_json::json!({
                "revision": next.revision,
                "base_revision": next.base_revision,
                "source_seq": next.source_seq,
                "updated_at_unix": next.updated_at_unix,
                "repo_head": next.repo.head,
                "rebuild": true,
                "summary": next.summary,
                "timing": next.timing,
            }),
        )?;
        self.set_status("idle", "")?;
        Ok(next)
    }

    pub fn archive(&self) -> AppResult<()> {
        let Some(mut meta) = self.meta()? else {
            return Ok(());
        };
        meta.archived = true;
        meta.archived_at_unix = Some(unix_now());
        atomic_write_json(&self.path("meta.json"), &meta)
    }
}

#[must_use]
pub fn make_scope_key(session_name: &str, workspace_id: &str, tab_id: Option<&str>) -> String {
    tab_id.map_or_else(
        || make_space_key(session_name, workspace_id),
        |tab| {
            format!(
                "tab-{}-{}",
                make_space_key(session_name, workspace_id),
                crate::branch_context::hash(tab)
            )
        },
    )
}

#[must_use]
pub fn make_space_key(session_name: &str, workspace_id: &str) -> String {
    let raw = format!("{session_name}\0{workspace_id}");
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in raw.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let safe_workspace: String = workspace_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe_workspace}-{hash:016x}")
}

#[must_use]
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn lock_file(path: &Path, nonblocking: bool) -> AppResult<File> {
    ensure_parent(path)?;
    let file = secure_open(path, false)?;
    if nonblocking {
        file.try_lock()?;
    } else {
        file.lock()?;
    }
    Ok(file)
}

pub fn try_lock_file(path: &Path) -> AppResult<Option<File>> {
    use std::fs::TryLockError;

    ensure_parent(path)?;
    let file = secure_open(path, false)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let mut payload = serde_json::to_vec_pretty(value)?;
    payload.push(b'\n');
    atomic_write(path, &payload)
}

pub fn read_json_or_default<T>(path: &Path) -> AppResult<T>
where
    T: DeserializeOwned + Default,
{
    Ok(read_json_optional(path)?.unwrap_or_default())
}

pub fn read_json_optional<T: DeserializeOwned>(path: &Path) -> AppResult<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn append_json_line<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    ensure_parent(path)?;
    let mut file = secure_open_append(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(())
}

fn read_json_lines<T: DeserializeOwned>(path: &Path) -> AppResult<Vec<T>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut values = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        values.push(serde_json::from_str(&line).map_err(|error| {
            format!("invalid JSON in {}:{}: {error}", path.display(), index + 1)
        })?);
    }
    Ok(values)
}

pub fn atomic_write_json_lines<T: Serialize>(path: &Path, values: &[T]) -> AppResult<()> {
    let mut payload = Vec::new();
    for value in values {
        serde_json::to_writer(&mut payload, value)?;
        payload.push(b'\n');
    }
    atomic_write(path, &payload)
}

pub fn atomic_write(path: &Path, payload: &[u8]) -> AppResult<()> {
    ensure_parent(path)?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().map_or_else(
        || "state".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let temporary = path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let mut file = secure_create_new(&temporary)?;
    let write_result = (|| -> AppResult<()> {
        file.write_all(payload)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        if let Some(parent) = path.parent()
            && let Ok(directory) = File::open(parent)
        {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn ensure_parent(path: &Path) -> AppResult<()> {
    let parent = path.parent().ok_or("path has no parent")?;
    private_dir(parent.to_path_buf())?;
    Ok(())
}

#[cfg(unix)]
fn secure_open(path: &Path, truncate: bool) -> AppResult<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(truncate)
        .mode(FILE_MODE)
        .open(path)?)
}

#[cfg(unix)]
fn secure_open_append(path: &Path) -> AppResult<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .mode(FILE_MODE)
        .open(path)?)
}

#[cfg(unix)]
fn secure_create_new(path: &Path) -> AppResult<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(FILE_MODE)
        .open(path)?)
}

fn truncate_utf8(value: String, maximum_bytes: usize) -> (String, bool) {
    if value.len() <= maximum_bytes {
        return (value, false);
    }
    let mut boundary = maximum_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_string(), true)
}

fn canonical_or_original(path: &str) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Decision, DecisionStatus};
    use std::sync::Arc;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "maeklog-test-{name}-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn store(directory: &TestDirectory) -> SpaceStore {
        SpaceStore::for_workspace(&directory.0, "default", "w7", "/tmp", Some("test")).unwrap()
    }

    fn event(kind: EventKind, content: &str) -> NewTurnEvent {
        NewTurnEvent {
            kind,
            session_id: "session".into(),
            turn_id: Some("turn".into()),
            model: Some("gpt-5.6".into()),
            content: content.into(),
            cwd: "/tmp".into(),
        }
    }

    #[test]
    fn only_complete_turns_are_processable() {
        let directory = TestDirectory::new("complete");
        let store = store(&directory);
        store
            .append_event(event(EventKind::UserPrompt, "build it"))
            .unwrap();
        assert!(store.processable_events().unwrap().is_empty());
        store
            .append_event(event(EventKind::AssistantFinal, "done"))
            .unwrap();
        let events = store.processable_events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
    }

    #[test]
    fn committing_a_batch_does_not_erase_concurrent_new_events() {
        let directory = TestDirectory::new("batch-append");
        let store = store(&directory);
        store
            .append_event(event(EventKind::UserPrompt, "request"))
            .unwrap();
        store
            .append_event(event(EventKind::AssistantFinal, "answer"))
            .unwrap();
        let writer_store = store.clone();
        let writer = std::thread::spawn(move || {
            for index in 0..50 {
                let mut input = event(EventKind::UserPrompt, "new input");
                input.turn_id = Some(format!("concurrent-{index}"));
                writer_store.append_event(input).unwrap();
            }
        });
        store
            .commit_summary(
                0,
                2,
                Summary::default(),
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        writer.join().unwrap();
        let remaining = store.events().unwrap();
        assert_eq!(remaining.len(), 50);
        assert_eq!(remaining.first().unwrap().seq, 3);
        assert_eq!(remaining.last().unwrap().seq, 52);
    }

    #[test]
    fn commit_advances_revision_and_removes_raw_payload() {
        let directory = TestDirectory::new("commit");
        let store = store(&directory);
        store
            .append_event(event(EventKind::UserPrompt, "secret request"))
            .unwrap();
        store
            .append_event(event(EventKind::AssistantFinal, "safe result"))
            .unwrap();
        let summary = Summary {
            objective: "목표".into(),
            decisions: vec![Decision {
                id: "D-001".into(),
                text: "결정".into(),
                status: DecisionStatus::Active,
                supersedes: None,
                evidence: vec!["event:2".into()],
            }],
            ..Summary::default()
        };
        let current = store
            .commit_summary(
                0,
                2,
                summary,
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert_eq!(current.revision, 1);
        assert_eq!(store.cursor().unwrap(), 2);
        assert!(store.events().unwrap().is_empty());
        let timeline = fs::read_to_string(store.path("timeline.jsonl")).unwrap();
        assert!(timeline.contains("D-001"));
        assert!(!timeline.contains("secret request"));
    }

    #[test]
    fn rebuild_increments_revision_without_consuming_an_open_turn() {
        let directory = TestDirectory::new("rebuild");
        let store = store(&directory);
        store
            .append_event(event(EventKind::UserPrompt, "현재 진행 중 요청"))
            .unwrap();
        let rebuilt = store
            .commit_rebuild(
                0,
                Summary {
                    objective: "장기 목표".into(),
                    ..Summary::default()
                },
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert_eq!(rebuilt.revision, 1);
        assert_eq!(rebuilt.source_seq, 0);
        assert_eq!(store.events().unwrap().len(), 1);
        assert_eq!(store.cursor().unwrap(), 0);
        let timeline = fs::read_to_string(store.path("timeline.jsonl")).unwrap();
        assert!(timeline.contains("\"rebuild\":true"));
    }

    #[test]
    fn remembers_session_and_persists_rebuild_request() {
        let directory = TestDirectory::new("session");
        let store = store(&directory);
        store.remember_session("session-123").unwrap();
        assert_eq!(
            store.latest_session_id().unwrap().as_deref(),
            Some("session-123")
        );
        store.request_rebuild("session-123").unwrap();
        assert_eq!(
            store.rebuild_request().unwrap().unwrap().session_id,
            "session-123"
        );
        let request = store.rebuild_request().unwrap().unwrap();
        store.clear_rebuild_request(&request).unwrap();
        assert!(store.rebuild_request().unwrap().is_none());
    }

    #[test]
    fn initial_rebuild_is_scheduled_only_once_for_revision_zero() {
        let directory = TestDirectory::new("initial-rebuild");
        let store = store(&directory);
        assert!(store.schedule_initial_rebuild("session-1").unwrap());
        assert!(!store.schedule_initial_rebuild("session-1").unwrap());
        let request = store.rebuild_request().unwrap().unwrap();
        store.clear_rebuild_request(&request).unwrap();
        store
            .commit_rebuild(
                0,
                Summary::default(),
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert!(!store.schedule_initial_rebuild("session-1").unwrap());
    }

    #[test]
    fn stale_revisions_cannot_overwrite_current_state() {
        let directory = TestDirectory::new("stale");
        let store = store(&directory);
        store
            .append_event(event(EventKind::AssistantFinal, "first"))
            .unwrap();
        store
            .commit_summary(
                0,
                1,
                Summary::default(),
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        store
            .append_event(event(EventKind::AssistantFinal, "second"))
            .unwrap();
        let error = store
            .commit_summary(
                0,
                2,
                Summary::default(),
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("stale summary revision"));
        assert_eq!(store.current().unwrap().revision, 1);
    }

    #[test]
    fn timestamps_persist_atomically_and_legacy_revisions_remain_readable() {
        let legacy: CurrentState = serde_json::from_str(
            r#"{"revision":1,"base_revision":null,"source_seq":1,"updated_at_unix":900}"#,
        )
        .unwrap();
        assert_eq!(legacy.timing, ContextTiming::default());
        let directory = TestDirectory::new("timestamps");
        let store = store(&directory);
        let timing = ContextTiming {
            through_unix: Some(100),
            decisions: [("D-001".into(), 80)].into(),
            ..ContextTiming::default()
        };
        store
            .commit_rebuild(
                0,
                Summary::default(),
                RepoSnapshot::default(),
                timing.clone(),
            )
            .unwrap();
        assert_eq!(store.current().unwrap().timing, timing);
        assert!(
            fs::read_to_string(store.path("timeline.jsonl"))
                .unwrap()
                .contains("\"through_unix\":100")
        );
    }

    #[test]
    fn concurrent_events_receive_unique_sequences() {
        let directory = TestDirectory::new("concurrent");
        let store = Arc::new(store(&directory));
        let mut threads = Vec::new();
        for index in 0..12 {
            let store = Arc::clone(&store);
            threads.push(std::thread::spawn(move || {
                store
                    .append_event(event(EventKind::UserPrompt, &format!("event {index}")))
                    .unwrap()
                    .seq
            }));
        }
        let mut sequences: Vec<u64> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=12).collect::<Vec<_>>());
    }

    #[cfg(unix)]
    #[test]
    fn state_permissions_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new("permissions");
        let store = store(&directory);
        store
            .append_event(event(EventKind::UserPrompt, "private"))
            .unwrap();
        let directory_mode = fs::metadata(store.directory())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(store.path("inbox.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
