#!/usr/bin/env python3
"""Isolated model/list and exec fixtures. Never contacts a model or reads credentials."""
import json
import os
from pathlib import Path
import signal
import sys
import time

args = sys.argv[1:]
trace = Path(os.environ["MAEKLOG_TEST_TRACE"])

def record(value):
    with trace.open("a") as output:
        output.write(json.dumps(value, ensure_ascii=False) + "\n")

def reply(request, result):
    print(json.dumps({"id": request["id"], "result": result}), flush=True)

def model(name, efforts, default=None, **extra):
    return {
        "id": name, "model": name, "displayName": name.replace("gpt-test-", "Catalog "),
        "supportedReasoningEfforts": [{"reasoningEffort": effort, "description": "Catalog " + effort} for effort in efforts],
        "defaultReasoningEffort": default, "hidden": False, "isDefault": name == "gpt-test-a", **extra,
    }

if args and args[0] == "app-server":
    case = os.environ.get("MAEKLOG_TEST_CATALOG_CASE", "normal")
    record({"kind": "catalog-start", "args": args, "pid": os.getpid()})
    for line in sys.stdin:
        request = json.loads(line)
        method = request["method"]
        record({"kind": "rpc", "method": method, "params": request.get("params", {}), "pid": os.getpid()})
        if method == "initialize":
            if case == "stderr":
                sys.stderr.write("x" * 100000)
                sys.stderr.flush()
                time.sleep(30)
            reply(request, {"userAgent": "fixture", "platformFamily": "unix", "platformOs": "linux"})
        elif method == "initialized":
            continue
        elif method == "config/read":
            config = {"model_provider": "openai", "chatgpt_base_url": "https://chatgpt.com/backend-api/", "api_key": "SECRET_UNKNOWN_NEVER_LOG"}
            if case == "config":
                config["model_catalog_json"] = "SECRET_CUSTOM_NEVER_LOG"
            if case == "provider":
                config["model_provider"] = "fixture-other-provider"
            reply(request, {"config": config, "origins": {}})
        elif method == "model/list":
            assert request["params"]["includeHidden"] is False
            if case == "protocol-error":
                print(json.dumps({"id": request["id"], "error": {"code": 403, "message": "SECRET_ERROR_NEVER_LOG"}}), flush=True)
                continue
            if case == "malformed":
                print("{invalid response", flush=True)
                continue
            if case == "wrong-id":
                print(json.dumps({"id": request["id"] + 99, "result": {"data": [], "nextCursor": None}}), flush=True)
                continue
            if case == "timeout":
                pid = os.fork()
                if pid == 0:
                    signal.signal(signal.SIGTERM, signal.SIG_IGN)
                    time.sleep(60)
                    sys.exit(0)
                Path(os.environ["MAEKLOG_TEST_CHILD_PID"]).write_text(str(pid))
                time.sleep(60)
            if case == "flood":
                sys.stdout.write("x" * 600000)
                sys.stdout.flush()
                time.sleep(30)
            if case == "empty":
                reply(request, {"data": [], "nextCursor": None})
                continue
            if case == "loop":
                reply(request, {"data": [], "nextCursor": "repeat"})
                continue
            if request["params"].get("cursor") is None:
                first = model("gpt-test-a", ["low", "high"], "high")
                first["displayName"] = "\x1b[31mCatalog A\x1b[0m\n\u202e clean"
                reply(request, {"data": [first, model("gpt-test-hidden", [], hidden=True)], "nextCursor": "page-two"})
            else:
                assert request["params"]["cursor"] == "page-two"
                reply(request, {"data": [
                    model("gpt-test-b", ["high", "xhigh"], "xhigh", inputModalities=["text"]),
                    model("gpt-test-plain", []),
                    model("gpt-test-image", [], inputModalities=["image"]),
                ], "nextCursor": None})
        else:
            raise AssertionError("Unexpected RPC " + method)
    sys.exit(0)

assert args and args[0] == "exec", args
assert "--ignore-user-config" in args and "--ephemeral" in args
assert args[args.index("--sandbox") + 1] == "read-only"
assert 'service_tier="default"' in args and "features.fast_mode=false" in args and "features.hooks=false" in args
selected = args[args.index("--model") + 1]
efforts = [value for value in args if value.startswith("model_reasoning_effort=")]
prompt = sys.stdin.read()
schema = args[args.index("--output-schema") + 1]
kind = "live" if schema.endswith("live-summary.schema.json") else "daily"
record({"kind": kind, "args": args, "model": selected, "efforts": efforts, "prompt": prompt, "pid": os.getpid()})
gate = os.environ.get("MAEKLOG_TEST_GATE_A")
while selected == "gpt-test-a" and gate and Path(gate).exists():
    time.sleep(0.02)
fail = os.environ.get("MAEKLOG_TEST_EXEC_FAIL_FILE")
if fail and Path(fail).exists():
    sys.stderr.write("Fixture account access failure")
    sys.exit(1)
output = Path(args[args.index("--output-last-message") + 1])
if kind == "live":
    output.write_text(json.dumps({"current": {"text": selected + " " + ",".join(efforts), "evidence": ["request"]}, "progress": [], "remaining": []}))
else:
    output.write_text(json.dumps({"objective": "", "objective_evidence": [], "discussion": [], "decisions": [], "progress": [], "blockers": [], "next_actions": [], "notes": [], "record_updates": [], "state_updates": []}))
