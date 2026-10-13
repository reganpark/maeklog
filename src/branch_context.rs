//! Capture-time context; never retarget a queued event using today's checkout.
use crate::AppResult;
use crate::model::{EventKind, TurnEvent};
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    pub key: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub incarnation: Option<String>,
    pub worktree: String,
    pub unlinked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Source {
    pub uid: String,
    pub order: u64,
    pub space: String,
    pub session: String,
    pub turn: Option<String>,
    pub kind: EventKind,
    pub at_unix_utc: Option<u64>,
    pub context: Context,
}

#[derive(Default, Deserialize, Serialize)]
struct Receipt {
    order: u64,
    payload: String,
}
#[derive(Default, Deserialize, Serialize)]
struct Receipts {
    next: u64,
    events: BTreeMap<String, Receipt>,
}

pub fn hash(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let result = crate::process::run_with_timeout(
        Command::new("git").current_dir(cwd).args(args),
        None,
        Duration::from_secs(2),
        32768,
    )
    .ok()?;
    result
        .success
        .then(|| result.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn detect(cwd: &str, unlinked_key: &str) -> Context {
    let mut context = Context {
        key: format!("unlinked-{}", hash(unlinked_key)),
        worktree: cwd.into(),
        unlinked_reason: Some(
            crate::tr!(
                "저장소·브랜치 귀속 미확인",
                "Repository/branch attribution unknown"
            )
            .into(),
        ),
        ..Context::default()
    };
    let Some(common) = git(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .and_then(|p| fs::canonicalize(p).ok()) else {
        return context;
    };
    context.repository = Some(common.to_string_lossy().into_owned());
    let Some(reference) = git(cwd, &["symbolic-ref", "-q", "HEAD"]) else {
        return context;
    };
    let Some(branch) = reference.strip_prefix("refs/heads/") else {
        return context;
    };
    context.branch = Some(branch.into());
    context.head = git(cwd, &["rev-parse", "HEAD"]);
    // A reflog's birth fingerprint distinguishes delete/recreate and avoids guessing
    // across missing/pruned logs. Metadata here is identity only, never evidence time.
    let log = common.join("logs").join(&reference);
    let Some(incarnation) = incarnation(&log) else {
        context.unlinked_reason = Some(
            crate::tr!(
                "브랜치 세대 미확인 · 자동 연결 안 함",
                "Branch generation unknown · Auto-link disabled"
            )
            .into(),
        );
        return context;
    };
    context.key = format!(
        "branch-{}",
        hash(&format!("{}\0{branch}\0{incarnation}", common.display()))
    );
    context.incarnation = Some(incarnation);
    context.unlinked_reason = None;
    context
}

/// Tab memory is isolated even when two tabs use the same repository/branch.
pub fn detect_for_store(store: &SpaceStore, cwd: &str) -> AppResult<Context> {
    let mut scope = detect(cwd, &store.key);
    if let Some(tab) = store.meta()?.and_then(|m| m.tab_id) {
        if scope.unlinked_reason.is_none() {
            scope.key = format!(
                "tab-branch-{}",
                hash(&format!("{}\0{}", store.key, scope.key))
            );
        }
        scope.tab_id = Some(tab);
    }
    Ok(scope)
}

fn incarnation(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let mut first = String::new();
    BufReader::new(file.take(8192)).read_line(&mut first).ok()?;
    if !first.contains('\t') {
        return None;
    }
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let inode = String::new();
    let birth = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    Some(hash(&format!("{first}\0{inode}\0{birth:?}")))
}

pub fn directory(root: &Path, context: &Context) -> PathBuf {
    root.join("branches").join(&context.key)
}
pub fn sources(store: &SpaceStore) -> AppResult<BTreeMap<u64, Source>> {
    read_json_or_default(&store.path("event-sources.json"))
}
pub fn source(store: &SpaceStore, event: &TurnEvent) -> AppResult<Source> {
    if let Some(source) = sources(store)?.remove(&event.seq) {
        return Ok(source);
    }
    Ok(Source {
        uid: format!("legacy-{}-{}", store.key, event.seq),
        order: event.seq,
        space: store.key.clone(),
        session: event.session_id.clone(),
        turn: event.turn_id.clone(),
        kind: event.kind.clone(),
        at_unix_utc: (event.recorded_at_unix > 0).then_some(event.recorded_at_unix),
        context: Context {
            key: format!("unlinked-{}", hash(&store.key)),
            worktree: event.cwd.clone(),
            unlinked_reason: Some(
                crate::tr!(
                    "旧 Space 기록 · 수집 당시 브랜치 미확인",
                    "Legacy Space records · Branch at collection unknown"
                )
                .into(),
            ),
            ..Default::default()
        },
    })
}

/// Caller owns .event.lock. Returns false only for a proven same-id duplicate.
pub fn capture(store: &SpaceStore, event: &TurnEvent) -> AppResult<bool> {
    let mut sources = sources(store)?;
    let mut context = detect_for_store(store, &event.cwd)?;
    if !matches!(event.kind, EventKind::UserPrompt)
        && let Some(prompt) = sources.values().rev().find(|s| {
            s.session == event.session_id
                && s.kind == EventKind::UserPrompt
                && (event.turn_id.is_none() || s.turn == event.turn_id)
        })
    {
        context = prompt.context.clone();
    }
    let generation = crate::participation::load(store)?.generation;
    let base_uid = event.turn_id.as_ref().map_or_else(
        || format!("{}-{generation}-{}", store.key, event.seq),
        |turn| hash(&format!("{}\0{turn}\0{:?}", event.session_id, event.kind)),
    );
    let payload = hash(&event.content);
    // Some hook producers reuse a turn id for distinct final messages. Keep both
    // payloads, while identical deliveries still have a stable cross-Space id.
    let uid = format!("{base_uid}-{payload}");
    // Dedup is root-wide: a late delivery after checkout must not create a second
    // copy under a different branch. Values are scope keys, never raw dialogue.
    let _origins = lock_file(&store.state_root.join(".event-origins.lock"), false)?;
    let origins_path = store.state_root.join("event-origins.json");
    let mut origins: BTreeMap<String, String> = read_json_or_default(&origins_path)?;
    if origins.contains_key(&uid) {
        return Ok(false);
    }
    let dir = directory(&store.state_root, &context);
    let _lock = lock_file(&dir.join(".capture.lock"), false)?;
    let mut receipts: Receipts = read_json_or_default(&dir.join("receipts.json"))?;
    if let Some(old) = receipts.events.get(&uid) {
        if old.payload == payload {
            return Ok(false);
        }
        return Err("source fingerprint collision; original retained".into());
    }
    receipts.next = receipts.next.max(1);
    let order = receipts.next;
    receipts.next += 1;
    receipts
        .events
        .insert(uid.clone(), Receipt { order, payload });
    let source = Source {
        uid,
        order,
        space: store.key.clone(),
        session: event.session_id.clone(),
        turn: event.turn_id.clone(),
        kind: event.kind.clone(),
        at_unix_utc: (event.recorded_at_unix > 0).then_some(event.recorded_at_unix),
        context: context.clone(),
    };
    sources.insert(event.seq, source);
    atomic_write_json(&dir.join("context.json"), &context)?;
    atomic_write_json(&store.path("event-sources.json"), &sources)?;
    crate::storage::append_json_line(&store.path("inbox.jsonl"), event)?;
    origins.insert(source_uid(&sources, event.seq)?, context.key);
    atomic_write_json(&origins_path, &origins)?;
    atomic_write_json(&dir.join("receipts.json"), &receipts)?;
    Ok(true)
}

fn source_uid(sources: &BTreeMap<u64, Source>, seq: u64) -> AppResult<String> {
    sources
        .get(&seq)
        .map(|s| s.uid.clone())
        .ok_or_else(|| "missing capture source".into())
}

pub fn context_for_events(store: &SpaceStore, events: &[TurnEvent]) -> AppResult<Context> {
    if let Some(first) = events.first() {
        return Ok(source(store, first)?.context);
    }
    Ok(Context {
        key: format!("unlinked-{}", hash(&store.key)),
        unlinked_reason: Some(
            crate::tr!(
                "이력 발췌의 브랜치 미확인",
                "Branch of history excerpt unknown"
            )
            .into(),
        ),
        ..Default::default()
    })
}

pub fn model_repo(
    store: &SpaceStore,
    events: &[TurnEvent],
) -> AppResult<crate::model::RepoSnapshot> {
    let scope = context_for_events(store, events)?;
    Ok(crate::model::RepoSnapshot {
        available: scope.repository.is_some(),
        root: Some(scope.worktree),
        branch: scope.branch,
        head: scope.head,
        ..Default::default()
    })
}

/// Space cursor stays monotonic, but a previous branch's prose is not model input.
#[cfg(test)]
pub fn model_base(
    store: &SpaceStore,
    events: &[TurnEvent],
) -> AppResult<crate::model::CurrentState> {
    let current = store.current()?;
    let saved: Option<String> =
        crate::storage::read_json_optional(&store.path("current-context.json"))?;
    let scope = context_for_events(store, events)?;
    if saved.as_deref() == Some(&scope.key) || saved.is_none() && scope.unlinked_reason.is_some() {
        Ok(current)
    } else {
        Ok(crate::model::CurrentState {
            revision: current.revision,
            source_seq: current.source_seq,
            ..Default::default()
        })
    }
}

pub fn try_reduce_lock(store: &SpaceStore, events: &[TurnEvent]) -> AppResult<Option<File>> {
    let scope = context_for_events(store, events)?;
    crate::storage::try_lock_file(&directory(&store.state_root, &scope).join(".reduce.lock"))
}

pub fn same_context_batch(
    store: &SpaceStore,
    events: Vec<TurnEvent>,
    newest: bool,
) -> AppResult<Vec<TurnEvent>> {
    let Some(anchor) = (if newest {
        events.last()
    } else {
        events.first()
    }) else {
        return Ok(events);
    };
    let key = source(store, anchor)?.context.key;
    let mut selected = Vec::new();
    let iter: Box<dyn Iterator<Item = TurnEvent>> = if newest {
        Box::new(events.into_iter().rev())
    } else {
        Box::new(events.into_iter())
    };
    for event in iter {
        if source(store, &event)?.context.key != key {
            break;
        }
        selected.push(event);
    }
    if newest {
        selected.reverse();
    }
    Ok(selected)
}
