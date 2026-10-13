#!/usr/bin/env python3
"""Real Maeklog CLI, isolated settings/state, fake Codex only; no model inference."""
import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

repo = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve()
root = Path(tempfile.mkdtemp(prefix="maeklog-model-settings-qa-"))
(root / "codex-home").mkdir()
trace = root / "trace.jsonl"
config = root / "config"
settings = config / "model-settings.json"
language_settings = config / "language-settings.json"
state = root / "state"
state.mkdir()
gate = root / "gate-a"
fail_file = root / "fail-exec"
env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", LC_ALL="C.UTF-8", HERDR_PLUGIN_CONFIG_DIR=str(config), CODEX_HOME=str(root / "codex-home"),
    MAEKLOG_STATE_DIR=str(state), MAEKLOG_PLUGIN_ROOT=str(repo), MAEKLOG_CODEX_BIN=str(repo / "scripts/mock-model-catalog.py"),
    MAEKLOG_TEST_TRACE=str(trace), MAEKLOG_TEST_CHILD_PID=str(root / "child.pid"), MAEKLOG_TEST_GATE_A=str(gate),
    MAEKLOG_TEST_EXEC_FAIL_FILE=str(fail_file), MAEKLOG_TEST_CATALOG_CASE="normal",
    HERDR_ENV="1", HERDR_WORKSPACE_ID="wA", HERDR_TAB_ID="wA:t1", HERDR_PANE_ID="wA:p1", HERDR_SESSION="model-settings-test",
    HERDR_SOCKET_PATH="/fixture/model-settings.sock", HERDR_BIN_PATH=str(repo / "scripts/mock-live-herdr.mjs"),
    PM_CLIENT_TOPOLOGY=str(root / "topology.json"), TERM="xterm-256color")
for key in ["MAEKLOG_INTERNAL", "MAEKLOG_DISABLE_LUNA"]:
    env.pop(key, None)
(root / "topology.json").write_text(json.dumps({"socket": env["HERDR_SOCKET_PATH"], "panes": [{"pane_id": "wA:p1", "workspace_id": "wA", "tab_id": "wA:t1", "terminal_id": "terminal-1", "pid": 101, "agent": "codex", "cwd": str(root)}]}))
(state / "client-routing.json").write_text(json.dumps({"clients": [], "turns": {}}))
worker = None
guard = None
sequence = 0

def run(args, payload=None, extra=None, success=True, executable=None, unset=()):
    child_env = dict(env, **(extra or {}))
    for key in unset:
        child_env.pop(key, None)
    result = subprocess.run([str(executable or binary), *args], cwd=repo, env=child_env,
        input=json.dumps(payload) if payload is not None else None, text=True, capture_output=True, timeout=14)
    assert (result.returncode == 0) == success, (args, result.stdout, result.stderr)
    return result

def entries():
    return [json.loads(line) for line in trace.read_text().splitlines()] if trace.exists() else []

def calls(kind=None):
    return [row for row in entries() if row["kind"] in ([kind] if kind else ["daily", "live"])]

def get(path):
    return json.loads(path.read_text())

def until(check, label, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.025)
    raise AssertionError(label)

def live(method, **fields):
    global sequence
    sequence += 1
    params = {"threadId": "thread-a", **fields}
    if method != "turn/started":
        params["turnId"] = "turn-a"
    return run(["live", "event", "101", "thread-a"], {"sequence": sequence, "cwd": str(root), "notification": {"method": method, "params": params}})


