# Codex terminal client → Herdr → Maeklog

Maeklog uses a local Codex TUI patch to identify the terminal/tab that owns a conversation and to display public progress. The current patch is based on `openai/codex` tag `rust-v0.163.0`, commit `83086188ae0614bd882818ff1aa23f3194962012`.

Start with the [English installation guide](../../docs/setup.md) for plugin installation, the four collection hooks, manual `/hooks` review and trust, preflight, the checked Codex build, and a first conversation. A new installation uses Maeklog's own namespace. It does not discover or import another plugin's configuration or records.

## What the patch does

The official 0.163.0 hook schema does not provide the terminal client's PID/pane or an equivalent per-turn origin. Its optional `client` field names the client application rather than the submitting terminal.

The optional TUI adapter calls the installed `maeklog-hook client attach CLIENT_PID THREAD_ID` on thread activation and before a user turn. Clearing or switching away from a thread and clean shutdown call `client detach`. Maeklog verifies the foreground Codex PID through Herdr and uses the existing `pane.report_agent_session` API. No custom Herdr build is required.

The registry records the Herdr socket, session, terminal identity and PID. Hooks resolve it against live Herdr state, instead of using the daemon's inherited pane. Screen-based agent detection may lag behind the callback, so terminal identity and the actual foreground PID/process name establish ownership.

The adapter also projects public progress into a bounded local cache: completed commentary, plans and tool lifecycle metadata. Private reasoning, tool output bodies and final answers are excluded from this progress cache. The adapter is inactive outside Herdr, does not run in `codex exec` or the daemon, and gives each call a two-second timeout so a reporting failure cannot stop Codex. See the [wire contract](live-progress.md) and [progress behavior](../../docs/live-progress.md).

The default helper is `$XDG_CONFIG_HOME/herdr/plugins/config/maeklog/bin/maeklog-hook`, or `~/.config/herdr/plugins/config/maeklog/bin/maeklog-hook` without XDG configuration. `MAEKLOG_CLIENT_BIN` is a diagnostic override. The supported installation flow checks the default helper location; a separate `HERDR_PLUGIN_CONFIG_DIR` outside that location is rejected by preflight. Use the same `XDG_CONFIG_HOME` and `CODEX_HOME` for hook installation, trust review, updates and Codex sessions.

Current dialogue issue/PR references use the official `UserPromptSubmit` `additionalContext` support. The helper gives the primary turn a source-bound local report command. Host hooks and progress callbacks collect its bounded temporary queue. See the [task report contract](task-reports.md).

## Installation and preflight

After the Herdr plugin is built, register its hooks:

```sh
herdr plugin action invoke install-codex-hooks --plugin maeklog
```

In Codex, enable hooks if needed and use `/hooks` to review and trust the Maeklog handlers for **SessionStart, UserPromptSubmit, Stop and Interrupt**. The installer does not write trust on the user's behalf.

