use crate::storage::{SpaceStore, lock_file, make_scope_key};
use crate::{AppResult, codex_hooks, config, herdr, worker};
use serde_json::Value;

pub fn run(action: Option<&str>) -> AppResult<()> {
    match action.unwrap_or("help") {
        "open" | "enable" => open_current(true),
        "hide" => hide_current(false),
        "disable" => hide_current(true),
        "clear-records" => {
            hide_current(true)?;
            let store = current_store()?;
            let count = crate::maintenance::clear_records(&store)?;
            println!(
                "Deleted {count} private files; this tab remains off. Saved branch records are retained; use memory clear for an explicit exact scope."
            );
            Ok(())
        }
        "refresh" => worker::request_refresh(&current_store()?).map(|_| ()),
        "rebuild" => worker::request_rebuild(&current_store()?),
        "startup" => restore_visible(),
        "workspace-focused" => workspace_focused(),
        "workspace-closed" => workspace_closed(),
        "pane-closed" => pane_closed(true),
        "pane-exited" => pane_closed(false),
        "install-codex-hooks" => codex_hooks::install(),
        "remove-codex-hooks" => codex_hooks::remove(),
        _ => Err("expected a Maeklog action name".into()),
    }
}
fn current_store() -> AppResult<SpaceStore> {
    let target = herdr::caller_context(true)?;
    SpaceStore::find_for_scope(
        &config::state_root()?,
        &config::herdr_session_name(),
        &target.workspace_id,
        target.tab_id.as_deref(),
    )
    .ok_or_else(|| {
        crate::tr!(
            "이 탭에서 Maeklog를 먼저 열어 주세요.",
            "Open Maeklog in this tab first."
        )
        .into()
    })
}
fn open_current(explicit: bool) -> AppResult<()> {
    let target = herdr::caller_context(true)?;
    open_scope(&target.workspace_id, target.tab_id.as_deref(), explicit)
}
fn open_scope(workspace_id: &str, tab_id: Option<&str>, explicit: bool) -> AppResult<()> {
    let root = config::state_root()?;
    let session = config::herdr_session_name();
    let _lifecycle = lock_file(
        &root.join("panel-locks").join(format!(
            "{}.lock",
            make_scope_key(&session, workspace_id, tab_id)
        )),
        false,
    )?;
    let existing = SpaceStore::find_for_scope(&root, &session, workspace_id, tab_id);
    if !explicit
        && existing
            .as_ref()
            .is_some_and(|s| !crate::participation::load(s).is_ok_and(|p| p.enabled))
    {
        return Ok(());
    }
    let workspace = herdr::workspace(workspace_id)?.ok_or("workspace is no longer live")?;
    let panes: Vec<_> = herdr::panes(workspace_id)?
        .into_iter()
        .filter(|p| p.tab_id.as_deref() == tab_id)
        .collect();
    let previous_panel = existing
        .as_ref()
        .and_then(|s| s.ui().ok())
        .and_then(|ui| ui.panel_pane_id);
    let target = herdr::choose_target_pane(&panes, previous_panel.as_deref())
        .ok_or("this tab has no target pane")?;
    let store = SpaceStore::for_scope(
        &root,
        &session,
        workspace_id,
        tab_id,
        &herdr::pane_cwd(target),
        Some(&workspace.label),
    )?;
    if !crate::participation::load(&store)?.enabled {
        if !explicit {
            return Ok(());
        }
        crate::daily::resume_three(&store)?;
        crate::participation::set_enabled(&store, true)?;
    }
    // Hooks remain authoritative over a pane's stale resumed session id.
    if store.latest_session_id()?.is_none()
        && let Some(id) = herdr::codex_session_id(workspace_id, tab_id)?
    {
        store.remember_session(&id)?;
    }
    worker::resume_pending(&store)?;
    let mut ui = store.ui()?;
    if explicit {
        ui.visible = true;
    } else if !ui.visible {
        return Ok(());
    }
    if let Some(pane) = panes
        .iter()
        .find(|p| Some(p.pane_id.as_str()) == ui.panel_pane_id.as_deref())
    {
        return herdr::restore_context_process(pane, &root);
    }
    let pane_id = herdr::open_context_pane(&target.pane_id, false)?;
    ui.visible = true;
    ui.panel_pane_id = Some(pane_id);
    store.set_ui(ui)
}
fn hide_current(disable: bool) -> AppResult<()> {
    let target = herdr::caller_context(true)?;
    let root = config::state_root()?;
    let session = config::herdr_session_name();
    let _lifecycle = lock_file(
        &root.join("panel-locks").join(format!(
            "{}.lock",
            make_scope_key(&session, &target.workspace_id, target.tab_id.as_deref())
        )),
        false,
    )?;
    let store = match SpaceStore::find_for_scope(
        &root,
        &session,
        &target.workspace_id,
        target.tab_id.as_deref(),
    ) {
        Some(store) => store,
        None if disable => {
            let panes: Vec<_> = herdr::panes(&target.workspace_id)?
                .into_iter()
                .filter(|p| p.tab_id == target.tab_id)
                .collect();
            let pane =
                herdr::choose_target_pane(&panes, None).ok_or("this tab has no target pane")?;
            SpaceStore::for_scope(
                &root,
                &session,
                &target.workspace_id,
                target.tab_id.as_deref(),
                &herdr::pane_cwd(pane),
                None,
            )?
        }
        None => return Ok(()),
    };
    if disable {
        crate::participation::set_enabled(&store, false)?;
    }
    let mut ui = store.ui()?;
    let panel = ui.panel_pane_id.take();
    ui.visible = false;
    store.set_ui(ui)?;
    if let Some(pane_id) = panel
        && herdr::pane_exists(&pane_id)
    {
        herdr::close_plugin_pane(&pane_id)?;
    }
    Ok(())
}
fn restore_visible() -> AppResult<()> {
    let root = config::state_root()?;
    let session = config::herdr_session_name();
    let live: std::collections::HashSet<_> = herdr::workspaces()?
        .into_iter()
        .map(|w| w.workspace_id)
        .collect();
    for store in SpaceStore::all(&root)? {
        let Some(meta) = store.meta()? else {
            continue;
        };
        if !crate::participation::load(&store)?.enabled
            || meta.archived
            || meta.session_name != session
            || !live.contains(&meta.workspace_id)
            || store.legacy_with_tabs()?
        {
            continue;
        }
        let _ = worker::resume_pending(&store);
        if store.ui()?.visible
            && let Err(error) = open_scope(&meta.workspace_id, meta.tab_id.as_deref(), false)
        {
            eprintln!(
                "Maeklog restore failed for {} {:?}: {error}",
                meta.workspace_id, meta.tab_id
            );
        }
    }
    Ok(())
}
fn workspace_focused() -> AppResult<()> {
    let id = event_value("workspace_id")
        .or_else(|| std::env::var("HERDR_WORKSPACE_ID").ok())
        .ok_or("workspace event has no workspace id")?;
    let tab = herdr::workspace(&id)?.and_then(|w| w.active_tab_id);
    // New tabs are collected by hooks; opening is explicit. Restoring a known
    // visible tab must not reopen the panel in another active tab.
    let root = config::state_root()?;
    if SpaceStore::find_for_scope(&root, &config::herdr_session_name(), &id, tab.as_deref())
        .is_some()
    {
        open_scope(&id, tab.as_deref(), false)?;
    }
    Ok(())
}
fn workspace_closed() -> AppResult<()> {
    let id = event_value("workspace_id").ok_or("workspace event has no workspace id")?;
    for store in SpaceStore::all(&config::state_root()?)? {
        if store
            .meta()?
            .is_some_and(|m| m.workspace_id == id && m.session_name == config::herdr_session_name())
        {
            store.archive()?;
        }
    }
    Ok(())
}
fn pane_closed(user_closed: bool) -> AppResult<()> {
    let Some(id) = event_value("pane_id") else {
        return Ok(());
    };
    let Some(store) = SpaceStore::find_by_panel_pane(&config::state_root()?, &id)? else {
        return Ok(());
    };
    let mut ui = store.ui()?;
    ui.panel_pane_id = None;
    if user_closed {
        ui.visible = false;
    }
    store.set_ui(ui)
}
fn event_value(key: &str) -> Option<String> {
    let value: Value =
        serde_json::from_str(&std::env::var("HERDR_PLUGIN_EVENT_JSON").ok()?).ok()?;
    find_string_key(&value, key).map(ToOwned::to_owned)
}
fn find_string_key<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(|v| find_string_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_string_key(v, key)),
        _ => None,
    }
}
