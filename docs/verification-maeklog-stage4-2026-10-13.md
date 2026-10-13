# Maeklog menu and settings coherence verification — 2026-10-13

This record covers clearer menu controls, consistent model-settings paths, the
feature source backport to the existing implementation, and isolated verification.
Historical capture assets are unchanged and are not evidence for this work.

## Behavior

The content menu keeps **All records** as its default read-only action and places
**Summary model…** second, before current progress. Rows describe actions without
inactive main-view key suffixes. While open, the menu shows only its active
up/down, Enter and Esc footer. The model screen retains explicit save/cancel,
dynamic catalog efforts, current-setting preservation and zero inference on save.

The main view and link picker use `v` to check references again. Main-view `r`
continues to reflect eligible completed turns; uppercase `R` has no action.
The date picker consumes `m`, F10 and right click until cancel or date confirmation.
Opening another menu cannot bypass its explicit selection.

Model settings use an explicit `HERDR_PLUGIN_CONFIG_DIR` first. A namespace-owned
installed hook in `bin` with valid co-located `hook-runtime.json` uses that settings
parent; other CLI launches use the canonical Herdr plugin configuration directory.
A recognized malformed runtime fails without redirecting settings. Hook/state
path resolution retains its existing behavior. The backport gives model settings
the existing installation's canonical Herdr directory while keeping its original
hook-install fallback and state paths.

System language remains automatic, with no language switch: first nonempty
`LC_ALL`, `LC_MESSAGES`, then `LANG`; Korean locales use Korean and unsupported,
C/POSIX or unset locales use English. Separate desktop preferences and `LANGUAGE`
are not queried. Existing evidence, quotes and saved content retain their language.
Model/optional effort/language profiles and stale-request barriers retain the
[stage 3 contract](verification-maeklog-stage3-2026-10-13.md).

## Source and isolated checks

The final Maeklog release has SHA-256
`48a6b430e5136cf98f1e13d02bb571b665c882ebc3d984204a0d2cf2879a6f8b`.
The independently built existing-namespace release has SHA-256
`313242105911d9fe71d33dfcac28961739871a729f27ba486dca6c2e30af71b2`.
Both rebuilt binaries match their pre-interruption release bytes. Verification
uses these binaries and synthetic, isolated state/configuration and mock services.

| Check | Maeklog | Existing-namespace source |
| --- | --- | --- |
| Formatting and strict all-target Clippy | PASS | PASS |
| Full Korean unit suite | 260/260 PASS | 258/258 PASS |
| Full English/C.UTF-8 unit suite | 260/260 PASS | 258/258 PASS |
| Locked release build | PASS | PASS |
| Selected Node integration suites | 7/7 PASS | 16/16 PASS: 15 existing regressions plus new model picker |
| Python model-settings/profile runtime | PASS | PASS |
| Korean/English model picker PTY | PASS | PASS; 28 captured ANSI frames |

Each model-profile runtime suite exercises five live-summary and one
completed-turn **fake** model execution. No actual model inference was performed.
Checks cover installed-hook/explicit-config coherence, current-profile caching,
same-second stale commits, frozen queued settings and locale, source preservation,
no effort override, private atomic settings and failure preservation. Picker
fixtures cover wide/narrow/short menus, loading, dynamic models/efforts, scrolling,
no-effort models, stale/empty/offline catalogs, cancel purity and child cleanup.
Rendered fixture frames were reviewed for layout, visible model access and controls.

The existing-namespace source backport spans 42 Rust files, including four new
locale/catalog/settings/picker modules. After namespace adaptation and formatting,
32 match the candidate feature source exactly. The remaining differences retain
original labels, comments, synthetic fixtures, relay marker and the intentionally
separate canonical settings fallback. All 28 audited hook/native-integration,
schema, manifest, lockfile and launcher files match the original source HEAD.
No broad rename, dependency change or state-schema rewrite was required.

Local detailed evidence is kept under `$VALIDATION_ROOT`: `root-regressions/`,
`model-settings/`, `pm-model-settings/`, `pm-model-picker/`, `maeklog-ui/` and
`system-locale/`. These private validation artifacts are not checked into Maeklog.
The dated PNG/ANSI/text capture assets were not regenerated.

## Test observations and limits

Two PTY fixtures initially observed incomplete or stale frames. The date fixture
now waits for a completed footer, confirmed modal closure and the selected date
before Enter. The reader fixture observes each changed completed viewport before
sending another Down or resizing; the original maximum of 84 Down events is
retained. The same production binaries pass the corrected fixtures, with all
content, scope and explicit-action assertions retained.

An initial restricted-sandbox picker invocation timed out before capture. The
profile fixture completed its contract assertions but exited with a teardown
`Directory not empty` failure, so that run is not reported as passing. Escalated
isolated reruns of both suites completed with exit zero, including cleanup.
Initial failures are retained separately; a production cause was not established.
A late detached fixture writer is plausible, but not proven.

An earlier parallel unit run returned `Busy` rather than expected `Skipped` in
`lookup_failure_retains_last_success_and_backoff_skips_futile_calls`. Its isolated
retest and unchanged full serial suites passed. A controlled fork reproduction
shows that a close-on-exec descriptor can retain a file lease between fork and
exec after the parent closes its descriptor; explicit unlock ends that lease.
This mechanism is consistent with the documented inherited-handle semantics of
[Rust File locks](https://doc.rust-lang.org/std/fs/struct.File.html#method.lock),
but was not traced as the historical failure's cause. Production locking and the
backoff assertions remain unchanged. Final full-unit results above use serial runs.

## Runtime boundary

The verified Maeklog binary was copied to its local release location, registered
and enabled as an independent Herdr plugin. The existing four key-configuration
lines were switched to `maeklog.open` / `maeklog.hide` for prefix+m and
prefix+Shift+m, then reloaded. Live checks verified the registered Maeklog pane
in the intended task tab, its executable identity and unchanged focus.

The actual UI opened the menu and model picker, performed catalog metadata
discovery, and cancelled without changing settings. Discovery returned seven
models; Luna/medium defaults remained visible and no settings file was created.
`summary_requested` remained false, with no queued work or live-summary request. The original ten panels were retained; original Codex/runtime files and
hooks retained their hashes. Existing collection continues through the original
installation; this Maeklog connection is an independent preview.

The first plugin-action invocation followed Herdr's focused UI tab rather than
caller environment scope. Only the newly created preview pane was identified
and closed; the corrected direct scoped launch used the canonical plugin
environment and verified the intended tab. Existing panes and focus were preserved.
No production source change was needed for this routing correction.

The original running implementation is not upgraded by this source backport.
Its collector/hooks, live Codex bridge and existing records remain in place.
Maeklog and the existing implementation keep independent settings/state identities;
no record migration, hook handover, global Codex replacement, real-model evaluation,
Git initialization/commit, push or publication is included. macOS native execution,
terminal clipboard delivery and actual model output quality remain outside these checks.
