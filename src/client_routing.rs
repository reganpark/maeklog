//! Client-origin registrations, independent of the shared Codex daemon environment.
use crate::AppResult;
use crate::config;
use crate::herdr::PaneInfo;
use crate::model::CodexHookInput;
use crate::storage::{atomic_write_json, lock_file, read_json_optional};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const REGISTRY: &str = "client-routing.json";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Route {
    pub session_name: String,
    pub socket: PathBuf,
    pub herdr_bin: PathBuf,
    pub terminal_id: String,
    pub pid: u32,
    pub workspace_id: String,
    pub tab_id: String,
}

pub fn caller_context(route: Option<&Route>) -> AppResult<crate::herdr::CallerContext> {
    route.map_or_else(
        || crate::herdr::caller_context(false),
        |route| {
            Ok(crate::herdr::CallerContext {
                workspace_id: route.workspace_id.clone(),
                tab_id: Some(route.tab_id.clone()),
            })
        },
    )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Client {
    thread: String,
    route: Route,
}

#[derive(Default, Deserialize, Serialize)]
struct Registry {
    clients: Vec<Client>,
    turns: BTreeMap<String, Route>,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn turn_key(input: &CodexHookInput) -> Option<String> {
    input
        .turn_id
        .as_deref()
        .filter(|id| valid_id(id))
        .map(|turn| format!("{}:{turn}", input.session_id))
}

fn call(route: &Route, arguments: &[&str]) -> AppResult<serde_json::Value> {
    crate::herdr::call_at(
        arguments,
        &route.socket,
        &route.session_name,
        &route.herdr_bin,
    )
}

fn live_pane(route: &Route) -> AppResult<PaneInfo> {
    let value = call(route, &["pane", "list"])?;
    let panes: Vec<PaneInfo> = serde_json::from_value(
        value
            .pointer("/result/panes")
            .ok_or("missing pane list")?
            .clone(),
    )?;
    let pane = panes
        .into_iter()
        // Screen-based agent detection may lag behind the first TUI callback.
        // The exact foreground PID and process name below establish ownership.
        .find(|pane| pane.terminal_id.as_deref() == Some(route.terminal_id.as_str()))
        .ok_or("registered Codex terminal is no longer live")?;
    let processes = call(route, &["pane", "process-info", "--pane", &pane.pane_id])?;
    let owns_pane = processes
        .pointer("/result/process_info/foreground_processes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("pid").and_then(serde_json::Value::as_u64) == Some(u64::from(route.pid))
                    && item.get("name").and_then(serde_json::Value::as_str) == Some("codex")
            })
        });
    if !owns_pane {
        return Err("registered Codex client no longer owns the terminal".into());
    }
    Ok(pane)
}

/// A report nonce carries the frozen route. Current terminal ownership and
/// primary attachment must still match; inherited daemon context is irrelevant.
pub(crate) fn validate_primary_route(root: &Path, route: &Route, thread: &str) -> AppResult<()> {
    let pane = live_pane(route)?;
    if pane.workspace_id != route.workspace_id
        || pane.tab_id.as_deref() != Some(&route.tab_id)
        || !pane
            .agent_session
            .as_ref()
            .is_some_and(|session| session.kind == "id" && session.value == thread)
        || !route_attached(root, route, thread)?
    {
        return Err(crate::tr!(
            "작업 보고의 주 에이전트 연결이 바뀌었습니다",
            "Primary agent connection changed for this work report"
        )
        .into());
    }
    Ok(())
}

fn task_scope(root: &Path, route: &Route) -> Option<crate::storage::SpaceStore> {
    crate::storage::SpaceStore::find_for_scope(
        root,
        &route.session_name,
        &route.workspace_id,
        Some(&route.tab_id),
    )
}

fn activate_task(root: &Path, route: &Route, pane: &PaneInfo, thread: &str) -> AppResult<()> {
    let store = match task_scope(root, route) {
        Some(store) => store,
        None => crate::storage::SpaceStore::for_scope(
            root,
            &route.session_name,
            &route.workspace_id,
            Some(&route.tab_id),
            &crate::herdr::pane_cwd(pane),
            None,
        )?,
    };
    crate::task_links::activate(&store, thread)
}

