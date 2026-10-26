//! Public progress is a local projection, never input to the completed-turn queue.
use crate::model::{CodexHookInput, EventKind, TurnEvent};
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_optional, unix_now};
use crate::{AppResult, branch_context, config, participation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;

const MAX_WIRE_BYTES: u64 = 32 * 1024;
const MAX_REPORTS: usize = 24;
const MAX_ACTIVITIES: usize = 24;
const MAX_TEXT: usize = 1_200;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Observation {
    pub id: String,
    pub text: String,
    pub at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub id: String,
    pub label: String,
    pub status: String,
    pub at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub generation: u64,
    pub thread_id: String,
    pub turn_id: String,
    pub context_key: String,
    pub revision: u64,
    pub started_at: u64,
    pub updated_at: u64,
    pub ended: bool,
    pub status: String,
    pub request: String,
    pub commentary: Vec<Observation>,
    pub plan: Vec<PlanStep>,
    pub activities: Vec<Activity>,
    pub truncated: bool,
    #[serde(default)]
    pub source_prompt_seq: Option<u64>,
    #[serde(default)]
    pub source_final_seq: Option<u64>,
    #[serde(default)]
    pub client_pid: u32,
    #[serde(default)]
    pub last_sequence: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    sequence: u64,
    cwd: String,
    notification: Value,
}

fn new_snapshot(generation: u64, thread: &str, turn: &str, context: String, now: u64) -> Snapshot {
    Snapshot {
        generation,
        thread_id: thread.into(),
        turn_id: turn.into(),
        context_key: context,
        revision: 1,
        started_at: now,
        updated_at: now,
        ended: false,
        status: "inProgress".into(),
        request: String::new(),
        commentary: Vec::new(),
        plan: Vec::new(),
        activities: Vec::new(),
        truncated: false,
        source_prompt_seq: None,
        source_final_seq: None,
        client_pid: 0,
        last_sequence: 0,
    }
}

/// Caller holds the participation gate after the ordinary hook event was saved.
pub fn record_hook(store: &SpaceStore, event: &TurnEvent) -> AppResult<()> {
    let Some(turn) = event.turn_id.as_deref() else {
        return Ok(());
    };
    // append_event returns an unused candidate seq for a proven duplicate.
    // Only persisted capture receipts may alter the transient final boundary.
    if !branch_context::sources(store)?.contains_key(&event.seq) {
        return Ok(());
    }
    let _lock = lock_file(&store.path(".live-turn.lock"), false)?;
    let generation = participation::load(store)?.generation;
    let old: Option<Snapshot> = read_json_optional(&store.path("live-turn.json"))?;
    let same = old.as_ref().is_some_and(|s| {
        s.generation == generation && s.thread_id == event.session_id && s.turn_id == turn
    });
    if !same && !matches!(event.kind, EventKind::UserPrompt) {
        return Ok(());
    }
    let mut state = if same {
        old.clone().ok_or("missing matching live turn")?
    } else {
        let mut state = new_snapshot(
            generation,
            &event.session_id,
            turn,
            branch_context::source(store, event)?.context.key,
            event.recorded_at_unix,
        );
        // Bridge sequences remain monotonic across turns on the same client.
        if let Some(old) = &old {
            state.client_pid = old.client_pid;
            state.last_sequence = old.last_sequence;
        }
        state
    };
    match event.kind {
        EventKind::UserPrompt => {
            state.context_key = branch_context::source(store, event)?.context.key;
            state.source_prompt_seq = Some(event.seq);
            state.request = bounded(&event.content, 4_000, &mut state.truncated);
        }
        EventKind::AssistantFinal | EventKind::Interrupted => {
            state.ended = true;
            state.status = if matches!(event.kind, EventKind::Interrupted) {
                "interrupted"
            } else {
                "completed"
            }
            .into();
            state.source_final_seq = Some(event.seq);
        }
    }
    if old.as_ref() != Some(&state) {
        state.revision = state.revision.saturating_add(1);
        state.updated_at = event.recorded_at_unix;
        atomic_write_json(&store.path("live-turn.json"), &state)?;
    }
    Ok(())
}

pub fn load(store: &SpaceStore) -> AppResult<Option<Snapshot>> {
    let pref = participation::load(store)?;
    let Some(state): Option<Snapshot> = read_json_optional(&store.path("live-turn.json"))? else {
        return Ok(None);
    };
    let meta = store.meta()?.ok_or("missing Space metadata")?;
    if !pref.enabled
        || meta.archived
        || state.generation != pref.generation
        || state.context_key != branch_context::detect_for_store(store, &meta.cwd)?.key
        || (!state.ended && !thread_current(store, &state.thread_id, state.client_pid)?)
    {
        return Ok(None);
    }
    if let Some(seq) = state.source_final_seq {
        let selection: crate::daily::Selection =
            crate::storage::read_json_or_default(&store.path("selected-current.json"))?;
        if selection.done.contains(&seq) || store.current()?.source_seq >= seq {
            return Ok(None);
        }
    }
    Ok(Some(state))
}

pub fn thread_current(store: &SpaceStore, thread: &str, pid: u32) -> AppResult<bool> {
    if let Some(attached) = crate::client_routing::attached_thread(store, thread, pid)? {
        return Ok(attached);
    }
    let connection: crate::connection::Connection =
        crate::storage::read_json_or_default(&store.path("connection.json"))?;
    Ok(connection.session_id.is_empty() || connection.session_id == thread)
}

pub fn cli(arguments: &[String]) -> AppResult<()> {
    let [action, pid, thread] = arguments else {
        return Err("usage: maeklog live event CLIENT_PID THREAD_ID (JSON on stdin)".into());
    };
    if action != "event" {
        return Err("unknown live progress action".into());
    }
    let pid: u32 = pid.parse()?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_WIRE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WIRE_BYTES {
        return Err("live progress payload exceeds 32 KiB".into());
    }
    let envelope: Envelope = serde_json::from_slice(&bytes)?;
    let params = envelope
        .notification
        .get("params")
        .ok_or("missing notification params")?;
    if params.get("threadId").and_then(Value::as_str) != Some(thread.as_str()) {
        return Err("live notification belongs to another thread".into());
    }
    let turn = params
        .get("turnId")
        .or_else(|| params.pointer("/turn/id"))
        .and_then(Value::as_str)
        .ok_or("missing live turn id")?;
    let input = CodexHookInput {
        session_id: thread.clone(),
        turn_id: Some(turn.into()),
        cwd: envelope.cwd.clone(),
        hook_event_name: "LiveProgress".into(),
        ..CodexHookInput::default()
    };
    let root = config::state_root()?;
    let route = crate::client_routing::resolve(&root, &input)?
        .ok_or("live progress requires a registered terminal client")?;
    if route.pid != pid {
        return Err("live progress caller does not own this turn".into());
    }
    let Some(store) = progress_store(&root, &route, &envelope)? else {
        return Ok(());
    };
    // A completed relay command has already left its bounded temporary queue.
    // Drain on the host before the progress participation gate, never recursively.
    if let Err(error) = crate::task_report::drain(&store) {
        eprintln!("maeklog task relay: {error}");
    }
    let _gate = participation::lock(&store)?;
    let preference = participation::load(&store)?;
    if !preference.enabled
        || (preference.history_blocked
            && preference
                .open_turns
                .get(thread)
                .is_none_or(|id| id.as_deref() != Some(turn)))
    {
        return Ok(());
    }
    crate::client_routing::remember_turn(&root, &input, &route)?;
    apply(&store, thread, turn, pid, &envelope, unix_now())?;
    Ok(())
}

fn progress_store(
    root: &std::path::Path,
    route: &crate::client_routing::Route,
    envelope: &Envelope,
) -> AppResult<Option<SpaceStore>> {
    if let Some(store) = SpaceStore::find_for_scope(
        root,
        &route.session_name,
        &route.workspace_id,
        Some(&route.tab_id),
    ) {
        // Queued old events must not rewrite the current tab's cwd or reopen an archive.
        return Ok(store.meta()?.filter(|m| !m.archived).map(|_| store));
    }
    if envelope.notification.get("method").and_then(Value::as_str) != Some("turn/started") {
        return Ok(None);
    }
    let _client = lock_file(&root.join(".client-routing.lock"), false)?;
    let thread = envelope.notification["params"]["threadId"]
        .as_str()
        .ok_or("missing live thread id")?;
    if !crate::client_routing::route_attached(root, route, thread)? {
        return Ok(None);
    }
    SpaceStore::for_scope(
        root,
        &route.session_name,
        &route.workspace_id,
        Some(&route.tab_id),
        &envelope.cwd,
        None,
    )
    .map(Some)
}

#[allow(
    clippy::too_many_lines,
    reason = "A bounded notification transaction updates one local projection"
)]
fn apply(
    store: &SpaceStore,
    thread: &str,
    turn: &str,
    pid: u32,
    envelope: &Envelope,
    now: u64,
) -> AppResult<()> {
    let _lock = lock_file(&store.path(".live-turn.lock"), false)?;
    let old: Option<Snapshot> = read_json_optional(&store.path("live-turn.json"))?;
    let generation = participation::load(store)?.generation;
    let same = old
        .as_ref()
        .is_some_and(|s| s.generation == generation && s.thread_id == thread && s.turn_id == turn);
    let method = envelope
        .notification
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("");
    if old
        .as_ref()
        .is_some_and(|s| s.client_pid == pid && envelope.sequence <= s.last_sequence)
        || (!same && method != "turn/started")
    {
        return Ok(());
    }
    let mut state = if same {
        old.clone().ok_or("missing live state")?
    } else {
        // A new prompt may already have opened a later turn; never replace it
        // with a delayed start notification for a completed older request.
        if store
            .unprocessed_events()?
            .iter()
            .rev()
            .find(|e| matches!(e.kind, EventKind::UserPrompt))
            .is_some_and(|e| e.session_id != thread || e.turn_id.as_deref() != Some(turn))
        {
            return Ok(());
        }
        new_snapshot(
            generation,
            thread,
            turn,
            branch_context::detect_for_store(store, &envelope.cwd)?.key,
            now,
        )
    };
    state.client_pid = pid;
    state.last_sequence = envelope.sequence;
    let params = &envelope.notification["params"];
    if state.ended && method != "turn/completed" {
        return Ok(());
    }
    if method == "turn/started" {
        // Only an accepted current start may change the tab's location. The
        // participation and live locks serialize this with prompt capture.
        let _client = lock_file(&store.state_root.join(".client-routing.lock"), false)?;
        if !thread_current(store, thread, pid)? {
            return Ok(());
        }
        let meta = store.meta()?.ok_or("missing Space metadata")?;
        SpaceStore::for_scope(
            &store.state_root,
            &meta.session_name,
            &meta.workspace_id,
            meta.tab_id.as_deref(),
            &envelope.cwd,
            Some(&meta.label),
        )?;
    }
    match method {
        "turn/started" => {}
        "turn/completed" => {
            state.ended = true;
            let status = params
                .pointer("/turn/status")
                .and_then(Value::as_str)
                .unwrap_or("ended");
            state.status = match status {
                "completed" | "interrupted" | "failed" => status,
                _ => "ended",
            }
            .into();
        }
        "turn/plan/updated" => {
            let steps = params
                .get("plan")
                .and_then(Value::as_array)
                .ok_or("missing plan")?;
            if steps.len() > 16 {
                state.truncated = true;
            }
            state.plan = steps
                .iter()
                .take(16)
                .filter_map(|step| {
                    let status = step.get("status")?.as_str()?;
                    if !["pending", "inProgress", "completed"].contains(&status) {
                        return None;
                    }
                    Some(PlanStep {
                        step: bounded(step.get("step")?.as_str()?, 400, &mut state.truncated),
                        status: status.into(),
                    })
                })
                .collect();
            if let Some(text) = params
                .get("explanation")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                update_item(
                    &mut state,
                    &serde_json::json!({"type":"agentMessage","id":"pm-plan-explanation","phase":"commentary","text":text}),
                    true,
                    now,
                )?;
            }
        }
        "item/started" | "item/completed" => {
            let item = params.get("item").ok_or("missing live item")?;
            update_item(&mut state, item, method == "item/completed", now)?;
        }
        _ => return Ok(()),
    }
    // Sequence receipts are saved even for repeat notifications, but do not
    // invalidate the user's cached summary when its evidence did not change.
    let mut previous = old.clone();
    if let Some(old) = &mut previous {
        old.client_pid = pid;
        old.last_sequence = envelope.sequence;
    }
    if previous.as_ref() != Some(&state) {
        state.revision = state.revision.saturating_add(1);
        state.updated_at = now;
    }
    atomic_write_json(&store.path("live-turn.json"), &state)
}

