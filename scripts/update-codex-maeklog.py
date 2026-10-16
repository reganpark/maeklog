#!/usr/bin/env python3
"""Prepare or install a checked Codex client update for Maeklog.

Use --check for release information, or --apply to build, validate and install.
All build and live-test evidence stays in a private directory. No external model
is called by the checks. Existing CLI and daemon processes keep running.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from codex_hook_trust import approved_hooks, installation_paths


ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r"\d+\.\d+\.\d+")
launcher_spec = importlib.util.spec_from_file_location(
    "maeklog_codex_launcher", ROOT / "scripts/codex_launcher.py")
launcher = importlib.util.module_from_spec(launcher_spec)
launcher_spec.loader.exec_module(launcher)


def output(command, cwd=None):
    return subprocess.check_output([str(p) for p in command], cwd=cwd, text=True).strip()


def latest_version():
    release = json.loads(output(["gh", "api", "repos/openai/codex/releases/latest"]))
    tag = release["tag_name"]
    version = tag.removeprefix("rust-v")
    if release.get("prerelease") or release.get("draft") or not VERSION.fullmatch(version):
        raise RuntimeError("Expected an official stable Codex release")
    return version


def caller_pane(explicit=None):
    if os.environ.get("HERDR_ENV") != "1":
        raise RuntimeError("Run the update inside Herdr; --check also works outside Herdr")
    if explicit:
        pane = json.loads(output(["herdr", "pane", "get", explicit]))["result"]["pane"]
    elif thread := os.environ.get("CODEX_THREAD_ID"):
        # Tool processes can inherit a different pane from the shared daemon.
        agents = json.loads(output(["herdr", "agent", "list"]))["result"]["agents"]
        matches = [a for a in agents if a.get("agent_session", {}).get("value") == thread]
        if len(matches) != 1:
            raise RuntimeError("Cannot identify the calling Codex pane; supply --pane explicitly")
        pane = matches[0]
    else:
        pane = json.loads(output(["herdr", "pane", "current", "--current"]))["result"]["pane"]
    return pane["pane_id"]


def bridge_patch(version):
    patches = list((ROOT / "integrations/codex").glob("codex-*-maeklog.patch"))
    def patch_version(path):
        name = path.name.removeprefix("codex-").removesuffix("-maeklog.patch")
        return tuple(map(int, name.split(".")))
    eligible = [p for p in patches if patch_version(p) <= tuple(map(int, version.split(".")))]
    if not eligible:
        raise RuntimeError("No client bridge patch is available for this version")
    return max(eligible, key=patch_version)


def describe_bridge(version):
    try:
        patch = bridge_patch(version)
    except RuntimeError:
        print("Maeklog integration: no bridge patch available; this release cannot be installed.")
        return
    if installation_current(version):
        print("Maeklog integration: this exact checked installation is already active.")
        return
    patch_version = patch.name.removeprefix("codex-").removesuffix("-maeklog.patch")
    print(f"Bridge patch: {patch.name}" + (" (older-version fallback)" if patch_version != version else ""))
    print("Maeklog compatibility is not verified by this release check. "
          "Source, build and lifecycle checks must pass before installation.")


def check_bridge(version, patch, evidence, source):
    try:
        step("patch-check: check client bridge compatibility",
             ["git", "apply", "--check", patch], evidence, source)
    except RuntimeError as error:
        raise RuntimeError(
            f"Maeklog bridge {patch.name} did not pass compatibility checks for Codex {version}. "
            "A bridge port is required; retrying unchanged will not repair this. "
            f"The installed CLI has not been changed. {error}") from error


def installation_current(version):
    try:
        home = Path.home()
        installed = launcher.paths(home)
        binary = installed["binary_link"].resolve(strict=True)
        manifest = json.loads((binary.parent.parent / "maeklog-install.json").read_text())
        hashes = manifest["sha256"]
        patch_hash = hashlib.sha256(bridge_patch(version).read_bytes()).hexdigest()
        # Older checked packages used two direct links. Keep recognizing those
        # until launcher-only migration; never mistake a wrapper for the CLI.
        entry_points_current = (launcher.verify(manifest, binary, home) if "launcher" in manifest
                                else installed["codex"].is_symlink()
                                and installed["binary_link"].is_symlink()
                                and installed["codex"].resolve(strict=True) == binary)
        return (entry_points_current and manifest["codex_version"] == version and str(binary) in hashes
                and manifest.get("bridge_patch_sha256") == patch_hash
                and all(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected
                        for path, expected in hashes.items()))
    except (OSError, KeyError, ValueError, RuntimeError, TypeError, AttributeError):
        return False


def prepare_bazel(source, env):
    version = (source / ".bazelversion").read_text().strip()
    if binary := shutil.which("bazel"):
        if output([binary, "--version"], source) != f"bazel {version}":
            raise RuntimeError(f"This release needs Bazel {version}")
        return
    for binary in (Path.home() / ".cache/bazelisk/downloads/sha256").glob("*/bin/bazel"):
        if output([binary, "--version"]) == f"bazel {version}":
            env["PATH"] = str(binary.parent) + os.pathsep + env.get("PATH", os.defpath)
            return
    raise RuntimeError(f"Install Bazelisk or Bazel {version} before preparing this release")


def check_lock_normalization(source):
    original = tomllib.loads(output(["git", "show", "HEAD:codex-rs/Cargo.lock"], source))
    normalized = tomllib.loads((source / "codex-rs/Cargo.lock").read_text())
    # Release tags sometimes retain 0.0.0 versions for local workspace packages.
    # cargo fetch may normalize those, but must not upgrade external dependencies.
    def graph(lock):
        return [{k: v for k, v in package.items()
                 if k != "version" or "source" in package} for package in lock["package"]]
    if graph(original) != graph(normalized):
        raise RuntimeError("Rust dependency graph changed beyond workspace version normalization")


def step(label, command, evidence, cwd=None, env=None):
    print(label, flush=True)
    log = evidence / (label.split(":", 1)[0] + ".log")
    with log.open("w") as stream:
        process = subprocess.Popen([str(p) for p in command], cwd=cwd, env=env,
                                   stdin=subprocess.DEVNULL, stdout=stream,
                                   stderr=subprocess.STDOUT)
        while True:
            try:
                code = process.wait(timeout=30)
                break
            except subprocess.TimeoutExpired:
                print(f"  Working… {log}", flush=True)
    if code:
        raise RuntimeError(f"{label} failed ({code}); inspect {log}")


def matching_resources(prefix, version):
    matches = [p.parent for p in (prefix / "node_modules/@openai").rglob("codex-package.json")
               if json.loads(p.read_text()).get("version") == version]
    if len(matches) != 1:
        raise RuntimeError("Expected one matching official Codex resource package")
    return matches[0]


def install_verified(candidate, resources, pm, version, commit, evidence, apply):
    revision = 1
    packages = Path.home() / ".local/share/maeklog"
    while ((packages / f"codex-{version}-maeklog.{revision}").exists()
           or (packages / f"codex-{version}-maeklog.{revision}").is_symlink()):
        revision += 1
    command = [sys.executable, ROOT / "scripts/install-client-routing.py",
               "--codex-binary", candidate / "bin/codex", "--pm-binary", pm,
               "--resource-package", resources, "--codex-version", version,
               "--base-commit", commit, "--revision", str(revision),
               "--bridge-patch-sha256", hashlib.sha256(bridge_patch(version).read_bytes()).hexdigest()]
    step("install-preflight: check the existing installation", command, evidence)
    if apply:
        step("install: back up and replace the launchers", [*command, "--apply"], evidence)
        for name in ["codex", "codex-maeklog"]:
            binary = Path.home() / ".local/bin" / name
            if output([binary, "--version"]) != f"codex-cli {version}":
                raise RuntimeError(f"Installed {name} version did not match")
        binary = (Path.home() / ".local/bin/codex-maeklog").resolve(strict=True)
        if launcher.sha256(binary) != launcher.sha256(candidate / "bin/codex"):
            raise RuntimeError("Installed codex-maeklog differs from the checked binary")
        manifest = json.loads((binary.parent.parent / "maeklog-install.json").read_text())
        if not launcher.verify(manifest, binary, Path.home()) or not installation_current(version):
            raise RuntimeError("Installed launcher or package differs from the checked installation")


def update(version, pane, evidence, target, jobs, apply):
    patch = bridge_patch(version)
    source = evidence / "source"
    step("clone: fetch the official release source",
         ["git", "clone", "--depth", "1", "--branch", f"rust-v{version}",
          "https://github.com/openai/codex.git", source], evidence)
    commit = output(["git", "rev-parse", "HEAD"], source)
    # Fail on conflicts. Never use git apply --3way or edit the upstream source.
    check_bridge(version, patch, evidence, source)
    step("patch: apply the client bridge", ["git", "apply", patch], evidence, source)
    env = {**os.environ, "CARGO_TARGET_DIR": str(target), "CARGO_BUILD_JOBS": str(jobs),
           "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_RELEASE_DEBUG": "0"}
    env.pop("NO_COLOR", None)
    rust = source / "codex-rs"
    prepare_bazel(source, env)
    step("fetch: prepare Rust dependencies", ["cargo", "fetch"], evidence, rust, env)
    check_lock_normalization(source)
    if output(["git", "diff", "--", "codex-rs/Cargo.lock"], source):
        step("bazel-lock: synchronize Bazel dependency metadata",
             ["just", "bazel-lock-update"], evidence, source, env)
    step("fmt: format the source", ["just", "fmt"], evidence, rust, env)
    step("bridge-test: check attach and detach",
         ["just", "test", "--locked", "-p", "codex-tui", "herdr_bridge"], evidence, rust, env)
    step("lint: check the TUI", ["just", "clippy", "--locked", "-p", "codex-tui"], evidence, rust, env)
    step("build: compile the updated CLI",
         ["cargo", "rustc", "--locked", "--release", "-p", "codex-cli", "--bin", "codex",
          "--", "-C", "lto=off"], evidence, rust, env)
    prefix = evidence / "official"
    step("resources: download matching official helpers",
         ["npm", "install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix", prefix,
          f"@openai/codex@{version}"], evidence)
    resources = matching_resources(prefix, version)
    candidate = evidence / "candidate"
    shutil.copytree(resources, candidate)
    shutil.copy2(target / "release/codex", candidate / "bin/codex")
    if output([candidate / "bin/codex", "--version"]) != f"codex-cli {version}":
        raise RuntimeError("Built CLI version did not match the release")
    plugin, _ = installation_paths()
    pm = plugin / "bin/maeklog-hook"
    step("installer-test: check backups and package validation",
         [sys.executable, ROOT / "scripts/test-install-client-routing.py"], evidence)
    step("launcher-test: check update dispatch and process identity",
         [sys.executable, ROOT / "scripts/test-codex-launcher.py"], evidence)
    step("routing-test: check terminal and tab attribution",
         ["node", ROOT / "scripts/test-client-routing.mjs", pm], evidence, ROOT)
    step("live-test: check startup, hooks, new conversation, exit and resume",
         [sys.executable, ROOT / "scripts/test-codex-client-live.py",
          "--binary", candidate / "bin/codex", "--server-binary", resources / "bin/codex",
          "--pane", pane, "--root", evidence / "live", "--pm-binary", pm,
          "--default-bridge"], evidence, ROOT)
    verification = {"codex_version": version, "base_commit": commit, "patch": str(patch),
                    "patch_sha256": hashlib.sha256(patch.read_bytes()).hexdigest(),
                    "binary_sha256": hashlib.sha256((candidate / "bin/codex").read_bytes()).hexdigest(),
                    "applied": False, "tests": ["bridge", "lint", "installer", "launcher", "routing", "live"]}
    (evidence / "verification.json").write_text(json.dumps(verification, indent=2) + "\n")
    install_verified(candidate, resources, pm, version, commit, evidence, apply)
    verification["applied"] = apply
    (evidence / "verification.json").write_text(json.dumps(verification, indent=2) + "\n")
    print(f"{'Installed' if apply else 'Prepared'} Codex {version}. Evidence: {evidence}", flush=True)
    if apply:
        print("Exit existing Codex sessions normally, then use codex resume in the same terminal.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", nargs="?", default="latest")
    parser.add_argument("--check", action="store_true",
                        help="Show release, installed version and bridge status without building")
    parser.add_argument("--preflight", action="store_true",
                        help="Check local prerequisites and current manual hook approvals; no downloads or build")
    parser.add_argument("--apply", action="store_true", help="Install after all checks pass")
    parser.add_argument("--pane", help="Explicit calling Herdr pane for the temporary live test")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--target-dir", type=Path,
                        default=Path.home() / ".cache/maeklog/codex-update/target")
    args = parser.parse_args()
    if sum([args.check, args.apply, args.preflight]) > 1:
        parser.error("Choose --check, --preflight or --apply")
    if args.jobs < 1 or (args.version != "latest" and not VERSION.fullmatch(args.version)):
        parser.error("Expected a stable version and positive build job count")
    if not args.check:
        # Validate trust before even looking up a release or downloading source.
        # This only compares user-approved definitions; it never writes hashes.
        pm, _ = approved_hooks()
        if not output([pm, "--version"]).startswith("maeklog "):
            raise RuntimeError("The installed hook is not a Maeklog binary; reinstall Maeklog hooks before continuing")
        for command in ["git", "gh", "cargo", "just", "npm", "node", "herdr", "codex"]:
            if not shutil.which(command):
                raise RuntimeError(f"Required command not installed: {command}")
        nextest = shutil.which("cargo-nextest")
        if not nextest:
            raise RuntimeError("Install cargo-nextest before continuing; the native client checks use just test")
        try:
            # Calling the executable directly keeps rustup from downloading a
            # missing repository toolchain during this local-only preflight.
            output([nextest, "nextest", "--version"])
        except (OSError, subprocess.CalledProcessError):
            raise RuntimeError("cargo-nextest is not runnable; install it before continuing") from None
        if not shutil.which("bazel") and not any(
                (Path.home() / ".cache/bazelisk/downloads/sha256").glob("*/bin/bazel")):
            raise RuntimeError("Install Bazel or put Bazelisk's bazel shim on PATH before continuing")
        pane = caller_pane(args.pane)
        if args.preflight:
            print("Local prerequisites and all four current Maeklog hook approvals passed. No build or installation performed.")
            return
    version = latest_version() if args.version == "latest" else args.version
    binary = Path.home() / ".local/bin/codex-maeklog"
    current = output([binary, "--version"]) if binary.exists() else "not installed"
    print(f"Installed integration: {current}; target: {version}", flush=True)
    if args.check:
        describe_bridge(version)
        return
    if installation_current(version):
        print("The checked integration is already installed; no build needed.")
        return
    base = Path.home() / ".local/share/maeklog/validation"
    base.mkdir(parents=True, exist_ok=True)
    target = args.target_dir.expanduser().resolve()
    target.mkdir(parents=True, exist_ok=True)
    lock_path = Path.home() / ".cache/maeklog/codex-update/update.lock"
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with lock_path.open("a") as lock:
        try:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError("Another Codex integration update is already running") from None
        evidence = Path(tempfile.mkdtemp(prefix=f"codex-{version}-update-", dir=base))
        update(version, pane, evidence, target, args.jobs, args.apply)
    print(f"Elapsed: {time.monotonic() - started:.0f} seconds")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, OSError) as error:
        print(f"Update stopped: {error}", file=sys.stderr)
        sys.exit(1)
