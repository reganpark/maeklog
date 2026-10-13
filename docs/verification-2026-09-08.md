# Per-Space opt-out — v0.9.0

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

## Follow-up: clear demo-game and restart from three turns — v0.10.0

User-requested deletion targeted the live `demo-game` Space `<runtime-id-6>`, session `default`, cwd `$HOME/repos/gm/demo-game`. The exact Maeklog store was `<store-id-1>`. After stopping participation and waiting for the per-Space worker lease, ten Maeklog record files were deleted without a recovery copy: connection, current brief, cursor, inbox, processing diagnostics, retry, sequence, session reference, status and timeline. Matching shared action-timing diagnostics were filtered by known session ids. Only identity/off/visibility settings and lock files remain. The demo-game source files and Codex source conversation were not deleted. No Maeklog pane remains there, and the Space was left off rather than reactivated for a live test.

`prefix + m` already maps to the plugin's Open action. Open now explicitly reactivates an off Space; startup/focus do not. Reactivation reads at most three completed user/final pairs from the known current Codex session, without predecessor history, commentary or an unfinished turn. If unavailable it uses at most three completed collected turns from the latest queue session, or waits for new input. The old backlog is replaced, not later caught up. The previous brief/preview are excluded from the fresh model input. The seed has at most six messages and 14,400 content characters, fitting one healthy batch. Subsequent new events are separated by a restart boundary. Already-enabled Open is idempotent and does not re-import history. Earlier context is explicitly marked omitted.

Verification: 71 Rust tests, formatting/Clippy/release, RustSec, real-PTY menu tests, and existing recovery/latest/parallel suites passed. Updated isolated opt-out tests passed against debug and staged release: one bounded restart call, empty previous brief, no full bootstrap, idempotent Open, clear-records persistence, no automatic focus/startup restart, no model call for an empty reactivation, and normal processing of the next new turn. No paid model calls were used for these tests.

Installed v0.10.0 main/hook SHA-256: `671c3a56031aa87272b1bac4c6855b9adc75ea93bc4cd0487ce113e62b4f16d0`. Hook definitions remained unchanged. Six already-visible panels were updated and other participation/visibility choices preserved; demo-game remained off with no dialogue files. The superseded Maeklog coordinator was replaced; one already-running v0.9.1 worker in demo-shop-1 was allowed to finish under its lease, with no user Codex process interrupted. Temporary binary/preference backup: `/path/to/private/validation-artifact` (contains no recovery copy of deleted demo-game dialogue).

This v0.10 policy intentionally supersedes v0.9's refusal to re-enable through Open and its ban on all off-period import: pressing Open now explicitly permits the limited recent-three-turn import.

## Follow-up: pane right-click menu — v0.9.1

The original manifest actions did not add a native Herdr context-menu entry. v0.9.1 supplies a Maeklog-pane-only terminal context menu using Herdr's supported per-pane right-click routing. See [Herdr v0.8.2 mouse routing implementation](https://github.com/herdrdev/herdr/blob/v0.8.2/src/app/input/mouse.rs#L1619). No Herdr binary or global mouse setting was modified.

- Right-click in the Maeklog pane content (or F10) opens `Maeklog 끄기 · 이 Space`, `패널만 닫기 · 수집 계속`, and `취소`. The pane title/border still belongs to Herdr.
- 68 unit tests, formatting, Clippy, release build, existing recovery/latest/parallel suites, and opt-out integration tests passed.
- New real-PTY tests send SGR right-button press/release and left-button menu clicks to the actual panel. Cancel, Hide and Disable had the expected distinct state effects; right-click routing was limited to the fixture pane and restored on exit. Passed debug and staged release. Herdr was mocked in these automated tests; no paid model calls were made.
- The installed live `repos` Maeklog pane showed the menu via F10, then Escape dismissed it without closing the pane or changing participation. Actual physical mouse clicks in every live Space were not exhaustively tested.
- Six already-visible panels were updated; participation and hidden-state preferences, hook definitions, and the focused Space were preserved. Main user Codex processes were not restarted.
- v0.9.1 main/hook binary SHA-256: `e3fde3cc706a7e1f2bd61b3aa51dbba8ff38ea6f1aba6c234043a2c6b2dabdfd`.
- Temporary deployment backup: `/path/to/private/validation-artifact`.

The remainder records the earlier v0.9.0 opt-out implementation and verification.

Implemented and installed on 2026-09-08. Space actions independently control participation (`enable`/`disable`) and panel visibility (`open`/`hide`). No live Space was opted out for testing.

## Verification

- Formatting, Clippy with warnings denied, 66 Rust unit tests and release build passed.
- Existing recovery, latest-preview and three-space parallel-worker integration suites passed with mock Luna.
- New opt-out integration suite passed against both debug and staged release binaries. Real subprocesses/hooks/actions/file locks, mock Herdr and Luna, isolated temporary state; no AI API calls.
- Both cumulative and latest in-flight calls were cancelled; fixture model PIDs exited, raw queue/current/timeline/sequence/connection stayed byte-identical and retry attempts were restored. A peer Space completed normally.
- Disabled hooks, explicit open/refresh/rebuild, direct panel launch, startup, focus and coordinator restart could not resume collection or inference.
- Re-enable resumed the retained queue. Whole-transcript bootstrap and a retained rebuild request were excluded. Late/mismatched finals were ignored. Hiding an enabled panel still allowed future user/final collection and summary commits.
- Unit tests cover invalidation after rapid off/on, rebuild eligibility after opt-out, and cancellation of TERM-ignoring descendants without waiting for the full timeout.
- RustSec audit passed (36 dependencies, 1,242 advisories at audit time).

## Installed state

- Main binary and hook binary SHA-256: `d0b2cc95b9d298e301966370c84298dc29ec479f1f6a8e443250eb7c31b2faf1`.
- Hook definitions were not changed or reinstalled. Their SHA-256 stayed `d042b423c5112b3e4031466088c1e4091e1b2afe4fce4518d7186c8ec19c66d0`.
- Herdr action discovery exposes Turn off / Turn on Maeklog in this Space and distinguishes Hide (keep collecting).
- Six already-visible Maeklog panels were refreshed; their executable inodes and the active coordinator/space worker matched v0.9.0. No superseded space worker remained at handoff. Main user Codex processes were not restarted or signalled.
- All eight stored participation preferences and visibility choices were preserved, including hidden `<runtime-id-7>` and `<runtime-id-6>`. The focused Space at handoff stayed focused.
- Local binary/preference backup: `/path/to/private/validation-artifact` (temporary; not durable archival storage).

## Boundaries

This is implementation-author testing, not an independent review or an exhaustive live off/on test in every user Space. The stop behavior was exercised with local mock models, not a paid in-flight API call. Local cancellation cannot retract requests already sent or guarantee immediate remote billing cessation.

After any opt-out, whole-transcript rebuild remains unavailable in that Space to prevent opted-out messages from being imported. Existing summaries/queue and future collected turns remain usable with Refresh. Codex's own transcript storage is unchanged. Older Maeklog releases do not honor the new preference, so downgrading is not safe while relying on per-Space opt-out.