def remove_fixture_after_worker_drain():
    # The coordinator holds worker.lock until every worker-space child is reaped.
    # A worker-space owns .worker.lock through its final status/processing writes.
    # These leases belong only to this fixture; no process-name or global kill.
    paths = [state / "worker.lock", *sorted((state / "spaces").glob("*/.worker.lock"))]
    leases = []
    deadline = time.monotonic() + 10
    try:
        for path in paths:
            lease = path.open("a+")
            leases.append(lease)
            while True:
                try:
                    fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise AssertionError(f"Fixture worker did not drain before cleanup: {path}")
                    time.sleep(0.025)
        assert worker is None or worker.poll() is not None, "tracked fixture coordinator still active"
        shutil.rmtree(root)
        time.sleep(0.2)
        assert not root.exists(), "a fixture worker recreated the removed root"
        cleanup = {"passed": True, "coordinator_lease_drained": True,
            "space_lease_count": len(paths) - 1, "tracked_worker_reaped": worker is None or worker.poll() is not None,
            "root_removed_without_recreation": True, "global_process_control": False}
        if len(sys.argv) > 2:
            destination = Path(sys.argv[2]).resolve()
            destination.mkdir(parents=True, exist_ok=True)
            (destination / "cleanup-result.json").write_text(json.dumps(cleanup, indent=2) + "\n")
    finally:
        for lease in reversed(leases):
            lease.close()

