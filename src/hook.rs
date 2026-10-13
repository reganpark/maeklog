use crate::AppResult;
use crate::config;
use crate::model::{CodexHookInput, EventKind};
use crate::storage::{NewTurnEvent, SpaceStore};
use crate::worker;
use std::io::Read;

const MAX_HOOK_INPUT_BYTES: u64 = 1_048_576;

pub fn entry() {
    if std::env::var_os("MAEKLOG_INTERNAL").is_some() {
        println!("{{}}");
        return;
    }

    match record() {
        Ok(Some(context)) => {
            println!(
                "{}",
                serde_json::json!({"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":context}})
            );
            return;
        }
        Err(error) => eprintln!("maeklog hook: {error}"),
        Ok(None) => (),
    }
    println!("{{}}");
}

#[allow(
    clippy::too_many_lines,
    reason = "A single hook transaction binds origin, participation and both durable and live capture"
)]
fn record() -> AppResult<Option<String>> {
    let input = read_input()?;
    if !supported(&input) {
        return Ok(None);
    }
    let state_root = config::state_root()?;
    let route = match crate::client_routing::resolve(&state_root, &input) {
        // SessionStart can precede the TUI receiving its newly created thread ID.
        Err(_) if input.hook_event_name == "SessionStart" => return Ok(None),
        result => result?,
    };
    if route.is_none() && std::env::var_os("HERDR_WORKSPACE_ID").is_none() {
        return Ok(None);
    }
    let target = crate::client_routing::caller_context(route.as_ref())?;
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir()?.to_string_lossy().into_owned()
    } else {
        input.cwd.clone()
    };
    let session_name = route
        .as_ref()
        .map_or_else(config::herdr_session_name, |route| {
            route.session_name.clone()
        });
    let terminal_scope = matches!(input.hook_event_name.as_str(), "Stop" | "Interrupt")
        .then(|| {
            SpaceStore::find_for_scope(
                &state_root,
                &session_name,
                &target.workspace_id,
                target.tab_id.as_deref(),
            )
        })
        .flatten();
    let store = match terminal_scope {
        Some(store) => store,
        None => SpaceStore::for_scope(
            &state_root,
            &session_name,
            &target.workspace_id,
            target.tab_id.as_deref(),
            &cwd,
            None,
        )?,
    };
    if matches!(input.hook_event_name.as_str(), "Stop" | "Interrupt")
        && let Err(error) = crate::task_report::drain(&store)
    {
        eprintln!("maeklog task reports: {error}");
    }
    let participation_guard = crate::participation::lock(&store)?;
    let mut preference = crate::participation::load(&store)?;
    if !preference.enabled {
        return Ok(None);
    }
    if input.hook_event_name != "SessionStart"
        && let Some(route) = &route
    {
        crate::client_routing::remember_turn(&state_root, &input, route)?;
    }
    // A Stop from a turn begun while opted out must not leak back in on enable.
    if preference.history_blocked {
        match input.hook_event_name.as_str() {
            "UserPromptSubmit" => {
                preference
                    .open_turns
                    .insert(input.session_id.clone(), input.turn_id.clone());
            }
            "Stop" | "Interrupt" => {
                let Some(turn) = preference.open_turns.get(&input.session_id) else {
                    return Ok(None);
                };
                if turn
                    .as_ref()
                    .zip(input.turn_id.as_ref())
                    .is_some_and(|(a, b)| a != b)
                {
                    return Ok(None);
                }
                preference.open_turns.remove(&input.session_id);
            }
            _ => {}
        }
        crate::participation::save(&store, &preference)?;
    }
    crate::connection::record(&store, &input.session_id, &input.hook_event_name)?;
    if input.hook_event_name == "SessionStart" {
        drop(participation_guard);
        worker::resume_pending(&store)?;
        return Ok(None);
    }

    let (kind, content, should_reduce) = match input.hook_event_name.as_str() {
        "UserPromptSubmit" => (
            EventKind::UserPrompt,
            input.prompt.unwrap_or_default(),
            false,
        ),
        "Stop" => (
            EventKind::AssistantFinal,
            input.last_assistant_message.unwrap_or_default(),
            true,
        ),
        "Interrupt" => (EventKind::Interrupted, String::new(), true),
        _ => return Ok(None),
    };

    let event = store.append_event(NewTurnEvent {
        kind,
        session_id: input.session_id,
        turn_id: input.turn_id,
        model: input.model,
        content,
        cwd,
    })?;
    if let Err(error) = crate::live_turn::record_hook(&store, &event) {
        eprintln!("maeklog live progress: {error}");
    }
    let producer_notice =
        route.as_ref().and_then(|route| {
            match crate::task_report::producer_context(&store, route, &event) {
                Ok(notice) => notice,
                Err(error) => {
                    eprintln!("maeklog task producer: {error}");
                    None
                }
            }
        });
    drop(participation_guard);
    if should_reduce {
        worker::spawn_detached()?;
    }
    Ok(producer_notice)
}

fn read_input() -> AppResult<CodexHookInput> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_HOOK_INPUT_BYTES)
        .read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn supported(input: &CodexHookInput) -> bool {
    input.agent_id.is_none()
        && input.agent_type.is_none()
        && matches!(
            input.hook_event_name.as_str(),
            "SessionStart" | "UserPromptSubmit" | "Stop" | "Interrupt"
        )
        && !(input.hook_event_name == "SessionStart" && input.source.as_deref() == Some("compact"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_limit_is_bounded() {
        assert_eq!(MAX_HOOK_INPUT_BYTES, 1_048_576);
    }

    #[test]
    fn nested_agent_hooks_never_collect_parent_task_or_issue_a_report_capability() {
        for (agent_id, agent_type) in [
            (Some("child".into()), None),
            (None, Some("reviewer".into())),
        ] {
            let input = CodexHookInput {
                hook_event_name: "UserPromptSubmit".into(),
                agent_id,
                agent_type,
                ..CodexHookInput::default()
            };
            assert!(!supported(&input));
        }
    }
}
