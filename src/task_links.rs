//! Conversation references, separate from immutable branch journals.
use crate::AppResult;
use crate::branch_context::{self, Context};
use crate::model::{EventKind, TurnEvent};
use crate::storage::{
    SpaceStore, atomic_write_json, lock_file, read_json_or_default, try_lock_file, unix_now,
};
use crate::work_links::{self, Kind, Link, RefreshOutcome};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

#[path = "task_links/intent.rs"]
mod intent;

const STATE_FILE: &str = "task-links.json";
const MAX_CANDIDATES: usize = 64;
const REFRESH_BUDGET: Duration = Duration::from_secs(16);
const MAX_REFRESH_REFERENCES: usize = 3;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    #[default]
    Checking,
    Verified,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    UserPrompt,
    AgentReport,
    Manual,
    AssistantMention,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Evidence {
    pub seq: u64,
    pub uid: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub generation: u64,
    pub quote: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskReference {
    pub id: String,
    pub repository: Option<String>,
    pub number: u64,
    pub kind: Option<Kind>,
    pub raw: String,
    pub url: Option<String>,
    pub link: Option<Link>,
    pub verification: VerificationState,
    pub error: Option<String>,
    pub provenance: Provenance,
    pub source: Evidence,
    #[serde(default)]
    pub created_reported: bool,
    #[serde(default)]
    related_reported: bool,
    #[serde(default)]
    next_check_at: u64,
    #[serde(default)]
    consecutive_failures: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskWorktree {
    pub path: String,
    pub context: Option<Context>,
    pub source_repository: Option<String>,
    pub verification: VerificationState,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TaskSnapshot {
    pub current: Vec<TaskReference>,
    pub related_pr: Vec<TaskReference>,
    pub worktree: Option<TaskWorktree>,
    pub explicit_none: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TaskLinks {
    /// Space-wide intent/source counter, including active-thread changes. Metadata does not bump it.
    pub revision: u64,
    pub generation: u64,
    pub thread_id: Option<String>,
    pub current: Vec<TaskReference>,
    pub related_pr: Vec<TaskReference>,
    pub worktree: Option<TaskWorktree>,
    pub current_request: String,
    pub captured_repository: Option<String>,
    pub candidates: Vec<TaskReference>,
    pub previous: Vec<TaskSnapshot>,
    pub explicit_none: bool,
    pub latest_source_seq: u64,
    pub latest_source_uid: Option<String>,
    pub manual_barrier_seq: u64,
    latest_prompt: Option<CapturedPrompt>,
    next_refresh_at: u64,
    consecutive_failures: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceRole {
    Current,
    Related,
    Reference,
    Previous,
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub reference: TaskReference,
    pub role: ChoiceRole,
}

impl TaskLinks {
    pub fn can_undo(&self) -> bool {
        !self.previous.is_empty()
    }

    pub fn choices(&self) -> Vec<Choice> {
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for (refs, role) in [
            (&self.current, ChoiceRole::Current),
            (&self.related_pr, ChoiceRole::Related),
            (&self.candidates, ChoiceRole::Reference),
        ] {
            for reference in refs {
                if seen.insert(reference.id.clone()) {
                    result.push(Choice {
                        reference: reference.clone(),
                        role,
                    });
                }
            }
        }
        for old in self.previous.iter().rev() {
            for reference in old.current.iter().chain(&old.related_pr) {
                if seen.insert(reference.id.clone()) {
                    result.push(Choice {
                        reference: reference.clone(),
                        role: ChoiceRole::Previous,
                    });
                }
            }
        }
        result
    }

    fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot {
            current: self.current.clone(),
            related_pr: self.related_pr.clone(),
            worktree: self.worktree.clone(),
            explicit_none: self.explicit_none,
        }
    }

    fn remember(&mut self) {
        if !self.current.is_empty() || !self.related_pr.is_empty() || self.explicit_none {
            self.previous.push(self.snapshot());
        }
    }

    fn replace(&mut self, refs: Vec<TaskReference>, manual: bool) {
        self.remember();
        self.current = refs;
        self.related_pr.clear();
        self.worktree = None;
        self.explicit_none = false;
        self.next_refresh_at = 0;
        self.consecutive_failures = 0;
        if manual {
            self.manual_barrier_seq = self.latest_source_seq;
        }
    }

    fn adopt_current(&mut self, mut refs: Vec<TaskReference>) {
        let old_ids: BTreeSet<_> = self.current.iter().map(|r| r.id.as_str()).collect();
        let new_ids: BTreeSet<_> = refs.iter().map(|r| r.id.as_str()).collect();
        if self.explicit_none || old_ids != new_ids {
            self.replace(refs, false);
            return;
        }
        for reference in &mut refs {
            if let Some(old) = self.current.iter().find(|r| r.id == reference.id)
                && (reference.kind.is_none() || old.kind.is_none() || reference.kind == old.kind)
            {
                reference.kind = reference.kind.or(old.kind);
                reference.link.clone_from(&old.link);
                reference.url.clone_from(&old.url);
                reference.verification = old.verification;
                reference.error.clone_from(&old.error);
                reference.next_check_at = old.next_check_at;
                reference.consecutive_failures = old.consecutive_failures;
            }
        }
        self.current = refs;
    }

    fn add_candidate(&mut self, reference: TaskReference) {
        if self
            .current
            .iter()
            .chain(&self.related_pr)
            .any(|r| r.id == reference.id)
        {
            return;
        }
        if let Some(old) = self.candidates.iter_mut().find(|r| r.id == reference.id) {
            let mut incoming = reference;
            // Keep successful cache facts while replacing the reported role/evidence. A mere
            // reference is never silently upgraded because an old candidate had metadata.
            if incoming.kind.is_none() || old.kind.is_none() || incoming.kind == old.kind {
                incoming.kind = incoming.kind.or(old.kind);
                incoming.link.clone_from(&old.link);
                incoming.url.clone_from(&old.url);
                incoming.verification = old.verification;
                incoming.error.clone_from(&old.error);
                incoming.next_check_at = old.next_check_at;
                incoming.consecutive_failures = old.consecutive_failures;
            }
            *old = incoming;
            self.next_refresh_at = 0;
        } else {
            if self.candidates.len() >= MAX_CANDIDATES {
                self.candidates.remove(0);
            }
            self.candidates.push(reference);
            self.next_refresh_at = 0;
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CapturedPrompt {
    evidence: Evidence,
    /// Latest captured prompt only; replaced on the next prompt. No transcript import.
    content: String,
    truncated: bool,
    context: Context,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct State {
    revision: u64,
    generation: u64,
    active_thread: Option<String>,
    threads: BTreeMap<String, TaskLinks>,
}

fn state(store: &SpaceStore) -> AppResult<State> {
    read_json_or_default(&store.path(STATE_FILE))
}
fn save(store: &SpaceStore, value: &State) -> AppResult<()> {
    atomic_write_json(&store.path(STATE_FILE), value)
}
fn bump(value: &mut State) -> AppResult<()> {
    value.revision = value.revision.checked_add(1).ok_or(crate::tr!(
        "작업 참조 revision overflow",
        "Work reference revision overflow"
    ))?;
    Ok(())
}

/// Cache only: no Git, Herdr, GitHub or model subprocesses.
pub fn load(store: &SpaceStore) -> AppResult<TaskLinks> {
    let value = state(store)?;
    let mut task = value
        .active_thread
        .as_ref()
        .and_then(|id| value.threads.get(id))
        .cloned()
        .unwrap_or_default();
    let preference = crate::participation::load(store)?;
    if store.meta()?.is_some_and(|meta| meta.archived) {
        task = TaskLinks::default();
    }
    task.generation = preference.generation;
    task.revision = value.revision;
    Ok(task)
}

pub fn active_thread(store: &SpaceStore) -> AppResult<Option<String>> {
    Ok(state(store)?.active_thread)
}

/// Only trusted client registration calls this; resuming a thread restores its own references.
pub fn activate(store: &SpaceStore, thread: &str) -> AppResult<()> {
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let preference = crate::participation::load(store)?;
    let mut value = state(store)?;
    if !preference.enabled {
        if let Some(previous) = value.active_thread.take() {
            if let Some(task) = value.threads.get_mut(&previous) {
                task.latest_prompt = None;
            }
            bump(&mut value)?;
            save(store, &value)?;
        }
        // Disabled attach can retire its old pointer, but must not recreate a
        // task state after explicit record deletion. Thread references survive.
        return Ok(());
    }
    let generation = preference.generation;
    if value.active_thread.as_deref() != Some(thread) {
        for (id, task) in &mut value.threads {
            if id != thread {
                task.latest_prompt = None;
            }
        }
        value.active_thread = Some(thread.into());
        value
            .threads
            .entry(thread.into())
            .or_insert_with(|| TaskLinks {
                thread_id: Some(thread.into()),
                generation,
                ..TaskLinks::default()
            });
        bump(&mut value)?;
        save(store, &value)?;
    }
    Ok(())
}

/// Opt-out changes invalidate old producers, while retaining user selections and explicit none.
pub fn generation_changed(store: &SpaceStore, generation: u64) -> AppResult<()> {
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let mut value = state(store)?;
    if value.generation != generation {
        value.generation = generation;
        for task in value.threads.values_mut() {
            task.generation = generation;
        }
        bump(&mut value)?;
        save(store, &value)?;
    }
    Ok(())
}

pub fn deactivate(store: &SpaceStore, thread: &str) -> AppResult<()> {
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let mut value = state(store)?;
    if value.active_thread.as_deref() == Some(thread) {
        if let Some(task) = value.threads.get_mut(thread) {
            task.latest_prompt = None;
        }
        value.active_thread = None;
        bump(&mut value)?;
        save(store, &value)?;
    }
    Ok(())
}

/// Called after durable source capture, before a reducer can consume the raw inbox event.
pub fn capture_event(store: &SpaceStore, event: &TurnEvent) -> AppResult<()> {
    if !matches!(
        event.kind,
        EventKind::UserPrompt | EventKind::AssistantFinal
    ) {
        return Ok(());
    }
    let source = branch_context::source(store, event)?;
    let generation = crate::participation::load(store)?.generation;
    let repository = matches!(event.kind, EventKind::UserPrompt)
        .then(|| work_links::github_repository(&source.context))
        .flatten();
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let mut value = state(store)?;
    if matches!(event.kind, EventKind::UserPrompt) {
        if store.state_root.join("client-routing.json").exists() {
            // With strict routing, only attach/switch can activate a thread. A queued prompt
            // after detach cannot revive its card, even if its frozen turn route survives.
            if value.active_thread.as_deref() != Some(event.session_id.as_str()) {
                return Ok(());
            }
        } else {
            let connection: crate::connection::Connection =
                read_json_or_default(&store.path("connection.json"))?;
            if !connection.session_id.is_empty() && connection.session_id != event.session_id {
                return Ok(());
            }
            for (thread, task) in &mut value.threads {
                if thread != &event.session_id {
                    task.latest_prompt = None;
                }
            }
            value.active_thread = Some(event.session_id.clone());
        }
    } else if value.active_thread.as_deref() != Some(event.session_id.as_str()) {
        return Ok(());
    }
    let task = value.threads.entry(event.session_id.clone()).or_default();
    if event.seq <= task.latest_source_seq {
        return Ok(());
    }
    let evidence = Evidence {
        seq: event.seq,
        uid: source.uid,
        thread_id: event.session_id.clone(),
        turn_id: event.turn_id.clone(),
        generation,
        quote: String::new(),
    };
    if matches!(event.kind, EventKind::UserPrompt) {
        task.generation = generation;
        task.thread_id = Some(event.session_id.clone());
        task.current_request = event.content.chars().take(4000).collect();
        task.latest_source_seq = event.seq;
        task.latest_source_uid = Some(evidence.uid.clone());
        task.captured_repository = repository;
        task.latest_prompt = Some(CapturedPrompt {
            evidence: evidence.clone(),
            content: event.content.clone(),
            truncated: event.truncated,
            context: source.context,
        });
    }
    let provenance = if matches!(event.kind, EventKind::UserPrompt) {
        Provenance::UserPrompt
    } else {
        Provenance::AssistantMention
    };
    let extracted = intent::extract(
        &event.content,
        task.captured_repository.as_deref(),
        &evidence,
        provenance,
        event.truncated,
    );
    let direct: Vec<_> = extracted
        .iter()
        .filter(|(_, current)| *current)
        .map(|(r, _)| r.clone())
        .collect();
    if !direct.is_empty() && event.seq > task.manual_barrier_seq {
        task.adopt_current(direct);
    } else if provenance == Provenance::UserPrompt
        && !event.truncated
        && intent::direct_clear(&event.content, &event.content)
    {
        if !task.explicit_none {
            task.remember();
        }
        task.current.clear();
        task.related_pr.clear();
        task.worktree = None;
        task.explicit_none = true;
    }
    for (reference, current) in extracted {
        if !current {
            task.add_candidate(reference);
        }
    }
    bump(&mut value)?;
    save(store, &value)
}

fn guarded_task(value: &mut State, expected: u64, generation: u64) -> AppResult<&mut TaskLinks> {
    if value.revision != expected {
        return Err(crate::tr!(
            "현재 대화 또는 작업이 변경되었습니다 · 선택 화면을 다시 여세요",
            "Conversation or work changed · Reopen the picker"
        )
        .into());
    }
    let thread = value.active_thread.as_ref().ok_or(crate::tr!(
        "현재 대화 연결 미확인",
        "Current conversation connection unknown"
    ))?;
    let task = value.threads.entry(thread.clone()).or_default();
    if task.generation != generation {
        return Err(crate::tr!(
            "수집 설정이 변경되었습니다 · 현재 요청을 다시 확인하세요",
            "Collection settings changed · Recheck the current request"
        )
        .into());
    }
    Ok(task)
}

fn mutate(
    store: &SpaceStore,
    expected: u64,
    update: impl FnOnce(&mut TaskLinks) -> AppResult<()>,
) -> AppResult<()> {
    let _gate = crate::participation::lock(store)?;
    mutate_locked(store, expected, update)
}

fn mutate_locked(
    store: &SpaceStore,
    expected: u64,
    update: impl FnOnce(&mut TaskLinks) -> AppResult<()>,
) -> AppResult<()> {
    crate::participation::ensure_enabled(store)?;
    if store.meta()?.is_some_and(|meta| meta.archived) {
        return Err(crate::tr!(
            "보관된 탭의 작업은 변경할 수 없습니다",
            "Archived tab work cannot be changed"
        )
        .into());
    }
    let generation = crate::participation::load(store)?.generation;
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let mut value = state(store)?;
    update(guarded_task(&mut value, expected, generation)?)?;
    bump(&mut value)?;
    save(store, &value)
}

pub fn select(store: &SpaceStore, expected: u64, id: &str) -> AppResult<()> {
    mutate(store, expected, |task| {
        let mut reference = task
            .choices()
            .into_iter()
            .find(|choice| choice.reference.id == id)
            .ok_or(crate::tr!(
                "작업 후보가 변경되었습니다",
                "Work candidates changed"
            ))?
            .reference;
        reference.provenance = Provenance::Manual;
        if task.current.len() == 1 && task.current[0].id == reference.id {
            task.current[0] = reference;
            task.manual_barrier_seq = task.latest_source_seq;
        } else {
            task.replace(vec![reference], true);
        }
        Ok(())
    })
}

pub fn set_manual(store: &SpaceStore, expected: u64, input: &str) -> AppResult<()> {
    mutate(store, expected, |task| {
        let evidence = task
            .latest_prompt
            .as_ref()
            .map_or_else(Evidence::default, |p| p.evidence.clone());
        let reference = intent::parse(
            input,
            task.captured_repository.as_deref(),
            &evidence,
            Provenance::Manual,
        )?;
        task.replace(vec![reference], true);
        Ok(())
    })
}

pub fn clear(store: &SpaceStore, expected: u64) -> AppResult<()> {
    mutate(store, expected, |task| {
        if !task.explicit_none {
            task.remember();
        }
        task.current.clear();
        task.related_pr.clear();
        task.worktree = None;
        task.explicit_none = true;
        task.manual_barrier_seq = task.latest_source_seq;
        Ok(())
    })
}

pub fn undo(store: &SpaceStore, expected: u64) -> AppResult<()> {
    mutate(store, expected, |task| {
        let previous = task.previous.pop().ok_or(crate::tr!(
            "되돌릴 작업 연결이 없습니다",
            "No work link to undo"
        ))?;
        for reference in task
            .current
            .clone()
            .into_iter()
            .chain(task.related_pr.clone())
        {
            task.add_candidate(reference);
        }
        task.current = previous.current;
        task.related_pr = previous.related_pr;
        task.worktree = previous.worktree;
        task.explicit_none = previous.explicit_none;
        task.manual_barrier_seq = task.latest_source_seq;
        task.next_refresh_at = 0;
        Ok(())
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportOperation {
    Set,
    Replace,
    Resume,
    Clear,
    Related,
    Reference,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub source_uid: String,
    pub expected_revision: u64,
    pub operation: ReportOperation,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub created: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportContext {
    pub source_uid: String,
    pub source_seq: u64,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub generation: u64,
    pub expected_revision: u64,
    pub repository: Option<String>,
    pub request: String,
    pub current: Vec<TaskReference>,
}

pub fn context_for_report(store: &SpaceStore) -> AppResult<Option<ReportContext>> {
    let task = load(store)?;
    Ok(context_from_task(&task))
}

/// Proves a duplicate hook against the exact current prompt, without recomputing source UIDs.
pub fn context_for_report_event(
    store: &SpaceStore,
    event: &TurnEvent,
) -> AppResult<Option<ReportContext>> {
    let task = load(store)?;
    let Some(prompt) = &task.latest_prompt else {
        return Ok(None);
    };
    if event.kind != EventKind::UserPrompt
        || event.session_id != prompt.evidence.thread_id
        || event.turn_id != prompt.evidence.turn_id
        || event.content != prompt.content
        || event.truncated != prompt.truncated
    {
        return Ok(None);
    }
    Ok(context_from_task(&task))
}

fn context_from_task(task: &TaskLinks) -> Option<ReportContext> {
    let Some(prompt) = &task.latest_prompt else {
        return None;
    };
    if prompt.evidence.generation != task.generation {
        return None;
    }
    Some(ReportContext {
        source_uid: prompt.evidence.uid.clone(),
        source_seq: prompt.evidence.seq,
        thread_id: prompt.evidence.thread_id.clone(),
        turn_id: prompt.evidence.turn_id.clone(),
        generation: task.generation,
        expected_revision: task.revision,
        repository: task.captured_repository.clone(),
        request: task.current_request.clone(),
        current: task.current.clone(),
    })
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportDisposition {
    Applied,
    Candidate,
    Unchanged,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportReceipt {
    pub revision: u64,
    pub disposition: ReportDisposition,
    pub current: Vec<String>,
}

/// The transport authenticates the primary terminal/turn. This layer separately orders intent.
#[cfg(test)]
pub fn accept_report(store: &SpaceStore, report: &Report) -> AppResult<ReportReceipt> {
    let _gate = crate::participation::lock(store)?;
    accept_report_locked(store, report)
}

/// Host transport holds the participation gate and trusted routing lease through this call.
#[allow(
    clippy::too_many_lines,
    reason = "One source-bound transaction applies report roles, intent changes and the manual source barrier together"
)]
pub fn accept_report_locked(store: &SpaceStore, report: &Report) -> AppResult<ReportReceipt> {
    if report.references.len() > 24 || report.quote.len() > 8000 {
        return Err(crate::tr!(
            "작업 보고 범위가 너무 큽니다",
            "Work report scope is too large"
        )
        .into());
    }
    let mut disposition = ReportDisposition::Candidate;
    mutate_locked(store, report.expected_revision, |task| {
        let prompt = task.latest_prompt.clone().ok_or(crate::tr!(
            "현재 사용자 요청 근거 없음",
            "No current user-request evidence"
        ))?;
        if prompt.evidence.uid != report.source_uid || prompt.evidence.generation != task.generation
        {
            return Err(crate::tr!(
                "이전 요청의 작업 보고는 반영할 수 없습니다",
                "Cannot apply a work report for an earlier request"
            )
            .into());
        }
        let fresh = prompt.evidence.seq > task.manual_barrier_seq;
        let references = report_references(task, &prompt, report, fresh)?;
        let direct = !prompt.truncated
            && !report.quote.is_empty()
            && prompt.content.contains(&report.quote)
            && intent::direct_report(
                &prompt.content,
                &report.quote,
                &references,
                task.captured_repository.as_deref(),
            );
        match report.operation {
            ReportOperation::Set | ReportOperation::Replace
                if fresh && direct && !references.is_empty() =>
            {
                if task
                    .current
                    .iter()
                    .map(|r| &r.id)
                    .eq(references.iter().map(|r| &r.id))
                {
                    disposition = ReportDisposition::Unchanged;
                } else {
                    task.replace(references.clone(), false);
                    disposition = ReportDisposition::Applied;
                }
            }
            ReportOperation::Clear
                if fresh
                    && !prompt.truncated
                    && intent::direct_clear(&prompt.content, &report.quote) =>
            {
                if task.explicit_none {
                    disposition = ReportDisposition::Unchanged;
                } else {
                    task.remember();
                    task.current.clear();
                    task.related_pr.clear();
                    task.worktree = None;
                    task.explicit_none = true;
                    disposition = ReportDisposition::Applied;
                }
            }
            ReportOperation::Resume
                if !task.explicit_none
                    && !references.is_empty()
                    && references
                        .iter()
                        .all(|r| task.current.iter().any(|current| current.id == r.id)) =>
            {
                disposition = ReportDisposition::Unchanged;
            }
            ReportOperation::Related => {
                // Persist as a candidate until fetched PR metadata proves its relation to a bound issue.
                for reference in &references {
                    task.add_candidate(reference.clone());
                }
            }
            _ => {
                for reference in &references {
                    task.add_candidate(reference.clone());
                }
            }
        }
        let location_allowed = fresh
            && !task.explicit_none
            && !task.current.is_empty()
            && (disposition != ReportDisposition::Candidate
                || report.operation == ReportOperation::Related);
        if location_allowed && let Some(path) = &report.worktree {
            task.worktree = Some(TaskWorktree {
                path: path.clone(),
                context: None,
                source_repository: None,
                verification: VerificationState::Checking,
                error: None,
            });
            task.next_refresh_at = 0;
        }
        if report.operation == ReportOperation::Related {
            attach_related(task);
        }
        Ok(())
    })?;
    let task = load(store)?;
    Ok(ReportReceipt {
        revision: task.revision,
        disposition,
        current: task.current.iter().map(|r| r.id.clone()).collect(),
    })
}

fn unique_references(references: Vec<TaskReference>) -> AppResult<Vec<TaskReference>> {
    let mut result: Vec<TaskReference> = Vec::new();
    let mut positions = BTreeMap::new();
    for reference in references {
        if let Some(index) = positions.get(&reference.id).copied() {
            let old: &mut TaskReference = &mut result[index];
            if old.kind.zip(reference.kind).is_some_and(|(a, b)| a != b) {
                return Err(crate::tr!(
                    "같은 번호에 상충하는 종류가 보고되었습니다",
                    "Conflicting reference types reported for the same number"
                )
                .into());
            }
            old.kind = old.kind.or(reference.kind);
            if old.url.is_none() {
                old.url = reference.url;
            }
        } else {
            positions.insert(reference.id.clone(), result.len());
            result.push(reference);
        }
    }
    Ok(result)
}

fn report_references(
    task: &TaskLinks,
    prompt: &CapturedPrompt,
    report: &Report,
    fresh: bool,
) -> AppResult<Vec<TaskReference>> {
    let refs = report
        .references
        .iter()
        .map(|raw| {
            let mut reference = intent::parse(
                raw,
                task.captured_repository.as_deref(),
                &prompt.evidence,
                Provenance::AgentReport,
            )?;
            reference.created_reported = report.created;
            reference.related_reported = report.operation == ReportOperation::Related
                && fresh
                && !task.explicit_none
                && !task.current.is_empty();
            reference.source.quote = report.quote.chars().take(600).collect();
            Ok(reference)
        })
        .collect::<AppResult<_>>()?;
    unique_references(refs)
}

/// Worker only: bounded detail fetches, a nonblocking lease, and no network under the edit lock.
pub fn refresh(store: &SpaceStore, force: bool) -> AppResult<RefreshOutcome> {
    refresh_with(store, force, work_links::lookup_reference)
}

#[allow(
    clippy::too_many_lines,
    reason = "One bounded fetch lease orders the snapshot, network batch, epoch validation and cache commit"
)]
fn refresh_with(
    store: &SpaceStore,
    force: bool,
    mut lookup: impl FnMut(&str, u64, Option<Kind>, Instant) -> AppResult<Link>,
) -> AppResult<RefreshOutcome> {
    if !crate::participation::load(store)?.enabled {
        return Ok(RefreshOutcome::Skipped);
    }
    let Some(_lease) = try_lock_file(&store.path(".task-links-fetch.lock"))? else {
        return Ok(RefreshOutcome::Busy);
    };
    let initial = load(store)?;
    if initial.thread_id.is_none() || !force && initial.next_refresh_at > unix_now() {
        return Ok(RefreshOutcome::Skipped);
    }
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for group in [&initial.current, &initial.related_pr, &initial.candidates] {
        let mut refs: Vec<_> = group
            .iter()
            .filter(|r| r.repository.is_some())
            .filter(|r| force || r.next_check_at <= unix_now())
            .filter(|r| seen.insert(r.id.clone()))
            .cloned()
            .collect();
        refs.sort_by_key(|r| {
            (
                r.link.is_some(),
                r.link
                    .as_ref()
                    .and_then(|l| l.checked_at_unix_utc)
                    .unwrap_or(0),
            )
        });
        targets.extend(refs);
    }
    let pending_worktree = initial
        .worktree
        .as_ref()
        .filter(|w| w.verification == VerificationState::Checking || force || !targets.is_empty());
    if targets.is_empty() && pending_worktree.is_none() {
        return Ok(RefreshOutcome::Skipped);
    }
    let deadline = Instant::now() + REFRESH_BUDGET;
    let updates: BTreeMap<_, _> = targets
        .iter()
        .take(MAX_REFRESH_REFERENCES)
        .map(|reference| {
            let result = lookup(
                reference.repository.as_deref().unwrap_or_default(),
                reference.number,
                reference.kind,
                deadline,
            )
            .map_err(|error| error.to_string());
            (reference.id.clone(), result)
        })
        .collect();
    let mut verification_context = initial.clone();
    for reference in &mut verification_context.current {
        if let Some(Ok(link)) = updates.get(&reference.id) {
            reference.link = Some(link.clone());
        }
    }
    let worktree =
        pending_worktree.map(|worktree| verify_worktree(&verification_context, worktree));
    let _gate = crate::participation::lock(store)?;
    let preference = crate::participation::load(store)?;
    if !preference.enabled
        || preference.generation != initial.generation
        || store.meta()?.is_some_and(|m| m.archived)
    {
        return Ok(RefreshOutcome::Skipped);
    }
    let _lock = lock_file(&store.path(".task-links.lock"), false)?;
    let mut value = state(store)?;
    if value.active_thread != initial.thread_id || value.revision != initial.revision {
        return Ok(RefreshOutcome::Skipped);
    }
    let task = value
        .threads
        .get_mut(initial.thread_id.as_deref().unwrap_or_default())
        .ok_or(crate::tr!("현재 작업 상태 없음", "No current work state"))?;
    for reference in task
        .current
        .iter_mut()
        .chain(&mut task.related_pr)
        .chain(&mut task.candidates)
    {
        if let Some(update) = updates.get(&reference.id) {
            apply_reference_update(reference, update, unix_now());
        }
    }
    if let Some(worktree) = worktree {
        task.worktree = Some(worktree);
    }
    attach_related(task);
    let failed = updates.values().any(Result::is_err);
    task.consecutive_failures = if failed {
        task.consecutive_failures.saturating_add(1)
    } else {
        0
    };
    task.next_refresh_at = task
        .current
        .iter()
        .chain(&task.related_pr)
        .chain(&task.candidates)
        .filter(|r| r.repository.is_some())
        .map(|r| r.next_check_at)
        .min()
        .unwrap_or_else(|| unix_now().saturating_add(60));
    save(store, &value)?;
    Ok(RefreshOutcome::Refreshed)
}

fn apply_reference_update(reference: &mut TaskReference, update: &Result<Link, String>, now: u64) {
    match update {
        Ok(link) => {
            reference.kind = Some(link.kind);
            reference.url = Some(link.url.clone());
            reference.link = Some(link.clone());
            reference.verification = VerificationState::Verified;
            reference.error = None;
            reference.consecutive_failures = 0;
            reference.next_check_at = now.saturating_add(60);
        }
        Err(error) => {
            reference.verification = VerificationState::Error;
            reference.error = Some(error.clone());
            reference.consecutive_failures = reference.consecutive_failures.saturating_add(1);
            reference.next_check_at = now.saturating_add(
                (30_u64 << reference.consecutive_failures.saturating_sub(1).min(4)).min(300),
            );
        }
    }
}

fn verify_worktree(task: &TaskLinks, worktree: &TaskWorktree) -> TaskWorktree {
    let mut result = worktree.clone();
    let context = branch_context::detect(&worktree.path, "task-worktree");
    let repository = work_links::github_repository(&context);
    let same_common = task.latest_prompt.as_ref().is_some_and(|prompt| {
        context.repository.is_some() && context.repository == prompt.context.repository
    });
    let matches_reference = task.current.iter().any(|reference| {
        reference.repository.as_ref().is_some_and(|r| {
            repository
                .as_ref()
                .is_some_and(|repo| r.eq_ignore_ascii_case(repo))
        }) || reference
            .link
            .as_ref()
            .and_then(|link| link.head_repository.as_ref())
            .is_some_and(|r| {
                repository
                    .as_ref()
                    .is_some_and(|repo| r.eq_ignore_ascii_case(repo))
            })
    });
    if context.unlinked_reason.is_none() && (same_common || matches_reference) {
        result.context = Some(context);
        result.source_repository = repository;
        result.verification = VerificationState::Verified;
        result.error = None;
    } else {
        result.verification = VerificationState::Error;
        result.error = Some(
            crate::tr!(
                "작업 위치와 현재 요청의 저장소 관계 미확인",
                "Work-location repository relationship to current request unknown"
            )
            .into(),
        );
    }
    result
}

fn attach_related(task: &mut TaskLinks) {
    if task.explicit_none || task.current.is_empty() {
        return;
    }
    let issues: BTreeSet<_> = task
        .current
        .iter()
        .filter_map(|r| r.link.as_ref())
        .filter(|l| l.kind == Kind::Issue)
        .map(|l| l.id().to_ascii_lowercase())
        .collect();
    let Some(worktree) = task
        .worktree
        .as_ref()
        .filter(|w| w.verification == VerificationState::Verified)
    else {
        return;
    };
    let Some(context) = &worktree.context else {
        return;
    };
    let Some(repo) = &worktree.source_repository else {
        return;
    };
    for reference in &task.candidates {
        if reference.provenance != Provenance::AgentReport
            || !reference.related_reported
            || reference.verification != VerificationState::Verified
            || reference.source.seq <= task.manual_barrier_seq
        {
            continue;
        }
        let Some(pr) = reference.link.as_ref().filter(|l| {
            l.kind == Kind::Pr
                && l.checked_at_unix_utc
                    .is_some_and(|at| unix_now().saturating_sub(at) <= 60)
        }) else {
            continue;
        };
        if pr
            .head_repository
            .as_ref()
            .is_some_and(|r| r.eq_ignore_ascii_case(repo))
            && pr.head_ref_name == context.branch
            && pr
                .closing_issues
                .iter()
                .any(|id| issues.contains(&id.to_ascii_lowercase()))
            && !task.related_pr.iter().any(|r| r.id == reference.id)
        {
            task.related_pr.push(reference.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::NewTurnEvent;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        store: SpaceStore,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "pm-task-links-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let store = SpaceStore::for_scope(
                &root.join("state"),
                "test",
                "workspace",
                Some("tab"),
                root.to_str().unwrap(),
                None,
            )
            .unwrap();
            Self { root, store }
        }
        fn prompt(&self, thread: &str, content: &str) -> TurnEvent {
            activate(&self.store, thread).unwrap();
            self.event(thread, EventKind::UserPrompt, content)
        }
        fn event(&self, thread: &str, kind: EventKind, content: &str) -> TurnEvent {
            self.store
                .append_event(NewTurnEvent {
                    kind,
                    session_id: thread.into(),
                    turn_id: Some(format!("turn-{}", self.store.events().unwrap().len())),
                    model: None,
                    content: content.into(),
                    cwd: self.root.to_string_lossy().into_owned(),
                })
                .unwrap()
        }
        fn task(&self) -> TaskLinks {
            load(&self.store).unwrap()
        }
        fn report(&self, operation: ReportOperation, reference: &str, quote: &str) -> Report {
            let context = context_for_report(&self.store).unwrap().unwrap();
            Report {
                source_uid: context.source_uid,
                expected_revision: context.expected_revision,
                operation,
                references: vec![reference.into()],
                quote: quote.into(),
                worktree: None,
                created: false,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn fetched(reference: &TaskReference) -> Link {
        let mut link = work_links::parse(
            reference.kind.unwrap_or(Kind::Issue),
            &format!(
                "{}#{}",
                reference.repository.as_deref().unwrap(),
                reference.number
            ),
        )
        .unwrap();
        link.title = Some(format!("title {}", reference.number));
        link.state = Some("OPEN".into());
        link.checked_at_unix_utc = Some(unix_now());
        link
    }

    #[test]
    fn new_explicit_target_is_immediate_and_failure_never_restores_previous_task() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        refresh_with(&f.store, false, |repo, n, _, _| {
            work_links::parse(Kind::Issue, &format!("{repo}#{n}"))
        })
        .unwrap();
        f.prompt("A", "owner/repo#142 말고 owner/repo#156 작업해줘");
        let before = f.task();
        assert_eq!(before.current[0].number, 156);
        assert_eq!(before.current[0].verification, VerificationState::Checking);
        assert_eq!(before.previous[0].current[0].number, 142);
        refresh_with(&f.store, false, |_, _, _, _| Err("offline".into())).unwrap();
        assert_eq!(f.task().current[0].number, 156);
        assert_eq!(f.task().current[0].verification, VerificationState::Error);
    }

    #[test]
    fn thread_switch_preserves_each_task_and_rejects_old_picker_revision() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let revision_a = f.task().revision;
        activate(&f.store, "B").unwrap();
        assert!(f.task().current.is_empty());
        assert_eq!(f.task().thread_id.as_deref(), Some("B"));
        f.prompt("B", "owner/repo#156 작업해줘");
        assert!(clear(&f.store, revision_a).is_err());
        assert_eq!(f.task().current[0].number, 156);
        activate(&f.store, "A").unwrap();
        assert_eq!(f.task().current[0].number, 142);
        assert!(
            context_for_report(&f.store).unwrap().is_none(),
            "resume restores refs without retaining inactive raw prompts"
        );
        deactivate(&f.store, "B").unwrap();
        assert_eq!(f.task().thread_id.as_deref(), Some("A"));
        deactivate(&f.store, "A").unwrap();
        assert!(f.task().current.is_empty());
    }

    #[test]
    fn disabled_attach_retires_pointer_and_clear_blocks_state_recreation() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        crate::participation::set_enabled(&f.store, false).unwrap();
        assert_eq!(f.task().current[0].number, 142);
        activate(&f.store, "B").unwrap();
        assert_eq!(active_thread(&f.store).unwrap(), None);
        assert!(context_for_report(&f.store).unwrap().is_none());
        let retained = state(&f.store).unwrap();
        assert_eq!(retained.threads["A"].current[0].number, 142);
        assert!(retained.threads["A"].latest_prompt.is_none());
        assert!(!retained.threads.contains_key("B"));
        crate::participation::set_enabled(&f.store, true).unwrap();
        activate(&f.store, "B").unwrap();
        assert_eq!(f.task().thread_id.as_deref(), Some("B"));
        assert!(f.task().current.is_empty());
        activate(&f.store, "A").unwrap();
        assert_eq!(f.task().current[0].number, 142);
        crate::maintenance::clear_records(&f.store).unwrap();
        assert!(!f.store.path(STATE_FILE).exists());
        activate(&f.store, "B").unwrap();
        assert!(!f.store.path(STATE_FILE).exists());
        assert_eq!(active_thread(&f.store).unwrap(), None);
    }

    #[test]
    fn strict_detach_late_prompt_cannot_reactivate_and_final_does_not_switch_user_intent() {
        let f = Fixture::new();
        atomic_write_json(
            &f.store.state_root.join("client-routing.json"),
            &serde_json::json!({"clients":[],"turns":{}}),
        )
        .unwrap();
        f.prompt("A", "owner/repo#142 작업해줘");
        let source_uid = f.task().latest_source_uid;
        f.event(
            "A",
            EventKind::AssistantFinal,
            "owner/repo#156 작업했습니다",
        );
        assert_eq!(f.task().current[0].number, 142);
        assert_eq!(f.task().latest_source_uid, source_uid);
        assert_eq!(
            f.task().candidates[0].provenance,
            Provenance::AssistantMention
        );
        deactivate(&f.store, "A").unwrap();
        f.event("A", EventKind::UserPrompt, "owner/repo#156 작업해줘");
        assert_eq!(active_thread(&f.store).unwrap(), None);
        activate(&f.store, "A").unwrap();
        assert_eq!(f.task().current[0].number, 142);
    }

    #[test]
    fn manual_barrier_blocks_same_old_source_even_after_reading_latest_revision() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        set_manual(&f.store, f.task().revision, "owner/repo#156").unwrap();
        let report = f.report(
            ReportOperation::Replace,
            "owner/repo#142",
            "owner/repo#142 작업해줘",
        );
        let receipt = accept_report(&f.store, &report).unwrap();
        assert_eq!(receipt.disposition, ReportDisposition::Candidate);
        assert_eq!(f.task().current[0].number, 156);
        let mut resume = f.report(ReportOperation::Resume, "owner/repo#156", "");
        resume.worktree = Some("/other-worktree".into());
        accept_report(&f.store, &resume).unwrap();
        assert!(f.task().worktree.is_none());
        f.prompt("A", "이제 owner/repo#188 작업해줘");
        assert_eq!(f.task().current[0].number, 188);
    }

    #[test]
    fn generation_change_keeps_manual_none_and_references_but_invalidates_producer() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let old = f.report(ReportOperation::Resume, "owner/repo#142", "");
        crate::participation::set_enabled(&f.store, false).unwrap();
        crate::participation::set_enabled(&f.store, true).unwrap();
        assert_eq!(f.task().current[0].number, 142);
        assert!(context_for_report(&f.store).unwrap().is_none());
        assert!(
            accept_report(
                &f.store,
                &Report {
                    expected_revision: f.task().revision,
                    ..old
                }
            )
            .is_err()
        );
        clear(&f.store, f.task().revision).unwrap();
        crate::participation::set_enabled(&f.store, false).unwrap();
        crate::participation::set_enabled(&f.store, true).unwrap();
        assert!(f.task().explicit_none);
        f.prompt("A", "계속 검토해줘");
        assert!(f.task().explicit_none);
    }

    #[test]
    fn consumed_prompt_retains_current_source_and_minimum_evidence_for_report() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        crate::storage::atomic_write_json_lines::<TurnEvent>(&f.store.path("inbox.jsonl"), &[])
            .unwrap();
        let context = context_for_report(&f.store).unwrap().unwrap();
        assert_eq!(context.request, "owner/repo#142 작업해줘");
        assert!(!context.source_uid.is_empty());
        let report = f.report(
            ReportOperation::Set,
            "owner/repo#142",
            "owner/repo#142 작업해줘",
        );
        assert_eq!(
            accept_report(&f.store, &report).unwrap().disposition,
            ReportDisposition::Unchanged
        );
    }

    #[test]
    fn duplicate_current_hook_can_reissue_context_without_changing_source_or_current_intent() {
        let f = Fixture::new();
        let first = f.prompt("A", "owner/repo#142 작업해줘");
        let duplicate = f
            .store
            .append_event(NewTurnEvent {
                kind: EventKind::UserPrompt,
                session_id: first.session_id.clone(),
                turn_id: first.turn_id.clone(),
                model: None,
                content: first.content.clone(),
                cwd: first.cwd.clone(),
            })
            .unwrap();
        assert_ne!(first.seq, duplicate.seq);
        let context = context_for_report_event(&f.store, &duplicate)
            .unwrap()
            .unwrap();
        assert_eq!(context.source_seq, first.seq);
        assert_eq!(f.task().current[0].number, 142);
        let unrelated = TurnEvent {
            content: "owner/repo#156 작업해줘".into(),
            ..duplicate
        };
        assert!(
            context_for_report_event(&f.store, &unrelated)
                .unwrap()
                .is_none()
        );
        deactivate(&f.store, "A").unwrap();
        assert!(
            context_for_report_event(&f.store, &first)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn no_origin_new_bare_number_is_not_resolved_against_an_old_full_reference() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        f.prompt("A", "#156 작업해줘");
        assert!(f.task().captured_repository.is_none());
        let current = f.task().current[0].clone();
        assert_eq!(current.number, 156);
        assert!(current.repository.is_none());
        assert_eq!(current.verification, VerificationState::Error);
        assert_eq!(f.task().previous[0].current[0].number, 142);
        assert_eq!(
            refresh_with(&f.store, false, |_, _, _, _| panic!(
                "unresolved repo must not query GitHub"
            ))
            .unwrap(),
            RefreshOutcome::Skipped
        );
    }

    #[test]
    fn background_response_cannot_overwrite_a_manual_choice_made_during_network() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let revision = f.task().revision;
        let outcome = refresh_with(&f.store, false, |repo, number, _, _| {
            set_manual(&f.store, revision, "owner/repo#156").unwrap();
            work_links::parse(Kind::Issue, &format!("{repo}#{number}"))
        })
        .unwrap();
        assert_eq!(outcome, RefreshOutcome::Skipped);
        assert_eq!(f.task().current[0].number, 156);
        assert_eq!(
            f.task().current[0].verification,
            VerificationState::Checking
        );
    }

    #[test]
    fn per_reference_ttl_serves_current_first_and_then_pending_candidates_without_starvation() {
        let f = Fixture::new();
        f.prompt(
            "A",
            "owner/repo#1와 owner/repo#2와 owner/repo#3 작업해줘\nowner/repo#4 참고해",
        );
        let mut calls = Vec::new();
        let revision = f.task().revision;
        refresh_with(&f.store, false, |repo, n, _, _| {
            calls.push(n);
            work_links::parse(Kind::Issue, &format!("{repo}#{n}"))
        })
        .unwrap();
        assert_eq!(calls, vec![1, 2, 3]);
        calls.clear();
        eprintln!(
            "TTL pending before retry: {:?}, next={}",
            f.task()
                .candidates
                .iter()
                .map(|r| (&r.id, r.next_check_at))
                .collect::<Vec<_>>(),
            f.task().next_refresh_at
        );
        let second_outcome = refresh_with(&f.store, false, |repo, n, _, _| {
            calls.push(n);
            work_links::parse(Kind::Issue, &format!("{repo}#{n}"))
        })
        .unwrap();
        eprintln!("TTL second outcome: {second_outcome:?}");
        assert_eq!(second_outcome, RefreshOutcome::Refreshed);
        assert_eq!(calls, vec![4]);
        assert_eq!(
            f.task().revision,
            revision,
            "metadata must not invalidate a picker"
        );
        let result = refresh_with(&f.store, false, |_, _, _, _| {
            panic!("fresh cache fetched again")
        })
        .unwrap();
        assert_eq!(result, RefreshOutcome::Skipped);
    }

    #[test]
    fn lookup_failure_retains_last_success_and_backoff_skips_futile_calls() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let expected = fetched(&f.task().current[0]);
        refresh_with(&f.store, false, |_, _, _, _| Ok(expected.clone())).unwrap();
        let force_outcome =
            refresh_with(&f.store, true, |_, _, _, _| Err("offline".into())).unwrap();
        assert_eq!(force_outcome, RefreshOutcome::Refreshed);
        let task = f.task();
        assert_eq!(task.current[0].link.as_ref().unwrap().title, expected.title);
        assert_eq!(task.current[0].verification, VerificationState::Error);
        assert_eq!(
            refresh_with(&f.store, false, |_, _, _, _| panic!("backoff ignored")).unwrap(),
            RefreshOutcome::Skipped
        );
    }

    #[test]
    fn related_report_starts_worktree_verification_and_requires_matching_issue_and_source_branch() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let mut report = f.report(ReportOperation::Related, "owner/repo#168", "");
        report.worktree = Some(f.root.to_string_lossy().into_owned());
        report.created = true;
        accept_report(&f.store, &report).unwrap();
        let mut task = f.task();
        assert_eq!(
            task.worktree.as_ref().unwrap().verification,
            VerificationState::Checking
        );
        task.current[0].link = Some(fetched(&task.current[0]));
        let mut pr = work_links::parse(Kind::Pr, "owner/repo#168").unwrap();
        pr.checked_at_unix_utc = Some(unix_now());
        pr.head_repository = Some("fork/repo".into());
        pr.head_ref_name = Some("feature".into());
        pr.closing_issues = vec!["Issue:owner/repo#142".into()];
        task.candidates[0].link = Some(pr.clone());
        task.candidates[0].verification = VerificationState::Verified;
        task.worktree = Some(TaskWorktree {
            path: "/worktree".into(),
            context: Some(Context {
                branch: Some("feature".into()),
                ..Context::default()
            }),
            source_repository: Some("fork/repo".into()),
            verification: VerificationState::Verified,
            error: None,
        });
        attach_related(&mut task);
        assert_eq!(task.related_pr.len(), 1);
        task.related_pr.clear();
        task.candidates[0].link.as_mut().unwrap().closing_issues =
            vec!["Issue:owner/repo#22".into()];
        attach_related(&mut task);
        assert!(task.related_pr.is_empty());
        task.candidates[0].link = Some(pr);
        task.worktree.as_mut().unwrap().source_repository = Some("other/repo".into());
        attach_related(&mut task);
        assert!(task.related_pr.is_empty());
        task.worktree.as_mut().unwrap().source_repository = Some("fork/repo".into());
        task.candidates[0].related_reported = false;
        attach_related(&mut task);
        assert!(
            task.related_pr.is_empty(),
            "operation=reference is never a related-PR intent"
        );
    }

    #[test]
    fn related_report_updates_candidate_role_and_evidence_without_losing_successful_metadata() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        f.event("A", EventKind::AssistantFinal, "owner/repo#168 참고");
        let mut value = state(&f.store).unwrap();
        let candidate = &mut value.threads.get_mut("A").unwrap().candidates[0];
        let mut pr = fetched(candidate);
        pr.kind = Kind::Pr;
        candidate.link = Some(pr.clone());
        candidate.kind = Some(Kind::Pr);
        candidate.verification = VerificationState::Verified;
        save(&f.store, &value).unwrap();
        let report = f.report(ReportOperation::Related, "owner/repo#168", "");
        accept_report(&f.store, &report).unwrap();
        let task = f.task();
        assert_eq!(task.candidates[0].provenance, Provenance::AgentReport);
        assert!(task.candidates[0].related_reported);
        assert_eq!(task.candidates[0].source.uid, report.source_uid);
        assert_eq!(task.candidates[0].link.as_ref().unwrap().title, pr.title);
        let report = f.report(ReportOperation::Reference, "owner/repo#168", "");
        accept_report(&f.store, &report).unwrap();
        assert!(!f.task().candidates[0].related_reported);
        assert_eq!(
            f.task().candidates[0].verification,
            VerificationState::Verified
        );
    }

    #[test]
    fn report_normalizes_url_and_qualified_duplicates_without_picking_or_reordering_targets() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142와 owner/repo#156 작업해줘");
        let mut report = f.report(
            ReportOperation::Set,
            "OWNER/REPO#142",
            "owner/repo#142와 owner/repo#156 작업해줘",
        );
        report.references.extend([
            "https://github.com/owner/repo/issues/142".into(),
            "owner/repo#156".into(),
            "https://github.com/owner/repo/issues/156".into(),
        ]);
        accept_report(&f.store, &report).unwrap();
        assert_eq!(
            f.task()
                .current
                .iter()
                .map(|r| r.number)
                .collect::<Vec<_>>(),
            vec![142, 156]
        );
        report.expected_revision = f.task().revision;
        report
            .references
            .push("https://github.com/owner/repo/pull/142".into());
        assert!(accept_report(&f.store, &report).is_err());
        assert_eq!(f.task().current.len(), 2);
    }

    #[test]
    fn explicit_user_clear_is_immediate_but_quoted_clear_never_clears_the_task() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        f.prompt("A", "\"이 작업의 연결 해제\"는 예시");
        assert_eq!(f.task().current[0].number, 142);
        f.prompt("A", "이 작업의 연결 해제");
        assert!(f.task().explicit_none);
        let mut report = f.report(ReportOperation::Clear, "", "이 작업의 연결 해제");
        report.references.clear();
        assert_eq!(
            accept_report(&f.store, &report).unwrap().disposition,
            ReportDisposition::Unchanged
        );
        undo(&f.store, f.task().revision).unwrap();
        assert_eq!(f.task().current[0].number, 142);
    }

    #[test]
    fn clear_undo_and_multiple_targets_preserve_reference_history() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142와 owner/repo#156 작업해줘");
        assert_eq!(f.task().current.len(), 2);
        clear(&f.store, f.task().revision).unwrap();
        assert!(f.task().explicit_none);
        assert!(f.task().can_undo());
        undo(&f.store, f.task().revision).unwrap();
        assert_eq!(f.task().current.len(), 2);
        assert!(!f.task().explicit_none);
        f.prompt("A", "main으로 복귀해서 병합 결과 검토해줘");
        assert_eq!(f.task().current.len(), 2);
    }

    #[test]
    fn selecting_current_single_reference_keeps_related_links_and_worktree() {
        let f = Fixture::new();
        f.prompt("A", "owner/repo#142 작업해줘");
        let mut value = state(&f.store).unwrap();
        let task = value.threads.get_mut("A").unwrap();
        task.related_pr.push(
            intent::parse(
                "pr owner/repo#168",
                None,
                &Evidence::default(),
                Provenance::AgentReport,
            )
            .unwrap(),
        );
        task.worktree = Some(TaskWorktree {
            path: "/selected-worktree".into(),
            context: None,
            source_repository: None,
            verification: VerificationState::Checking,
            error: None,
        });
        save(&f.store, &value).unwrap();
        select(&f.store, f.task().revision, "owner/repo#142").unwrap();
        assert_eq!(f.task().related_pr.len(), 1);
        assert!(f.task().worktree.is_some());
        assert_eq!(f.task().current[0].provenance, Provenance::Manual);
        f.prompt("A", "owner/repo#142 검토해줘");
        assert_eq!(
            f.task().related_pr.len(),
            1,
            "revisiting the same task after merge keeps its PR references"
        );
        assert!(f.task().worktree.is_some());
    }
}
