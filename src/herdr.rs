use crate::AppResult;
use crate::config;
use crate::process::run_with_timeout;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const HERDR_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_HERDR_OUTPUT: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct WorkspaceInfo {
    #[serde(default)]
    pub active_tab_id: Option<String>,
    pub workspace_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneInfo {
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub agent_session: Option<AgentSessionInfo>,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub tab_id: Option<String>,
    pub pane_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaneProcessInfo {
    shell_pid: u32,
    #[serde(default)]
    foreground_processes: Vec<PaneProcess>,
}

#[derive(Debug, Deserialize)]
struct PaneProcess {
    pid: u32,
    name: String,
    #[serde(default)]
    argv: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum PanelProcessState {
    Running,
    Shell,
    Occupied,
}

fn panel_process_state(info: &PaneProcessInfo, executables: &[PathBuf]) -> PanelProcessState {
    if info.foreground_processes.iter().any(|process| {
        process.argv.len() == 2
            && process.argv[1] == "panel"
            && executables
                .iter()
                .any(|path| path == Path::new(&process.argv[0]))
    }) {
        return PanelProcessState::Running;
    }
    if let [process] = info.foreground_processes.as_slice()
        && process.pid == info.shell_pid
        && matches!(
            process.name.as_str(),
            "bash" | "zsh" | "sh" | "dash" | "ksh"
        )
    {
        return PanelProcessState::Shell;
    }
    PanelProcessState::Occupied
}

fn process_info(pane_id: &str) -> AppResult<PaneProcessInfo> {
    let value = call(&["pane", "process-info", "--pane", pane_id])?;
    let info = value
        .pointer("/result/process_info")
        .ok_or("missing pane process info")?;
    Ok(serde_json::from_value(info.clone())?)
}

pub fn restore_context_process(pane: &PaneInfo, state_root: &Path) -> AppResult<()> {
    // A persisted pane id alone is not permission to send commands to a reused pane.
    if pane.label.as_deref() != Some("Maeklog") || pane.agent.is_some() {
        return Ok(());
    }
    let root = config::plugin_root()?;
    let executable = root.join("target/release/maeklog");
    let executables = vec![
        executable.clone(),
        config::plugin_config_dir()?.join("bin/maeklog-hook"),
        std::env::current_exe()?,
    ];
    if panel_process_state(&process_info(&pane.pane_id)?, &executables) != PanelProcessState::Shell
    {
        return Ok(());
    }
    if !executable.is_file() {
        return Err("Maeklog panel executable is missing; build the plugin first".into());
    }
    let command = restored_panel_command(
        &executable,
        state_root,
        &root,
        &config::herdr_session_name(),
    );
    call(&["pane", "run", &pane.pane_id, &command])?;
    // Keep the caller's per-space lifecycle lock until exec replaces the shell.
    // Racing startup/focus events must not type a second command into the panel.
    for _ in 0..10 {
        if panel_process_state(&process_info(&pane.pane_id)?, &executables)
            == PanelProcessState::Running
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("Maeklog did not start in its restored pane".into())
}

fn restored_panel_command(
    executable: &Path,
    state_root: &Path,
    root: &Path,
    session: &str,
) -> String {
    format!(
        "exec env MAEKLOG_STATE_DIR={} MAEKLOG_PLUGIN_ROOT={} HERDR_SESSION={} {} panel",
        shell_quote(&state_root.to_string_lossy()),
        shell_quote(&root.to_string_lossy()),
        shell_quote(session),
        shell_quote(&executable.to_string_lossy()),
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentInfo {
    #[serde(default)]
    pub tab_id: Option<String>,
    pub workspace_id: String,
    pub agent: String,
    pub agent_status: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_session: Option<AgentSessionInfo>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentSessionInfo {
    pub value: String,
    #[serde(default)]
    pub kind: String,
}

pub fn workspaces() -> AppResult<Vec<WorkspaceInfo>> {
    let value = call(&["workspace", "list"])?;
    decode_array(&value, "/result/workspaces")
}

pub fn workspace(workspace_id: &str) -> AppResult<Option<WorkspaceInfo>> {
    Ok(workspaces()?
        .into_iter()
        .find(|workspace| workspace.workspace_id == workspace_id))
}

pub fn panes(workspace_id: &str) -> AppResult<Vec<PaneInfo>> {
    let value = call(&["pane", "list", "--workspace", workspace_id])?;
    decode_array(&value, "/result/panes")
}

pub fn agents() -> AppResult<Vec<AgentInfo>> {
    let value = call(&["agent", "list"])?;
    decode_array(&value, "/result/agents")
}

pub fn codex_session_id(workspace_id: &str, tab_id: Option<&str>) -> AppResult<Option<String>> {
    let candidates: Vec<AgentInfo> = agents()?
        .into_iter()
        .filter(|agent| {
            agent.workspace_id == workspace_id
                && agent.agent == "codex"
                && agent.tab_id.as_deref() == tab_id
        })
        .filter(|agent| {
            agent
                .agent_session
                .as_ref()
                .is_some_and(|session| session.kind == "id" && !session.value.is_empty())
        })
        .collect();
    Ok(select_codex_session(candidates))
}

fn select_codex_session(mut candidates: Vec<AgentInfo>) -> Option<String> {
    candidates.sort_by_key(|agent| (agent.focused, agent_status_priority(&agent.agent_status)));
    candidates
        .pop()
        .and_then(|agent| agent.agent_session.map(|session| session.value))
}

pub fn pane_exists(pane_id: &str) -> bool {
    call(&["pane", "get", pane_id]).is_ok()
}

pub fn route_panel_right_click(pane_id: &str, to_panel: bool) -> AppResult<()> {
    call(&[
        "pane",
        "input",
        "--pane",
        pane_id,
        "--right-click",
        if to_panel { "pane" } else { "herdr" },
    ])?;
    Ok(())
}

pub fn open_context_pane(target_pane: &str, focus: bool) -> AppResult<String> {
    let mut arguments = vec![
        "plugin",
        "pane",
        "open",
        "--plugin",
        "maeklog",
        "--entrypoint",
        "context",
        "--placement",
        "split",
        "--target-pane",
        target_pane,
        "--direction",
        "right",
    ];
    arguments.push(if focus { "--focus" } else { "--no-focus" });
    let value = call(&arguments)?;
    let pane_id = value
        .pointer("/result/pane/pane_id")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| find_string_key(result, "pane_id"))
        })
        .map(ToOwned::to_owned)
        .ok_or("Herdr did not return a pane id")?;
    // Plugin splits start at 50:50; Herdr's plugin API has no ratio option.
    // Adjust only this new split, leaving restored/manually resized panes alone.
    // Keep the created pane registered even if the optional resize fails.
    if let Err(error) = call(&[
        "pane",
        "resize",
        "--pane",
        target_pane,
        "--direction",
        "right",
        "--amount",
        "0.13",
    ]) {
        eprintln!("Maeklog opened, but its initial split could not be resized: {error}");
    }
    Ok(pane_id)
}

pub fn close_plugin_pane(pane_id: &str) -> AppResult<()> {
    // Herdr restores the terminal but not its plugin ownership registration.
    // Only the recorded Maeklog pane, still labelled as ours and not repurposed,
    // may use the ordinary close operation after a reboot.
    let value = call(&["pane", "get", pane_id])?;
    let pane: PaneInfo = serde_json::from_value(
        value
            .pointer("/result/pane")
            .ok_or("missing pane info")?
            .clone(),
    )?;
    let root = config::plugin_root()?;
    let executables = vec![
        root.join("target/release/maeklog"),
        config::plugin_config_dir()?.join("bin/maeklog-hook"),
        std::env::current_exe()?,
    ];
    if pane.label.as_deref() != Some("Maeklog")
        || pane.agent.is_some()
        || panel_process_state(&process_info(pane_id)?, &executables) == PanelProcessState::Occupied
    {
        return Err(
            "Recorded Maeklog pane is being used by another process; left unchanged".into(),
        );
    }
    call(&["pane", "close", pane_id])?;
    Ok(())
}

pub fn choose_target_pane<'a>(
    panes: &'a [PaneInfo],
    panel_pane: Option<&str>,
) -> Option<&'a PaneInfo> {
    let eligible = |pane: &&PaneInfo| Some(pane.pane_id.as_str()) != panel_pane;
    panes
        .iter()
        .filter(eligible)
        .find(|pane| pane.agent.as_deref() == Some("codex"))
        .or_else(|| panes.iter().filter(eligible).find(|pane| pane.focused))
        .or_else(|| panes.iter().find(eligible))
}

pub fn pane_cwd(pane: &PaneInfo) -> String {
    pane.foreground_cwd
        .as_ref()
        .or(pane.cwd.as_ref())
        .cloned()
        .unwrap_or_else(|| ".".to_string())
}

fn call(arguments: &[&str]) -> AppResult<Value> {
    let mut command = Command::new(config::herdr_bin());
    command.args(arguments);
    execute_call(&mut command)
}

pub fn call_at(
    arguments: &[&str],
    socket: &Path,
    session: &str,
    executable: &Path,
) -> AppResult<Value> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env("HERDR_SOCKET_PATH", socket)
        .env("HERDR_SESSION", session);
    execute_call(&mut command)
}

fn execute_call(command: &mut Command) -> AppResult<Value> {
    let result = run_with_timeout(command, None, HERDR_TIMEOUT, MAX_HERDR_OUTPUT)?;
    if !result.success {
        return Err(format!(
            "Herdr command failed ({:?}): {}",
            result.code,
            result.stderr.trim()
        )
        .into());
    }
    // pane run acknowledges successful submission with an empty stdout.
    if result.stdout.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&result.stdout)?)
}