fn update_item(state: &mut Snapshot, item: &Value, completed: bool, now: u64) -> AppResult<()> {
    let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or("missing live item id")?;
    if id.is_empty() || id.len() > 200 {
        return Err("invalid live item id".into());
    }
    if kind == "agentMessage" && completed {
        return update_report(state, item, id, now);
    }
    let label = match kind {
        // Older bridges may still send raw commands. Never persist their text:
        // shell arguments, environment assignments and executable paths can be secrets.
        "commandExecution" => crate::tr!("명령 실행", "Command execution").into(),
        "mcpToolCall" => format!(
            "{} · {}",
            item.get("server").and_then(Value::as_str).unwrap_or("MCP"),
            item.get("tool")
                .and_then(Value::as_str)
                .unwrap_or(crate::tr!("도구", "Tool"))
        ),
        "dynamicToolCall" | "collabToolCall" | "collabAgentToolCall" => item
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or(crate::tr!("작업", "Task"))
            .to_string(),
        "webSearch" => crate::tr!("자료 검색", "Web search").into(),
        "imageView" => crate::tr!("이미지 확인", "Image view").into(),
        "fileChange" => crate::tr!("파일 수정", "File change").into(),
        _ => return Ok(()),
    };
    let label = clean(&label, 300);
    let status = if completed {
        match item.get("status").and_then(Value::as_str) {
            Some("failed" | "declined" | "cancelled") => "failed",
            _ if item
                .get("exitCode")
                .and_then(Value::as_i64)
                .is_some_and(|c| c != 0) =>
            {
                "failed"
            }
            _ => "completed",
        }
    } else {
        "inProgress"
    };
    if let Some(index) = state.activities.iter().position(|a| a.id == id) {
        let old = &mut state.activities[index];
        // An out-of-order start can never reopen an already completed tool.
        if old.status != "inProgress" && !completed {
            return Ok(());
        }
        if old.status != status || old.label != label {
            old.status = status.into();
            old.label = label;
            old.at_unix = now;
            let updated = state.activities.remove(index);
            state.activities.push(updated);
        }
    } else {
        state.activities.push(Activity {
            id: id.into(),
            label,
            status: status.into(),
            at_unix: now,
        });
        if state.activities.len() > MAX_ACTIVITIES {
            state.activities.remove(0);
            state.truncated = true;
        }
    }
    Ok(())
}

