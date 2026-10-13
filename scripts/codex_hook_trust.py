"""Read-only Maeklog hook preflight for the supported Codex 0.162.1 format.

The fingerprint follows codex-hooks/engine/discovery.rs and
codex-config/fingerprint.rs: normalize one handler, omit TOML None values,
serialize recursively sorted compact JSON, then SHA-256. This module never
creates approvals or executes hooks.
"""
import hashlib
import json
import os
from pathlib import Path
import tomllib


EVENTS = {"SessionStart": "session_start", "UserPromptSubmit": "user_prompt_submit",
          "Stop": "stop", "Interrupt": "interrupt"}


def installation_paths(home=None):
    home = Path.home() if home is None else Path(home)
    config = Path(os.environ.get("XDG_CONFIG_HOME", home / ".config"))
    default_plugin = config / "herdr/plugins/config/maeklog"
    plugin = Path(os.environ.get("HERDR_PLUGIN_CONFIG_DIR", default_plugin))
    if plugin.resolve() != default_plugin.resolve():
        raise RuntimeError("This checked native bridge requires the Maeklog configuration under "
                           "XDG_CONFIG_HOME/herdr/plugins/config/maeklog. A separate "
                           "HERDR_PLUGIN_CONFIG_DIR is not supported; use the same XDG_CONFIG_HOME "
                           "for hook installation, /hooks review, update and Codex launch.")
    return plugin, Path(os.environ.get("CODEX_HOME", home / ".codex"))


def hook_command(binary):
    return "'" + str(binary).replace("'", "'\\''") + "' hook"


def command_hash(event, group, handler):
    """Compute the native fingerprint for a Maeklog-owned Unix command hook."""
    if event not in EVENTS or handler.get("type") != "command":
        raise RuntimeError("Expected a supported command hook")
    command = handler.get("command")
    if not isinstance(command, str) or not command.strip():
        raise RuntimeError("Hook command must be a nonempty string")
    timeout = handler.get("timeout")
    if timeout is not None and (type(timeout) is not int or not 0 <= timeout < 2**64):
        raise RuntimeError("Hook timeout must be a nonnegative integer")
    timeout = (min(max(1, 1 if timeout is None else timeout), 3) if event == "Interrupt"
               else max(1, 600 if timeout is None else timeout))
    asynchronous = handler.get("async", False)
    if type(asynchronous) is not bool:
        raise RuntimeError("Hook async must be true or false")
    normalized = {"type": "command", "command": command, "timeout": timeout,
                  "async": asynchronous}
    for name in ["commandWindows", "command_windows", "statusMessage"]:
        if handler.get(name) is not None and not isinstance(handler[name], str):
            raise RuntimeError(f"Hook {name} must be a string")
    if handler.get("statusMessage") is not None:
        normalized["statusMessage"] = handler["statusMessage"]
    limit = handler.get("additionalContextLimit")
    if limit is not None and (type(limit) is not int or not 0 <= limit < 2**64):
        raise RuntimeError("Hook additionalContextLimit must be a nonnegative integer")
    if event in ["SessionStart", "UserPromptSubmit"] and limit is not None and limit != 2500:
        normalized["additionalContextLimit"] = limit
    identity = {"event_name": EVENTS[event], "hooks": [normalized]}
    matcher = group.get("matcher")
    if matcher is not None and not isinstance(matcher, str):
        raise RuntimeError("Hook matcher must be a string")
    if event == "SessionStart" and matcher is not None:
        identity["matcher"] = matcher
    payload = json.dumps(identity, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(payload).hexdigest()


def approved_hooks(home=None):
    """Return exactly four current, enabled, manually trusted hook definitions."""
    plugin, codex_home = installation_paths(home)
    binary = plugin / "bin/maeklog-hook"
    review = "Run Install Maeklog Codex hooks, then review and approve all four with Codex /hooks."
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError(f"Maeklog hook binary is not installed. {review}")
    try:
        runtime = json.loads((plugin / "hook-runtime.json").read_text())
        if not isinstance(runtime, dict) or any(
                not isinstance(runtime.get(key), str) or not Path(runtime[key]).is_absolute()
                or ".." in Path(runtime[key]).parts for key in ["state_dir", "plugin_root"]):
            raise ValueError("runtime paths must be absolute")
        config = tomllib.loads((codex_home / "config.toml").read_text())
        definitions = json.loads((codex_home / "hooks.json").read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError(f"Cannot read the Maeklog/Codex hook configuration: {error}. {review}") from None
    features = config.get("features", {})
    if not isinstance(features, dict) or features.get("hooks", True) is not True:
        raise RuntimeError("Codex hooks are disabled or invalid in [features]. Enable hooks before /hooks review.")
    if not isinstance(definitions, dict) or set(definitions) - {"description", "hooks"}:
        raise RuntimeError("Codex hooks.json must contain a hooks object and optional description.")
    if definitions.get("description") is not None and not isinstance(definitions["description"], str):
        raise RuntimeError("Codex hooks.json description must be a string.")
    events = definitions.get("hooks", {})
    if not isinstance(events, dict):
        raise RuntimeError("Codex hooks.json 'hooks' must be an object.")
    hook_config = config.get("hooks", {})
    if not isinstance(hook_config, dict):
        raise RuntimeError("Codex [hooks] must be a table. Review the configuration with /hooks.")
    states = hook_config.get("state", {})
    if not isinstance(states, dict):
        raise RuntimeError("Codex [hooks.state] must be a table. Review the configuration with /hooks.")
    command = hook_command(binary)
    approved = {}
    for event, label in EVENTS.items():
        groups = events.get(event, [])
        if not isinstance(groups, list):
            raise RuntimeError(f"Codex hooks.{event} must be an array of hook groups.")
        matches = []
        for group_index, group in enumerate(groups):
            if not isinstance(group, dict) or not isinstance(group.get("hooks", []), list):
                raise RuntimeError(f"Codex hooks.{event} must use groups containing a hooks array.")
            for handler_index, handler in enumerate(group.get("hooks", [])):
                if not isinstance(handler, dict):
                    raise RuntimeError(f"Codex hooks.{event} handlers must be objects.")
                if handler.get("type") == "command" and handler.get("command") == command:
                    key = f"{codex_home}/hooks.json:{label}:{group_index}:{handler_index}"
                    matches.append((key, group, handler))
        if len(matches) != 1:
            raise RuntimeError(f"Expected exactly one installed Maeklog {event} hook; found {len(matches)}. {review}")
        key, group, handler = matches[0]
        try:
            current_hash = command_hash(event, group, handler)
        except RuntimeError as error:
            raise RuntimeError(f"Invalid Maeklog {event} hook: {error}. {review}") from None
        # The installer generates synchronous, unfiltered hooks. A custom filter
        # or async override can skip collection even after its definition is approved.
        if group.get("matcher") is not None or handler.get("async", False):
            raise RuntimeError(f"Maeklog {event} must be synchronous and unfiltered. {review}")
        state = states.get(key, {})
        if not isinstance(state, dict) or state.get("enabled", True) is not True:
            raise RuntimeError(f"Maeklog {event} hook is disabled. {review}")
        if state.get("trusted_hash") != current_hash:
            raise RuntimeError(f"Maeklog {event} hook is untrusted or its definition changed. {review}")
        approved[event] = {"key": key, "group": group, "handler": handler,
                           "trusted_hash": current_hash}
    return binary, approved
