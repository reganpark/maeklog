#!/usr/bin/env python3
"""Small local Herdr fixture; avoids Node startup on each bounded bridge call."""
import json
import os
from pathlib import Path
import sys
import tempfile


path = Path(os.environ["PM_CLIENT_TOPOLOGY"])
state = json.loads(path.read_text())
args = sys.argv[1:]
if os.environ.get("HERDR_SOCKET_PATH") != state["socket"]:
    raise RuntimeError("wrong Herdr endpoint")


def lookup(identifier):
    identifier = state.get("aliases", {}).get(identifier, identifier)
    return next((pane for pane in state["panes"] if pane["pane_id"] == identifier), None)


changed = False
if args[:2] == ["pane", "input"]:
    result = {}
elif args[:2] == ["agent", "list"]:
    result = {"agents": [{**pane, "agent": "codex", "agent_status": "working",
                          "terminal_title_stripped": "테스트 작업"} for pane in state["panes"]]}
elif args[:2] == ["pane", "list"]:
    result = {"panes": state["panes"]}
elif args[:2] == ["pane", "current"]:
    result = {"pane": lookup(args[3] if len(args) > 3 else os.environ.get("HERDR_PANE_ID"))}
elif args[:2] == ["pane", "get"]:
    result = {"pane": lookup(args[2])}
elif args[:2] == ["pane", "process-info"]:
    pane = lookup(args[3])
    name = pane.get("process_name", "codex") if pane else None
    result = {"process_info": {"foreground_processes":
              [{"pid": pane["pid"], "name": name, "argv": [name]}] if pane else []}}
elif args[:2] == ["pane", "report-agent-session"]:
    pane = lookup(args[2])
    if pane is None:
        raise RuntimeError("missing pane")
    pane["agent_session"] = {"kind": "id", "value": args[args.index("--agent-session-id") + 1],
                             "source": "herdr:codex"}
    result, changed = {}, True
elif args[:2] == ["pane", "release-agent"]:
    pane = lookup(args[2])
    if pane is None:
        raise RuntimeError("missing pane")
    pane.pop("agent_session", None)
    result, changed = {}, True
else:
    raise RuntimeError(f"Unexpected call: {args!r}")

if changed:
    descriptor, temporary = tempfile.mkstemp(prefix="topology-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w") as stream:
            json.dump(state, stream)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)
print(json.dumps({"result": result}))