/// Host background/explicit picker initialization for an already running client
/// after a helper upgrade. Hooks and sandbox tools never restore attachments.
pub fn activate_attached_task(store: &crate::storage::SpaceStore) -> AppResult<bool> {
    if crate::task_links::active_thread(store)?.is_some() {
        return Ok(false);
    }
    let Some(meta) = store.meta()?.filter(|meta| !meta.archived) else {
        return Ok(false);
    };
    if !store.state_root.join(REGISTRY).exists() {
        return Ok(false);
    }
    // This is the same lease used by native attach/detach. No participation gate
    // is acquired here: hook capture already orders gate -> registry lease.
    let _client = lock_file(&store.state_root.join(".client-routing.lock"), false)?;
    if crate::task_links::active_thread(store)?.is_some() {
        return Ok(false);
    }
    let Some(registry): Option<Registry> = read_json_optional(&store.state_root.join(REGISTRY))?
    else {
        return Ok(false);
    };
    let mut candidates = registry.clients.iter().filter(|client| {
        client.route.session_name == meta.session_name
            && client.route.workspace_id == meta.workspace_id
            && meta.tab_id.as_deref() == Some(client.route.tab_id.as_str())
    });
    let Some(client) = candidates.next() else {
        return Ok(false);
    };
    if candidates.next().is_some()
        || validate_primary_route(&store.state_root, &client.route, &client.thread).is_err()
    {
        return Ok(false);
    }
    crate::task_links::activate(store, &client.thread)?;
    Ok(true)
}

/// Called only by the terminal client, never by a daemon hook. Arguments carry
/// IDs, not conversation text. Herdr verifies the foreground process each time.
#[allow(
    clippy::too_many_lines,
    reason = "One locked attach/detach transaction verifies the client, updates Herdr and commits provenance"
)]
pub fn cli(arguments: &[String]) -> AppResult<()> {
    let [action, pid, thread] = arguments else {
        return Err("usage: maeklog client <attach|detach> CLIENT_PID THREAD_ID".into());
    };
    if !matches!(action.as_str(), "attach" | "detach") || !valid_id(thread) {
        return Err("invalid client action or thread id".into());
    }
    if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
        return Err("client registration requires a Herdr terminal".into());
    }
    let pid: u32 = pid.parse()?;
    let socket =
        PathBuf::from(std::env::var_os("HERDR_SOCKET_PATH").ok_or("missing Herdr socket")?);
    let caller = std::env::var("HERDR_PANE_ID")?;
    let session_name = config::herdr_session_name();
    let herdr_bin = config::herdr_bin();
    let value = crate::herdr::call_at(
        &["pane", "current", "--pane", &caller],
        &socket,
        &session_name,
        &herdr_bin,
    )?;
    let pane: PaneInfo = serde_json::from_value(
        value
            .pointer("/result/pane")
            .ok_or("missing caller pane")?
            .clone(),
    )?;
    let route = Route {
        session_name,
        socket,
        herdr_bin,
        terminal_id: pane
            .terminal_id
            .clone()
            .ok_or("missing terminal identity")?,
        pid,
        workspace_id: pane.workspace_id.clone(),
        tab_id: pane.tab_id.clone().ok_or("missing tab identity")?,
    };
    live_pane(&route)?;
    let root = config::state_root()?;
    let _lock = lock_file(&root.join(".client-routing.lock"), false)?;
    let mut registry: Registry = read_json_optional(&root.join(REGISTRY))?.unwrap_or_default();
    let same_client = |client: &Client| {
        client.route.socket == route.socket
            && client.route.terminal_id == route.terminal_id
            && client.route.pid == pid
    };
    if action == "detach" {
        // A delayed detach for A must not clear a newer attachment to B.
        if !registry
            .clients
            .iter()
            .any(|c| same_client(c) && c.thread == *thread)
        {
            return Ok(());
        }
        call(
            &route,
            &[
                "pane",
                "release-agent",
                &pane.pane_id,
                "--source",
                "herdr:codex",
                "--agent",
                "codex",
            ],
        )?;
        registry.clients.retain(|c| !same_client(c));
        if let Some(store) = task_scope(&root, &route) {
            crate::task_links::deactivate(&store, thread)?;
        }
    } else {
        call(
            &route,
            &[
                "pane",
                "report-agent-session",
                &pane.pane_id,
                "--source",
                "herdr:codex",
                "--agent",
                "codex",
                "--agent-session-id",
                thread,
                "--session-start-source",
                "resume",
            ],
        )?;
        let reported = call(&route, &["pane", "get", &pane.pane_id])?;
        if reported
            .pointer("/result/pane/agent_session/value")
            .and_then(serde_json::Value::as_str)
            != Some(thread)
        {
            return Err("Herdr did not accept the client session registration".into());
        }
        // Replace this terminal's old process/session, leaving other viewers distinct.
        for previous in registry.clients.iter().filter(|c| {
            c.route.socket == route.socket
                && c.route.terminal_id == route.terminal_id
                && (c.thread != *thread || c.route != route)
        }) {
            if let Some(store) = task_scope(&root, &previous.route) {
                crate::task_links::deactivate(&store, &previous.thread)?;
            }
        }
        registry
            .clients
            .retain(|c| c.route.socket != route.socket || c.route.terminal_id != route.terminal_id);
        registry.clients.push(Client {
            thread: thread.clone(),
            route: route.clone(),
        });
        activate_task(&root, &route, &pane, thread)?;
    }
    atomic_write_json(&root.join(REGISTRY), &registry)?;
    println!("{}", json!({"client_routing":true,"action":action}));
    Ok(())
}

