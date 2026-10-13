#!/usr/bin/env python3
"""Stable entry point for the checked Maeklog Codex client."""
import os
from pathlib import Path
import re
import sys


UPDATE_HELP = """usage: codex update [VERSION] [--check | --preflight | --apply] [UPDATER_OPTIONS]

Build, validate and install the Maeklog Codex integration using maeklog-update.
VERSION defaults to the latest official stable release. Installation happens
only after the updater's bridge, lint, installer, routing and live checks pass.

  --check          Show release and installed versions without building
  --preflight      Check local tools and current manual hook approvals only
  --apply          Install after verification (the default for codex update)
  --pane PANE      Select the calling Herdr pane for the temporary live test
  --jobs N         Build job count (default: 4)
  --target-dir DIR Reuse this Rust build directory
  -h, --help       Show this help without running the updater

Use codex --version for the installed client version. Options for this command
are the Maeklog updater's options, not an upstream Codex update interface.
Only a first argument of update is intercepted; codex -- update sends a prompt.
"""


def companion_paths():
    launcher = Path(__file__).resolve()
    # This file is installed at PREFIX/share/maeklog/codex-launcher.py.
    # Resolve from that location rather than HOME or the shared daemon's PATH.
    prefix = launcher.parents[2]
    return launcher, prefix / "bin/codex-maeklog", prefix / "bin/maeklog-update"


def checked_binary(launcher, pointer):
    if not pointer.is_symlink():
        raise RuntimeError(f"The checked Codex package pointer is missing: {pointer}")
    try:
        binary = pointer.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        raise RuntimeError(f"The checked Codex package pointer is invalid: {pointer}: {error}") from None
    package = binary.parent.parent
    if (binary == launcher or binary.name != "codex" or binary.parent.name != "bin"
            or package.parent != launcher.parent
            or not re.fullmatch(r"codex-\d+\.\d+\.\d+-maeklog\.[1-9]\d*", package.name)
            or not binary.is_file() or not os.access(binary, os.X_OK)):
        raise RuntimeError(f"Invalid checked Codex package pointer: {pointer}")
    return binary


def main():
    args = sys.argv[1:]
    launcher, pointer, updater = companion_paths()
    if args and args[0] == "update":
        update_args = args[1:]
        option_args = update_args[:update_args.index("--")] if "--" in update_args else update_args
        if "--help" in option_args or "-h" in option_args:
            print(UPDATE_HELP, end="")
            return
        if "--version" in option_args:
            print("codex update has no --version option; use codex --version for the installed "
                  "client, or codex update --check to check a release.", file=sys.stderr)
            sys.exit(2)
        try:
            resolved_updater = updater.resolve(strict=True)
        except OSError:
            raise RuntimeError(f"Maeklog updater is missing: {updater}") from None
        if (resolved_updater == launcher or resolved_updater == pointer.resolve()
                or not resolved_updater.is_file() or not os.access(resolved_updater, os.X_OK)):
            raise RuntimeError(f"Maeklog updater is not executable: {updater}")
        if not any(option in option_args for option in ["--check", "--preflight", "--apply"]):
            update_args.insert(0, "--apply")
        os.execv(str(updater), [str(updater), *update_args])
    binary = checked_binary(launcher, pointer)
    # Use the actual package executable, preserving Codex's process name and PID.
    os.execv(str(binary), ["codex", *args])


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError) as error:
        print(f"Codex launcher stopped: {error}", file=sys.stderr)
        sys.exit(1)