try:
    default = run(["model", "show"]).stdout
    assert "gpt-6-luna" in default and "medium" in default and "Standard" in default and "English (en)" in default
    assert not settings.exists() and not calls()
    auto = run(["language", "show"]).stdout
    assert "Auto (auto)" in auto and "English (en)" in auto
    assert not language_settings.exists() and not config.exists()
    run(["language", "set", "ko"])
    assert get(language_settings) == {"language": "ko"}
    assert "요약 모델" in run(["model", "show"]).stdout
    assert "Korean (ko)" in run(["language", "show"], extra={"LC_ALL": "C.UTF-8"}).stdout
    run(["language", "set", "en"])
    assert "Summary model" in run(["model", "show"], extra={"LC_ALL": "ko_KR.UTF-8"}).stdout
    run(["language", "set", "auto"])
    assert "Korean (ko)" in run(["language", "show"], extra={"LC_ALL": "ko_KR.UTF-8"}).stdout
    assert language_settings.stat().st_mode & 0o777 == 0o600
    assert (config / ".language-settings.lock").stat().st_mode & 0o777 == 0o600
    assert not entries(), "language saves must perform no catalog or inference calls"
    saved_language = language_settings.read_bytes()
    for bad_tag in ["-bad", "en\n--config", 'ko";bad', "EN", ""]:
        run(["language", "set", bad_tag], success=False)
        assert language_settings.read_bytes() == saved_language
    listed = run(["model", "list"]).stdout
    assert all(name in listed for name in ["gpt-test-a", "gpt-test-b", "gpt-test-plain"])
    assert "gpt-test-hidden" not in listed and "gpt-test-image" not in listed and "\x1b" not in listed and "\u202e" not in listed
    assert "Catalog A clean" in listed
    rpc = [row for row in entries() if row["kind"] == "rpc"]
    assert [row["method"] for row in rpc] == ["initialize", "initialized", "config/read", "model/list", "model/list"]
    assert all(row["params"]["includeHidden"] is False for row in rpc if row["method"] == "model/list")
    run(["model", "set", "gpt-test-a"])
    assert get(settings) == {"model": "gpt-test-a", "effort": "high"}, "omitted effort must use catalog default"
    assert settings.stat().st_mode & 0o777 == 0o600 and config.stat().st_mode & 0o777 == 0o700
    assert (config / ".model-settings.lock").stat().st_mode & 0o777 == 0o600
    assert not list(config.glob("*.tmp"))
    # UI explicit configuration and installed hook CLI share exactly one profile.
    hook_config = root / "installed-hook-config"
    hook_bin = hook_config / "bin"
    hook_bin.mkdir(parents=True)
    installed_hook = hook_bin / "maeklog-hook"
    shutil.copy2(binary, installed_hook)
    hook_runtime = hook_config / "hook-runtime.json"
    hook_runtime.write_text(json.dumps({"state_dir": str(root / "hook-state"), "plugin_root": str(repo)}))
    run(["model", "set", "gpt-test-b"], extra={"HERDR_PLUGIN_CONFIG_DIR": str(hook_config)})
    run(["language", "set", "ko"], extra={"HERDR_PLUGIN_CONFIG_DIR": str(hook_config)})
    hook_env = {"XDG_CONFIG_HOME": str(root / "isolated-hook-xdg")}
    hook_view = run(["model", "show"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"]).stdout
    assert "gpt-test-b" in hook_view and "xhigh" in hook_view and str(hook_config / "model-settings.json") in hook_view
    hook_language = run(["language", "show"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"]).stdout
    assert "Korean (ko)" in hook_language and str(hook_config / "language-settings.json") in hook_language
    run(["language", "set", "en"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"])
    assert get(hook_config / "language-settings.json") == {"language": "en"}
    assert get(hook_config / "model-settings.json") == {"model": "gpt-test-b", "effort": "xhigh"}
    run(["model", "set", "gpt-test-a"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"])
    assert get(hook_config / "model-settings.json") == {"model": "gpt-test-a", "effort": "high"}
    assert "gpt-test-a" in run(["model", "show"], extra={"HERDR_PLUGIN_CONFIG_DIR": str(hook_config)}).stdout
    hook_saved = (hook_config / "model-settings.json").read_bytes()
    hook_runtime.write_text("{invalid")
    invalid_hook = run(["model", "show"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"], success=False)
    assert "Installed Maeklog runtime is invalid; no configured path was selected" in invalid_hook.stderr
    assert (hook_config / "model-settings.json").read_bytes() == hook_saved
    # Explicit UI environment is authoritative even when an unused runtime file is bad.
    assert "gpt-test-a" in run(["model", "show"], extra={"HERDR_PLUGIN_CONFIG_DIR": str(hook_config)}, executable=installed_hook).stdout
    hook_runtime.unlink()
    missing_runtime = run(["model", "show"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"]).stdout
    assert "gpt-6-luna" in missing_runtime and str(root / "isolated-hook-xdg" / "herdr/plugins/config/maeklog/model-settings.json") in missing_runtime
    missing_language = run(["language", "show"], extra=hook_env, executable=installed_hook, unset=["HERDR_PLUGIN_CONFIG_DIR"]).stdout
    assert "Auto (auto)" in missing_language and str(root / "isolated-hook-xdg" / "herdr/plugins/config/maeklog/language-settings.json") in missing_language
    original = settings.read_bytes()
    for args in [["model", "set", "gpt-test-a", "--effort", "medium"], ["model", "set", "not-returned"],
            ["model", "set", "-bad"], ["model", "set", "gpt-test-a", "--effort", 'x";bad'], ["model", "set", "gpt-test-a", "--effort", "high\n--config"]]:
        run(args, success=False)
        assert settings.read_bytes() == original
    for case in ["protocol-error", "malformed", "wrong-id", "loop", "flood", "stderr", "config", "provider", "timeout"]:
        started = time.monotonic()
        result = run(["model", "set", "gpt-test-b"], extra={"MAEKLOG_TEST_CATALOG_CASE": case}, success=False)
        assert time.monotonic() - started < 11, case
        assert settings.read_bytes() == original and "SECRET_" not in result.stdout + result.stderr
        if case == "timeout":
            pid = int((root / "child.pid").read_text())
            proc = Path(f"/proc/{pid}/stat")
            assert not proc.exists() or proc.read_text().split()[2] == "Z", "catalog descendant is still active"
    assert "No visible text models" in run(["model", "list"], extra={"MAEKLOG_TEST_CATALOG_CASE": "empty"}).stdout
    assert settings.read_bytes() == original
    assert "요약 모델" in run(["model", "show"], extra={"LC_ALL": "ko_KR.UTF-8"}).stdout
    run(["model", "set", "gpt-test-plain"])
    assert get(settings) == {"model": "gpt-test-plain", "effort": None}
    run(["model", "set", "gpt-test-a", "--effort", "high"])
    assert not calls(), "settings/discovery must make no inference calls"
    original = settings.read_bytes()
    saved_language = language_settings.read_bytes()
    language_settings.write_text('{"language":"ko","unexpected":"SECRET_NEVER_LOG"}')
    invalid_language = language_settings.read_bytes()
    result = run(["language", "set", "en"], success=False)
    assert language_settings.read_bytes() == invalid_language and settings.read_bytes() == original
    assert "SECRET_" not in result.stdout + result.stderr
    result = run(["model", "show"], success=False)
    assert "Language settings file is malformed" in result.stderr and "SECRET_" not in result.stderr
    language_settings.write_bytes(saved_language)
    settings.write_text('{"model":"gpt-test-a","effort":"high","unexpected":"SECRET_NEVER_LOG"}')
    result = run(["model", "show"], success=False)
    assert "no default model was substituted" in result.stderr and "SECRET_" not in result.stderr
    settings.write_bytes(original)
    # Hold the scheduler while an explicit live snapshot is created.
    guard = (state / "worker.lock").open("a+")
    fcntl.flock(guard, fcntl.LOCK_EX)
    run(["client", "attach", "101", "thread-a"])
    run(["hook"], {"session_id": "thread-a", "turn_id": "turn-a", "hook_event_name": "UserPromptSubmit", "cwd": str(root), "prompt": "원문 그대로 — preserve original"})
    live("turn/started", turn={"id": "turn-a", "status": "inProgress"})
    live("item/completed", item={"type": "agentMessage", "id": "report-a", "phase": "commentary", "text": "원문 보고 그대로"})
    space = next((state / "spaces").iterdir())
    view_path = space / "live-summary.json"
    gate.write_text("")
    run(["catch-up"])
    initial = get(view_path)
    assert initial["execution"] == {"settings": {"model": "gpt-test-a", "effort": "high"}, "language": "en"}
    # A settings save alone must leave the queued request untouched.
    run(["model", "set", "gpt-test-b", "--effort", "xhigh"])
    assert get(view_path) == initial and not calls()
    # The already queued A runs with frozen A, even though global settings are B.
    fcntl.flock(guard, fcntl.LOCK_UN)
    guard.close(); guard = None
    worker = subprocess.Popen([str(binary), "worker"], cwd=repo, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    until(lambda: len(calls("live")) == 1, "frozen A did not start")
    assert calls("live")[0]["model"] == "gpt-test-a"
    assert 'model_reasoning_effort="high"' in calls("live")[0]["args"]
    assert "English (en)" in calls("live")[0]["prompt"]
    run(["catch-up"])
    replacement = get(view_path)
    assert replacement["execution"]["settings"] == {"model": "gpt-test-b", "effort": "xhigh"}
    assert replacement["request_id"] != initial["request_id"]
    # Force identical unix seconds; the unique ID/profile must still reject stale A.
    replacement["requested_at"] = initial["requested_at"]
    view_path.write_text(json.dumps(replacement))
    gate.unlink()
    until(lambda: len(calls("live")) == 2 and get(view_path)["status"] == "ready", "queued replacement B did not finish")
    assert get(view_path)["execution"] == replacement["execution"]
    assert get(view_path)["request_id"] == replacement["request_id"]
    assert get(view_path)["result"]["current"]["text"].startswith("gpt-test-b")
    assert calls("live")[1]["model"] == "gpt-test-b" and 'model_reasoning_effort="xhigh"' in calls("live")[1]["args"]
    count = len(calls())
    assert "no additional model call" in run(["catch-up"]).stdout
    time.sleep(0.1)
    assert len(calls()) == count
    # Same snapshot but a new effort must not hit B/xhigh's ready cache.
    ready_before_save = view_path.read_bytes()
    run(["model", "set", "gpt-test-b", "--effort", "high"])
    assert view_path.read_bytes() == ready_before_save and len(calls()) == count
    run(["catch-up"])
    until(lambda: len(calls("live")) == 3 and get(view_path)["status"] == "ready", "effort-specific request was incorrectly cached")
    assert 'model_reasoning_effort="high"' in calls("live")[2]["args"]
    # Explicit Korean overrides this English caller. Hold the space lease so the
    # queued request must retain Korean even after an English save before invocation.
    guard = (space / ".worker.lock").open("a+")
    def claim_language_lease():
        try:
            fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except BlockingIOError:
            return False
    until(claim_language_lease, "could not hold the language-freeze fixture lease", timeout=2)
    model_before_language = settings.read_bytes()
    run(["language", "set", "ko"])
    assert settings.read_bytes() == model_before_language and len(calls("live")) == 3
    run(["catch-up"], extra={"LC_ALL": "C.UTF-8"})
    frozen_language_request = get(view_path)
    assert frozen_language_request["execution"]["language"] == "ko"
    run(["language", "set", "en"])
    assert get(view_path) == frozen_language_request and settings.read_bytes() == model_before_language
    assert len(calls("live")) == 3, "language save must not launch or replace the queued request"
    fcntl.flock(guard, fcntl.LOCK_UN)
    guard.close(); guard = None
    until(lambda: len(calls("live")) == 4 and get(view_path)["status"] == "ready", "language-specific request was incorrectly cached")
    assert get(view_path)["execution"]["language"] == "ko" and "Korean (ko)" in calls("live")[3]["prompt"]
    assert "원문 그대로 — preserve original" in calls("live")[3]["prompt"]
    # Inference failure retains B, never substitutes the old Luna default or retries automatically.
    fail_file.write_text("")
    run(["catch-up"])
    until(lambda: len(calls("live")) == 5 and get(view_path)["status"] == "failed", "live model failure was not reported")
    assert calls("live")[-1]["model"] == "gpt-test-b"
    time.sleep(0.15)
    assert len(calls("live")) == 5 and get(settings) == {"model": "gpt-test-b", "effort": "high"}
    fail_file.unlink()
    # Daily uses fresh B/high/Korean at invocation, overriding this English worker.
    run(["language", "set", "ko"])
    run(["hook"], {"session_id": "thread-a", "turn_id": "turn-a", "hook_event_name": "Stop", "cwd": str(root), "last_assistant_message": "원문 완료 그대로"})
    until(lambda: len(calls("daily")) == 1, "daily summary did not start")
    daily = calls("daily")[0]
    assert daily["model"] == "gpt-test-b" and 'model_reasoning_effort="high"' in daily["args"]
    assert "Korean (ko)" in daily["prompt"] and "원문 완료 그대로" in daily["prompt"]
    until(lambda: worker.poll() is not None, "worker did not finish", timeout=15)
    assert worker.returncode == 0
    before_auto_save = len(calls())
    run(["language", "set", "auto"])
    assert get(language_settings) == {"language": "auto"} and len(calls()) == before_auto_save
    assert all('--ignore-user-config' in row["args"] and 'service_tier="default"' in row["args"] and 'features.fast_mode=false' in row["args"] and 'features.hooks=false' in row["args"] and '--ephemeral' in row["args"] and row["args"][row["args"].index('--sandbox')+1]=='read-only' for row in calls())
    result = {"passed": True, "inference_calls": {"live": len(calls("live")), "daily": len(calls("daily"))}, "catalog_protocol": "initialize/initialized/config-read/model-list paginated", "cases": ["defaults", "catalog defaults", "no effort", "private atomic save", "UI and installed hook settings coherence", "explicit language overrides", "language hook path coherence", "language malformed-file preservation", "failure preservation", "bounded timeout/output/descendant cleanup", "same-second stale commit", "frozen queued execution", "cache profile identity", "queued language freeze", "fresh daily language", "no inference on save", "failure no fallback", "daily settings"]}
    if len(sys.argv) > 2:
        destination = Path(sys.argv[2]).resolve()
        destination.mkdir(parents=True, exist_ok=True)
        (destination / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        (destination / "trace.jsonl").write_bytes(trace.read_bytes())
    print(json.dumps(result))
except BaseException as error:
    if len(sys.argv) > 2:
        destination = Path(sys.argv[2]).resolve()
        destination.mkdir(parents=True, exist_ok=True)
        (destination / "failure.json").write_text(json.dumps({"passed": False, "error": str(error)}, ensure_ascii=False, indent=2) + "\n")
        if trace.exists():
            (destination / "trace.jsonl").write_bytes(trace.read_bytes())
    raise
finally:
    if guard:
        guard.close()
    if worker and worker.poll() is None:
        os.killpg(worker.pid, signal.SIGTERM)
        try:
            worker.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(worker.pid, signal.SIGKILL)
            worker.wait()
    remove_fixture_after_worker_drain()