fn update_report(state: &mut Snapshot, item: &Value, id: &str, now: u64) -> AppResult<()> {
    if item.get("phase").and_then(Value::as_str) == Some("final_answer") {
        return Ok(());
    }
    let text = bounded(
        item.get("text").and_then(Value::as_str).unwrap_or(""),
        MAX_TEXT,
        &mut state.truncated,
    );
    if text.is_empty() {
        return Ok(());
    }
    if let Some(old) = state.commentary.iter_mut().find(|r| r.id == id) {
        if old.text != text {
            old.text = text;
            old.at_unix = now;
            let index = state
                .commentary
                .iter()
                .position(|r| r.id == id)
                .ok_or("missing changed report")?;
            let report = state.commentary.remove(index);
            state.commentary.push(report);
        }
    } else {
        state.commentary.push(Observation {
            id: id.into(),
            text,
            at_unix: now,
        });
        if state.commentary.len() > MAX_REPORTS {
            // Retain the initial direction as well as the latest progress.
            state.commentary.remove(4);
            state.truncated = true;
        }
    }
    Ok(())
}

pub fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(limit)
        .collect()
}

fn bounded(text: &str, limit: usize, truncated: &mut bool) -> String {
    if text.chars().count() > limit {
        *truncated = true;
    }
    clean(text, limit)
}

#[cfg(test)]
#[path = "live_turn_tests.rs"]
mod tests;
