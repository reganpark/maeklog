# Maeklog verification — 2026-09-07

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

Scope: highest-priority session-transition and follow-up classification checks for v0.8.1. This is implementation-author testing with recorded observations, not an independent all-space audit or proof of every generated summary's accuracy.

## Actual session transition

Used the installed Codex CLI v0.153.4 with the user's already trusted hook definitions and Luna Max. No hook-trust bypass or new project trust was added. Existing Herdr panes and active user sessions were not reset.

The temporary CLI used a synthetic workspace key and a separate Maeklog state directory. Herdr socket/pane reporting variables were unset so the test could not overwrite any live pane's session identity. Therefore this proves the native CLI-to-Maeklog-hook-to-Luna pipeline, not a live Herdr-pane `/new` operation in every existing Space.

Observed:

- Initial request and `PM_SESSION_BEFORE_OK` response were recorded and summarized as r1, cursor 2.
- Actual `/new`, followed by `PM_SESSION_AFTER_OK`, changed the session ID and produced r2, cursor 4.
- After the v0.8.1 hook update, another actual `/new` and `PM_SESSION_FINAL_OK` produced r3, cursor 6.
- The original objective, `새 세션에서도 Maeklog의 기록이 보존되는지 검증한다.`, remained identical in all three revisions.
- The final `connection.json.session_start_id` matched the new session ID, proving actual SessionStart receipt rather than merely fallback reconnection on UserPrompt/Stop. Two prior session IDs remained tracked.
- After the last reduction, the test inbox was empty and the worker was idle. The temporary interactive CLI was shut down normally.
- The three newly created test sessions were archived (recoverable), preventing them from being selected as ordinary recent predecessor context for the user's next session. Existing user sessions were not archived or restarted.

The three test session IDs were `<session-id-1>`, `<session-id-2>`, and `<session-id-3>`. The test's Maeklog state and timeline were retained under `/path/to/private/validation-artifact` at verification time. This temporary path is not a permanent evidence archive.

At inspection time, live Codex session IDs matched the plugin's remembered IDs in demo-shop, demo-shop-1, demo-shop-2, demo-space, demo-business and repos. This read-only match is not a forced transition test of all six. Unknown-session warning behavior, old-session late events, compact exclusion and fallback reconnection are covered by automated tests; a deliberate mismatch was not injected into a user's live pane.

[Official hook documentation](https://learn.chatgpt.com/docs/hooks#sessionstart) defines the SessionStart event and its startup/resume/clear/compact sources. Local delivery observations above, rather than documentation alone, establish the tested result.

## Actual Luna summary checks

`node scripts/test-summary-scope.mjs --live target/debug/maeklog` used the same candidate reducer code and three synthetic fixtures with GPT-5.6 Luna, reasoning max. Each fixture allowed one attempt and used isolated state, with no live repository changes. All three calls completed and passed their assertions; outputs were also inspected.

| Case | Observed output |
| --- | --- |
| Automated tests reported complete; real `/new` check not run | Kept automated completion as `reported`, removed the repeated automated-test action and the missing-independent-proof blocker, retained only the real transition check. |
| Optional MCP check in an inactive Space | No current blocker; retained MCP checking as a step-1 action conditional on opening that Space. |
| Mandatory security approval and only one of three Spaces tested | Preserved the user-approval blocker, narrowed the remaining check to the other two Spaces, and kept deployment conditional on approval and verification. |

The evaluation's synthetic input says “62 tests”; that is a fixture value, not this release's test count. Detailed synthetic results were retained at `/path/to/private/validation-artifact` at verification time.

These cases demonstrate the targeted correction, not deterministic correctness for all future model outputs. No rule automatically deletes every verification item or marks reported completion independently confirmed.

## Automated and release checks

- 63 Rust tests passed, including session receipts, preserved memory, mismatch warnings, display ordering and evidence/certainty rules.
- Recovery, latest-first and parallel-process integration tests passed without AI calls.
- Formatting, Clippy with warnings denied, release build and RustSec audit passed.
- Hook command definitions stayed byte-for-byte unchanged. The model, reasoning effort, concurrency cap, timeouts and retry budgets were not changed by this release.

## Not completed or claimed

- No forced `/new` in every existing Space, and no independent full transcript-versus-summary review of all five historically mentioned Spaces.
- No guarantee that all completed actions will always disappear correctly from future natural-language summaries.
- demo-shop-1 and demo-shop-2's historical cumulative backlogs were not cleared or declared complete. Latest-first summaries and cumulative completeness remain distinct; exhausted historical retry budgets were not silently reset by these checks.
- demo-shop-3 had no live Space/session during inspection. Its next-start MCP connection check remains pending and is not, by itself, a blocker for current Maeklog panel work.

These remaining checks must not be relabelled as completed merely because the release tests passed. Conversely, missing independent certainty alone must not create endless repeat tests for already reported completed work.