fn decode_array<T: for<'de> Deserialize<'de>>(value: &Value, pointer: &str) -> AppResult<Vec<T>> {
    let items = value
        .pointer(pointer)
        .cloned()
        .ok_or_else(|| format!("missing Herdr response field: {pointer}"))?;
    Ok(serde_json::from_value(items)?)
}

fn find_string_key<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(|child| find_string_key(child, key))),
        Value::Array(items) => items.iter().find_map(|child| find_string_key(child, key)),
        _ => None,
    }
}

fn agent_status_priority(status: &str) -> u8 {
    match status {
        "blocked" => 5,
        "working" => 4,
        "done" => 3,
        "idle" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(session: &str, status: &str, focused: bool) -> AgentInfo {
        AgentInfo {
            tab_id: None,
            workspace_id: "w1".into(),
            agent: "codex".into(),
            agent_status: status.into(),
            focused,
            agent_session: Some(AgentSessionInfo {
                value: session.into(),
                kind: "id".into(),
            }),
            terminal_title_stripped: None,
        }
    }

    #[test]
    fn focused_session_wins_then_working_status() {
        let chosen = select_codex_session(vec![
            agent("idle", "idle", false),
            agent("working", "working", false),
            agent("focused", "idle", true),
        ]);
        assert_eq!(chosen.as_deref(), Some("focused"));
    }

    #[test]
    fn restored_shell_is_distinct_from_a_running_panel_and_other_work() {
        let executable = PathBuf::from("/repo/maeklog/target/release/maeklog");
        let mut info: PaneProcessInfo = serde_json::from_value(serde_json::json!({
            "shell_pid": 123,
            "foreground_process_group_id": 123,
            "foreground_processes": [{"pid":123,"name":"bash","argv":["/bin/bash"]}]
        }))
        .unwrap();
        let expected = [executable.clone()];
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Shell
        );
        info.foreground_processes[0].name = "maeklog".into();
        info.foreground_processes[0].argv =
            vec![executable.to_string_lossy().into_owned(), "panel".into()];
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Running
        );
        info.foreground_processes[0].argv[1] = "worker".into();
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Occupied
        );
        info.foreground_processes[0].argv = vec!["/some/other/maeklog".into(), "panel".into()];
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Occupied
        );
        info.foreground_processes[0].name = "bash".into();
        info.foreground_processes[0].pid = 456;
        info.foreground_processes[0].argv = vec!["/bin/bash".into(), "build.sh".into()];
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Occupied
        );
        info.foreground_processes.clear();
        assert_eq!(
            panel_process_state(&info, &expected),
            PanelProcessState::Occupied
        );
    }

    #[test]
    fn restoration_paths_are_literal_shell_arguments() {
        let path = "/repo/space ' $(printf injected) `printf injected`";
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s' {}", shell_quote(path)))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), path);
        assert!(output.stderr.is_empty());
    }
}