Use the plugin root obtained in the installation guide:

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" --preflight
python3 "$maeklog_root/scripts/update-codex-maeklog.py" 0.163.0 --apply
```

`--preflight` checks required tools, the Herdr caller, installed Maeklog runtime/helper, hooks feature state, all four handler definitions, and their current manual enabled/trusted state. It performs no downloads, builds or model calls. Python 3.11+, Git, `gh`, Rust/rustup, `just`, `cargo-nextest`, Node/npm, Herdr, and a usable `bazel` command are required. The Codex 0.163.0 source build requires Bazel 9.2.0; use that version directly or let Bazelisk select it from the source's `.bazelversion`.

Without `--apply`, a version request still downloads, builds and runs validation, then reports an installation plan. `--check` reports versions and bridge/installation status; it is not a trust check.

The updater downloads official source and matching helper/resource packages, applies the applicable bridge patch without guessing a merge, and builds using a persistent Cargo cache. Before activation it runs focused bridge/TUI lint, installer, launcher and Maeklog routing checks, plus a real Herdr lifecycle test against a localhost response fixture. The lifecycle test creates and closes one temporary sibling pane. It calls no external model. A preparation or validation failure leaves the active installation in place.

The full installer supports a new Maeklog namespace and checked existing installations. It preserves unrelated Codex config/hook entries and backs up files it replaces. If an exact older Herdr server-side SessionStart reporter is present, only its enabled state is disabled so it cannot compete with the terminal-client reporter. The package contains the patched CLI and the matching official helpers/resources; the official npm package is kept intact.

`~/.local/bin/codex` points to the stable Python launcher at `~/.local/share/maeklog/codex-launcher.py`. `codex-maeklog` points to the native package's `bin/codex`; `maeklog-update` points to the updater in the plugin checkout. Keep that checkout available and ensure `~/.local/bin` is on PATH. Configuration follows XDG and Codex home overrides as described above; package/validation files default to `~/.local/share/maeklog` and the build cache to `~/.cache/maeklog/codex-update/target`.

Existing CLI processes keep running their original code until they are normally exited and resumed. Do not kill active work or restart the shared daemon as part of an update. Once strict client routing is enabled, unregistered old clients cannot collect via the daemon's inherited tab environment.

## Routine updates

The updater uses scripts and patches from the retained Maeklog checkout; it does not fetch Maeklog source. Set `maeklog_root` to that actual checkout and check for local changes:

```sh
git -C "$maeklog_root" status --short
```

Continue only if the output is empty; save any local changes before proceeding. In a regular Herdr shell, fetch `main` and select the fetched commit. This also works for a detached, single-branch v0.14.0 checkout. The switch preserves branch pointers and refuses to overwrite ignored files:

```sh
git -C "$maeklog_root" fetch origin main &&
  git -C "$maeklog_root" switch --detach --no-overwrite-ignore FETCH_HEAD &&
  codex update 0.163.0 --check &&
  codex update 0.163.0