/// None means the installation has not opted into client routing yet. Once the
/// registry exists, unresolved/ambiguous input MUST NOT fall back to daemon env.
pub fn resolve(root: &Path, input: &CodexHookInput) -> AppResult<Option<Route>> {
    let Some(registry): Option<Registry> = read_json_optional(&root.join(REGISTRY))? else {
        return Ok(None);
    };
    if !valid_id(&input.session_id) {
        return Err("invalid hook session id".into());
    }
    if let Some(route) = turn_key(input).and_then(|key| registry.turns.get(&key)) {
        live_pane(route)?;
        return Ok(Some(route.clone()));
    }
    if matches!(input.hook_event_name.as_str(), "Stop" | "Interrupt") {
        return Err("no registered origin for this completed turn; no event collected".into());
    }
    let mut candidates = Vec::new();
    for client in registry
        .clients
        .iter()
        .filter(|c| c.thread == input.session_id)
    {
        if let Ok(pane) = live_pane(&client.route) {
            if !pane
                .agent_session
                .as_ref()
                .is_some_and(|session| session.kind == "id" && session.value == client.thread)
            {
                continue;
            }
            let Some(tab_id) = pane.tab_id else {
                continue;
            };
            candidates.push(Route {
                workspace_id: pane.workspace_id,
                tab_id,
                ..client.route.clone()
            });
        }
    }
    match candidates.len() {
        1 => Ok(candidates.pop()),
        0 => Err("Codex client connection not registered; no event collected (daemon environment ignored)".into()),
        _ => Err("Codex conversation is open in multiple terminals; no event collected without an unambiguous origin".into()),
    }
}

pub fn remember_turn(root: &Path, input: &CodexHookInput, route: &Route) -> AppResult<()> {
    let Some(key) = turn_key(input) else {
        return Err("client-routed turn requires a turn id".into());
    };
    let _lock = lock_file(&root.join(".client-routing.lock"), false)?;
    let mut registry: Registry =
        read_json_optional(&root.join(REGISTRY))?.ok_or("missing client registry")?;
    if let Some(existing) = registry.turns.get(&key) {
        if existing != route {
            return Err("turn origin changed during registration; no event collected".into());
        }
    } else {
        registry.turns.insert(key, route.clone());
    }
    atomic_write_json(&root.join(REGISTRY), &registry)
}

/// Current display identity comes from attachments, never a frozen old turn route.
/// None means this installation has no terminal-client registry.
pub fn attached_thread(
    store: &crate::storage::SpaceStore,
    thread: &str,
    pid: u32,
) -> AppResult<Option<bool>> {
    let Some(registry): Option<Registry> = read_json_optional(&store.state_root.join(REGISTRY))?
    else {
        return Ok(None);
    };
    let meta = store.meta()?.ok_or("missing Space metadata")?;
    Ok(Some(registry.clients.iter().any(|client| {
        client.thread == thread
            && (pid == 0 || client.route.pid == pid)
            && client.route.session_name == meta.session_name
            && client.route.workspace_id == meta.workspace_id
            && meta
                .tab_id
                .as_ref()
                .is_none_or(|tab| *tab == client.route.tab_id)
    })))
}

/// Used while holding the client registry lock before initializing a live scope.
pub fn route_attached(root: &Path, route: &Route, thread: &str) -> AppResult<bool> {
    let Some(registry): Option<Registry> = read_json_optional(&root.join(REGISTRY))? else {
        return Ok(false);
    };
    Ok(registry
        .clients
        .iter()
        .any(|client| client.thread == thread && client.route == *route))
}
