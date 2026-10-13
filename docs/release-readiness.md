# Public release readiness — 2026-10-13

Maeklog is prepared as a source-based Herdr plugin at
[reganpark/maeklog](https://github.com/reganpark/maeklog). Start with the
[English installation guide](setup.md). Linux verification covers a fresh clone
of the public repository, new source builds and isolated installation. macOS remains experimental and needs
real-hardware testing; see the [public preview notes](releases/v0.14.0.md).

## Supported installation contract

| Part | Contract |
| --- | --- |
| Herdr | 0.8.2 or later, as declared by `herdr-plugin.toml` |
| Maeklog | 0.14.0; built from the pinned lockfile and Rust 1.97.1 toolchain |
| Python | 3.11 or later; installer tools use standard-library `tomllib` and Unix file locks |
| Codex client | 0.162.1 is the checked native integration target |
| Linux | Native installation and lifecycle verified on Linux x86_64 |
| macOS | Experimental; declared in the plugin manifest, but native execution and a clean-machine installation have not been verified |

The plugin installation builds Maeklog. Codex collection additionally requires
explicit installation of the four Maeklog hooks, manual review with Codex `/hooks`,
and the checked Codex client. The client updater builds from official upstream
source and downloads matching official helpers. It needs Git, GitHub CLI, Rust,
`just`, `cargo-nextest`, Node.js/npm, and Bazel 9.2.0 or Bazelisk's `bazel` shim. Compiler
and platform libraries must satisfy the selected upstream Codex release. This is
a substantial source build; no prebuilt Maeklog client download is provided.

The updater's `--preflight` checks installed local tools and compares all four
enabled hook definitions with their current native Codex approval fingerprints.
It performs no download, build or model request and never writes an approval.
`--check` reports version information. A version command without `--apply` builds
and validates a candidate but leaves the active launchers unchanged. `--apply`
activates the candidate only after those checks pass.

Initial installation allows an absent `~/.local/bin/codex` and no generated
Herdr SessionStart handler. If one unambiguous generated handler exists, its
enablement is disabled while its existing trust hash and unrelated configuration
are preserved. Multiple or ambiguous handlers stop installation for review.
Replaced files and pointers are backed up before activation; a failed activation
restores them. The updater does not restart existing Codex processes.

Use the stable `codex` launcher for terminal sessions. `codex-maeklog` is the
checked package pointer used for verification; direct terminal execution can
change the process name and fail foreground ownership checks on Linux. If PATH
selects another Codex, invoke `~/.local/bin/codex` explicitly. If that entry point
has itself been overwritten, invoke
`python3 ~/.local/share/maeklog/codex-launcher.py`.

`XDG_CONFIG_HOME` and `CODEX_HOME` may be customized when their values are retained
through hook installation, `/hooks` review, update and Codex launch. The native
default bridge expects the Maeklog config at
`XDG_CONFIG_HOME/herdr/plugins/config/maeklog`; a separate noncanonical
`HERDR_PLUGIN_CONFIG_DIR` is rejected. Keep the managed plugin source directory
available: `maeklog-update` points to its updater, scripts and bridge patches.

## Verification completed

- A fresh clone of the public GitHub repository passed `scripts/check`: formatting,
  Clippy, 287 Rust tests, the 55 Python tests below, a release build and the RustSec
  audit of the locked dependencies.
- The tab integration fixture passed first-record creation, independent tabs,
  hiding, disabling, restart and moved-pane routing. A separate real Herdr session
  opened the public-checkout panel and checked `m`, `F10` and `q`.
- The four Python regression suites pass 55 tests covering native fingerprints,
  disabled/missing/stale approvals, malformed definitions, duplicate handlers,
  fresh installation, exact Herdr handler ownership, package version checks,
  backups, rollback, launcher dispatch and update ordering.
- Fingerprints for all four supported events match actual Codex 0.162.1
  `hooks/list` metadata in an isolated home. Only initialization and hook metadata
  were requested; no thread or model turn was submitted for that check.
- Official Codex 0.162.1 source was cloned, patched and built in a fresh target
  directory. Patch compatibility, dependency-lock normalization, Bazel metadata,
  formatting, seven native bridge tests, Clippy, the release build, matching
  official resources, installer, launcher and client-routing checks passed.
- An initially empty disposable HOME received the real Maeklog hook installation.
  The human trust boundary was simulated only in that fixture. Local preflight,
  installation with no existing Codex entry point or Herdr SessionStart handler,
  native package hashes and both launcher version checks passed. Hooks/configuration
  use private file permissions; the hook and state directory use `0700`.
- The installed stable launcher passed a native TUI lifecycle against a separate
  official app-server and a local response fixture: new conversation registration,
  prompt/final collection, daemon-environment isolation, `/new`, normal exit,
  resume, a resumed turn, and detachment. Automatic summary inference was disabled.
  No external model inference was performed.
- Re-running the public updater recognized the verified package without rebuilding.
  The existing user's Codex configuration, hooks, launchers and Maeklog settings
  were unchanged by the isolated verification.

Both Maeklog and the integrated client were built from freshly cloned source;
installed host build tools and downloaded dependency caches were reused. The long
client build resumed in the same fresh build directory after its runner was
interrupted, then completed the remaining checks before activation. This was an
isolated-home test, not a pristine operating-system installation. It does not
establish a new user's login or manual approval UI, macOS support, or actual
summary-model access and quality. Other Codex releases remain subject to patch
compatibility and the updater's complete verification.

## Public files and license

The repository contains source, pinned dependency metadata, integration patches,
documentation, synthetic fixture scripts, and declared demo/capture assets. Build
products and private validation artifacts stay outside the public file set.
The bounded public-file audit found no personal home paths, recognized credential
shapes, authentication files, runtime hook/configuration backups, collected inboxes,
or real reducer-failure captures. The release documentation also passed local-link,
version-consistency and GitHub Markdown rendering checks.

Maeklog's own code is under the [MIT license](../LICENSE). Upstream Codex-derived
patch context and its attribution remain covered by the bundled
[Apache-2.0 license](../integrations/codex/LICENSE-CODEX) and
[Codex notice](../integrations/codex/NOTICE-CODEX). Any redistributed client binaries
must retain the upstream licenses and notices for their complete dependency set.

Private vulnerability reporting is enabled on GitHub; see
[SECURITY.md](../SECURITY.md). A public repository with a
valid root manifest on its default branch can be installed with
`herdr plugin install reganpark/maeklog`. The optional `herdr-plugin` topic makes it
eligible for discovery; it is not a review or compatibility certification. See
[Herdr Marketplace documentation](https://herdr.dev/docs/marketplace/).