codex update --help
codex --version
```

`codex update` selects the installed `maeklog-update --apply` workflow. An exact first argument of `update` selects the updater; ordinary arguments, including prompt text containing that word, `resume`, `exec` and `--version`, are forwarded to the native binary with arguments and PID preserved.

The default target is the latest official stable release. Finding that release does not establish Maeklog compatibility. `--check` reports bridge/installation status, identifies an older-version fallback, and distinguishes an already checked installation from a target that still needs validation. A failed source compatibility check reports that a bridge port is required and leaves the installed CLI in place; retrying the same source and patch cannot fix the conflict.

An already matching installation is checked against saved executable hashes and the current bridge patch SHA-256 before avoiding a rebuild. The launcher's files, links and source hash are checked separately. A changed bridge at the same upstream version requires a new checked local package. Concurrent updates are rejected.

Build logs, source and localhost fixture evidence remain under `~/.local/share/maeklog/validation`. `--target-dir` and `--jobs` control build resources. A new upstream release that conflicts with the patch requires a manual port; applying cleanly does not replace lifecycle validation. Routine updates run focused integration checks rather than the entire upstream TUI suite.

An npm update may replace or shadow the main `codex` launcher with an official unpatched executable. For TUI sessions use the stable launcher at `~/.local/bin/codex`. `codex-maeklog` is the checked native package pointer and can be used for `--version` diagnostics; it is not a supported direct TUI fallback. If PATH resolves another command, invoke `~/.local/bin/codex` explicitly. If that launcher entry itself was overwritten, run the same installed script with `python3 "$HOME/.local/share/maeklog/codex-launcher.py"` (append `resume` to resume). Use `maeklog-update --apply` from Herdr to prepare and check a supported release. A missing updater is an error; the launcher does not silently select a different updater. Keep `client-routing.json` and the old server-side SessionStart reporter's disabled state intact.

The advanced `--launcher-only` installer mode is for an existing checked Maeklog package with direct launcher links. It verifies saved package hashes and links and can install only the stable launcher without rebuilding:

```sh
python3 "$maeklog_root/scripts/install-client-routing.py" --launcher-only
python3 "$maeklog_root/scripts/install-client-routing.py" --launcher-only --apply
```

## Manual port and focused checks

The current source patch is [codex-0.163.0-maeklog.patch](codex-0.163.0-maeklog.patch). Earlier 0.157.1, 0.159.2, 0.159.3, 0.160.0 and 0.162.1 patches are historical source references, not promises of current installation support.

For a manual port in a separate checkout:

```sh
git clone --depth 1 --branch rust-v0.163.0 https://github.com/openai/codex.git codex-maeklog
cd codex-maeklog
git apply "$maeklog_root/integrations/codex/codex-0.163.0-maeklog.patch"
cd codex-rs
cargo fetch
just fmt
CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo build --locked -p codex-cli --bin codex
env -u NO_COLOR CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 just test --locked -p codex-tui
CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 just fix -p codex-tui
just fmt
CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_BUILD_JOBS=4 cargo rustc --locked --release -p codex-cli --bin codex -- -C lto=off
```

The published tag has workspace version 0.163.0 while workspace package versions in `Cargo.lock` can still say `0.0.0`. `cargo fetch` normalizes these; the updater checks that external dependency versions and the graph remain unchanged. Synchronize Bazel metadata with `just bazel-lock-update` when the Cargo lock changes. Build the matching debug CLI before running the full TUI suite, which invokes a real CLI in some tests.

The repository's mock-home checks are:

```sh
python3 scripts/test-install-client-routing.py
python3 scripts/test-codex-launcher.py
python3 scripts/test-update-codex-maeklog.py
```

The optional native lifecycle test requires the installed manually trusted Maeklog helper. Its `--pm-binary` option only accepts that same installed helper, not an arbitrary candidate adapter. The test uses isolated Codex/Maeklog directories and a local response fixture, closes only its own pane/server, and retains its private evidence directory:

```sh
python3 scripts/test-codex-client-live.py \
  --binary /path/to/checked/patched/bin/codex \
  --server-binary /path/to/official/bin/codex \
  --pane YOUR_CALLING_PANE_ID \
  --root /path/to/new/private/evidence-directory \
  --default-bridge
```

The [0.163.0 verification record](../../docs/verification-codex-0.163.0-2026-10-16.md) covers the current Linux source port, native checks and installed update command. The [dated 0.162.1 record](../../docs/verification-codex-0.162.1-2026-10-12.md) preserves anonymized historical results from the original implementation; it does not by itself verify a renamed Maeklog build or a new installation.

## Guarantees and limits

- Different sessions in the same repository remain separate by terminal/tab. New requests follow a moved pane's live tab; an already collected turn retains its source tab for its final response.
- A stale detach for one thread cannot remove a newer attachment. Closed or replaced processes cannot reuse their old registration. Reattachment refreshes live Herdr state and provenance before new user turns.
- A conversation open in several terminals is ambiguous for a new request because the hook schema lacks a submitting-client ID. Maeklog rejects such input with a diagnostic rather than guessing. An already bound turn retains its source.
- SessionStart can arrive before the TUI obtains a thread ID. Unmatched SessionStart is ignored; collection begins with a registered user request.
- Uncollected transcripts are not backfilled. Existing records, opt-out settings and user corrections are not rewritten by the integration.
- `client-routing.json` stores identity and turn-origin metadata, not prompt or answer bodies. It uses private permissions like other Maeklog state.
- The patch fixes Maeklog collection and Herdr session registration. Codex tool processes may still inherit daemon environment variables; general shell-tool caller context is outside this adapter's scope.

The upstream Codex source and bundled resources retain their own [license](LICENSE-CODEX) and [notice](NOTICE-CODEX). See the [release readiness record](../../docs/release-readiness.md) for the current installation validation scope.

This is a local patch rather than an upstream release. Port and rerun lifecycle checks for new upstream versions. Retire the patch when upstream provides a verified client-origin session/turn reporting contract.