#[derive(Clone, Debug)]
pub struct CallerContext {
    pub workspace_id: String,
    pub tab_id: Option<String>,
}
/// The caller's live pane wins over inherited IDs after a pane move. Never use
/// another tab's focused agent as a hook's source. Active-tab fallback is UI-only.
pub fn caller_context(active_fallback: bool) -> AppResult<CallerContext> {
    let workspace_id =
        std::env::var("HERDR_WORKSPACE_ID").map_err(|_| "no Herdr workspace context")?;
    let tab_id = std::env::var("HERDR_TAB_ID")
        .ok()
        .filter(|tab| tab.starts_with(&format!("{workspace_id}:")));
    if std::env::var("HERDR_PANE_ID")
        .ok()
        .is_some_and(|pane| pane.starts_with(&format!("{workspace_id}:")))
        && let Ok(value) = call(&["pane", "current", "--current"])
        && let Some(pane) = value
            .pointer("/result/pane")
            .cloned()
            .and_then(|p| serde_json::from_value::<PaneInfo>(p).ok())
        && !pane.workspace_id.is_empty()
        && pane.tab_id.is_some()
    {
        return Ok(CallerContext {
            workspace_id: pane.workspace_id,
            tab_id: pane.tab_id,
        });
    }
    let tab_id = if tab_id.is_none() && active_fallback {
        workspace(&workspace_id)?.and_then(|w| w.active_tab_id)
    } else {
        tab_id
    };
    Ok(CallerContext {
        workspace_id,
        tab_id,
    })
}
