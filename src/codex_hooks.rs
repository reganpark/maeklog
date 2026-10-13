use crate::AppResult;
use crate::config::{self, HookRuntimeConfig};
use crate::storage::{atomic_write_json, unix_now};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const HOOK_EVENTS: [(&str, u64); 4] = [
    ("SessionStart", 5),
    ("UserPromptSubmit", 5),
    ("Stop", 5),
    ("Interrupt", 3),
];

pub fn install() -> AppResult<()> {
    let config_directory = config::private_dir(config::plugin_config_dir()?)?;
    let bin_directory = config::private_dir(config_directory.join("bin"))?;
    let hook_binary = bin_directory.join("maeklog-hook");
    install_current_binary(&hook_binary)?;

    let runtime = HookRuntimeConfig {
        state_dir: config::state_root()?,
        plugin_root: config::plugin_root()?,
    };
    atomic_write_json(&config_directory.join("hook-runtime.json"), &runtime)?;

    let codex_home = config::private_dir(config::codex_home()?)?;
    let hooks_path = codex_home.join("hooks.json");
    let mut root = read_hooks(&hooks_path)?;
    let command = hook_command(&hook_binary);
    remove_maeklog_handlers(&mut root, &command);
    let hooks = hooks_object_mut(&mut root)?;
    for (event, timeout) in HOOK_EVENTS {
        let groups = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} must be an array"))?;
        groups.push(json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout
            }]
        }));
    }
    backup_if_present(&hooks_path)?;
    atomic_write_json(&hooks_path, &root)?;
    println!(
        "Installed Maeklog hooks in {}. Review and trust them with /hooks before use.",
        hooks_path.display()
    );
    Ok(())
}

pub fn remove() -> AppResult<()> {
    let hooks_path = config::codex_home()?.join("hooks.json");
    if !hooks_path.is_file() {
        println!("No Codex hooks file found.");
        return Ok(());
    }
    let mut root = read_hooks(&hooks_path)?;
    let hook_binary = config::plugin_config_dir()?.join("bin/maeklog-hook");
    let command = hook_command(&hook_binary);
    let changed = remove_maeklog_handlers(&mut root, &command);
    if changed {
        backup_if_present(&hooks_path)?;
        atomic_write_json(&hooks_path, &root)?;
        println!(
            "Removed Maeklog hook entries from {}.",
            hooks_path.display()
        );
    } else {
        println!("No Maeklog hook entries were installed.");
    }
    Ok(())
}

fn install_current_binary(destination: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let source = std::env::current_exe()?;
    let temporary = destination.with_extension(format!("new-{}", std::process::id()));
    fs::copy(source, &temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    fs::rename(&temporary, destination)?;
    Ok(())
}

fn read_hooks(path: &Path) -> AppResult<Value> {
    if !path.is_file() {
        return Ok(json!({ "hooks": {} }));
    }
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    if !value.is_object() {
        return Err("Codex hooks.json root must be an object".into());
    }
    Ok(value)
}

fn hooks_object_mut(root: &mut Value) -> AppResult<&mut Map<String, Value>> {
    let object = root
        .as_object_mut()
        .ok_or("hooks.json root is not an object")?;
    let hooks = object
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    hooks
        .as_object_mut()
        .ok_or_else(|| "hooks.json 'hooks' field must be an object".into())
}

fn remove_maeklog_handlers(root: &mut Value, command: &str) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let original_handlers = handlers.len();
            // Only the command generated for this configured installation is ours.
            handlers.retain(|handler| {
                handler.get("type").and_then(Value::as_str) != Some("command")
                    || handler.get("command").and_then(Value::as_str) != Some(command)
            });
            let removed = handlers.len() != original_handlers;
            changed |= removed;
            // Leave unrelated empty groups and malformed definitions untouched.
            !removed || !handlers.is_empty()
        });
    }
    changed
}

fn backup_if_present(path: &Path) -> AppResult<Option<PathBuf>> {
    use std::os::unix::fs::PermissionsExt;

    if !path.is_file() {
        return Ok(None);
    }
    let backup = path.with_extension(format!("json.maeklog-backup-{}", unix_now()));
    fs::copy(path, &backup)?;
    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))?;
    Ok(Some(backup))
}

fn hook_command(hook_binary: &Path) -> String {
    format!("{} hook", shell_quote(hook_binary))
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_exact_owned_command_handlers() {
        let command = hook_command(Path::new("/tmp/maeklog-config/bin/maeklog-hook"));
        let unrelated = json!([
            {"type": "command", "command": "keep-me", "timeout": 99},
            {"type": "command", "command": "'/tmp/maeklog-hook-wrapper' hook"},
            {"type": "command", "command": "'/tmp/other-config/bin/maeklog-hook' hook"},
            {"type": "command", "command": format!("{command} --verbose")},
            {"type": "command", "command": format!("env {command}")},
            {"type": "prompt", "command": command},
            {"command": command}
        ]);
        let mut handlers = unrelated.as_array().unwrap().clone();
        handlers.insert(1, json!({"type": "command", "command": command}));
        let mut value = json!({
            "hooks": {"Stop": [{"matcher": "fixture", "hooks": handlers}]},
            "custom": true
        });

        assert!(remove_maeklog_handlers(&mut value, &command));
        assert_eq!(value.pointer("/hooks/Stop/0/hooks"), Some(&unrelated));
        assert_eq!(
            value.pointer("/hooks/Stop/0/matcher"),
            Some(&json!("fixture"))
        );
        assert_eq!(value["custom"], true);
        assert!(!remove_maeklog_handlers(&mut value, &command));
    }

    #[test]
    fn prunes_only_groups_emptied_by_owned_handler_removal() {
        let command = hook_command(Path::new("/tmp/maeklog-config/bin/maeklog-hook"));
        let untouched = json!([
            {"matcher": "already-empty", "hooks": []},
            {"matcher": "missing-handlers"},
            {"matcher": "malformed", "hooks": "preserve-me"}
        ]);
        let mut value = json!({"hooks": {"Stop": untouched, "custom": "preserve-me"}});
        assert!(!remove_maeklog_handlers(&mut value, &command));
        for (event, _) in HOOK_EVENTS {
            if let Some(groups) = value["hooks"][event].as_array_mut() {
                groups.push(json!({"hooks": [{"type": "command", "command": command}]}));
            } else {
                value["hooks"][event] =
                    json!([{"hooks": [{"type": "command", "command": command}]}]);
            }
        }

        assert!(remove_maeklog_handlers(&mut value, &command));
        assert_eq!(value["hooks"]["Stop"], untouched);
        for (event, _) in HOOK_EVENTS {
            if event != "Stop" {
                assert_eq!(value["hooks"][event], json!([]));
            }
        }
        assert_eq!(value["hooks"]["custom"], "preserve-me");
    }

    #[test]
    fn owns_only_the_generated_command_with_spaces_and_quotes_in_its_path() {
        let path = Path::new("/tmp/Maeklog user's config/bin/maeklog-hook");
        let command = hook_command(path);
        assert_eq!(
            command,
            "'/tmp/Maeklog user'\\''s config/bin/maeklog-hook' hook"
        );
        let unquoted = format!("{} hook", path.display());
        let mut value = json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": command},
            {"type": "command", "command": unquoted}
        ]}]}});

        assert!(remove_maeklog_handlers(&mut value, &command));
        assert_eq!(
            value.pointer("/hooks/Stop/0/hooks"),
            Some(&json!([
                {"type": "command", "command": unquoted}
            ]))
        );
    }
}
