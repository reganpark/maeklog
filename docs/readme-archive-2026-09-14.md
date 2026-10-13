# 이전 README 보관본

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

이 문서는 2026-09-14에 README를 재정리하기 전의 보관본입니다. 여러 버전의 설명이 섞여 있으므로 현재 사용법은 [현재 README](../README.md)를 따르세요. 아래 내용은 변경하지 않고 보존했으며, 원문에 있는 상대 링크는 이전 README 위치를 기준으로 합니다.

---

# Maeklog

Maeklog keeps a persistent project brief beside each Herdr space. Your main Codex session can stay on GPT-5.6 Sol Max; a separate, bounded GPT-5.6 Luna Max run reduces completed turns into objective, decisions, progress, blockers, and next actions.

This repository is an early local-first implementation for Herdr 0.8.2.

## What works in v0.6

- Linkable Herdr plugin manifest with a managed right-side terminal panel.
- Per-space state with atomic revisions, an event cursor, timeline, and persistent open/closed preference.
- Codex `UserPromptSubmit`, `Stop`, and `Interrupt` event ingestion without injecting anything into the Sol context.
- A single global Luna worker with timeout, process-group cleanup, schema validation, and stale-revision protection.
- Live Herdr agent status and local Git facts kept visually separate from the last verified summary.
- Explicit hook install/remove actions that preserve unrelated Codex hooks and make a backup first.
- A bounded first-run/rebuild import that reads only user messages and final assistant answers from the local Codex session.
- Automatic discovery of live Codex session ids through Herdr, including agents that were already running before Maeklog was installed.
- Deterministic summary guardrails: transient acknowledgements cannot become goals, assistant-only completion claims render as reported, and obsolete decisions stay out of the default panel.

## Safety boundary

Nothing is installed automatically when the repository is cloned or linked. Herdr plugins run as the current user and are not sandboxed by Herdr, so review `herdr-plugin.toml` and the Rust source before enabling it.

Maeklog is one Rust binary. It deliberately does not use `tokio`, `clap`, `fs2`, or `ratatui`. Direct runtime dependencies and the audit rationale are recorded in [Dependency decision](dependencies.md).

The Luna run uses:

- model `gpt-5.6-luna`
- reasoning effort `max`
- read-only sandbox
- ephemeral session
- ignored user configuration and disabled hooks
- a fixed JSON output schema
- a hard wall-clock timeout

Normal reductions receive only the previous brief, unprocessed user/final-assistant turn events, and a bounded Git snapshot. The first reduction can bootstrap from a bounded local Codex transcript excerpt. If a newly started session is too sparse to carry project context, Maeklog may prepend only the most recent session from the same exact working directory within 30 days; both files must still fit the existing 64 MiB input ceiling and the final excerpt keeps the same message and character limits. The excerpt excludes developer messages, reasoning, commentary, tool calls, and tool output. A manual rebuild uses the same filter and never stores the raw imported excerpt in Maeklog state.

## Development

```bash
./scripts/check
herdr plugin link $HOME/repos/maeklog --disabled
herdr plugin list --plugin maeklog --json
```

After review, enable it and install the companion Codex hooks explicitly:

```bash
herdr plugin enable maeklog
herdr plugin action invoke maeklog.install-codex-hooks
```

Codex will require the new hook definitions to be reviewed and trusted through `/hooks`. Open a panel manually with:

```bash
herdr plugin action invoke maeklog.open
```

Once enabled, focusing a workspace whose panel preference is visible opens its context split without stealing focus. Use `q` inside the context panel to keep that space hidden; invoke the `open` action to show it again.

When an `r0` space is opened, Maeklog asks Herdr for the live Codex session id and schedules the one-time history import automatically. No dummy prompt, agent restart, or per-session hook setup is required. A manual retry remains available with:

```bash
herdr plugin action invoke maeklog.rebuild
```

Rebuild is best-effort because the local Codex rollout format is not a public plugin API. Missing, oversized, or unreadable history leaves the existing verified revision untouched and surfaces a recoverable panel error.

### Turn Maeklog off in one Space (v0.9)

Right-click **inside the Maeklog pane** (v0.9.1):

- **Maeklog 끄기 · 이 Space**: stop collection and summarization for this Space and close its Maeklog pane.
- **패널만 닫기 · 수집 계속**: close only the panel; keep collecting and summarizing.
- **취소** or Escape: dismiss the menu without changing anything.

This is Maeklog's own terminal menu, enabled only for its pane using Herdr's per-pane right-click routing. Other panes keep their Herdr menu. The Maeklog pane's title/border is still Herdr-owned; right-click the content area. `F10` also opens the menu, arrow keys select and Enter activates. Mouse-wheel scrolling works in the panel. On normal panel exit its mouse capture and right-click routing are restored. No Herdr binary or global mouse configuration is patched.

The same operations also remain available as CLI/plugin actions (they do **not** automatically add entries to Herdr's built-in context menu):

- **Turn off Maeklog in this Space**: close its Maeklog panel, stop collecting new user/final messages and stop its Luna work. Other Spaces keep running.
- **Open / prefix + m / Turn on Maeklog in this Space**: an explicit open re-enables a stopped Space, starting from at most the latest three completed turns. Focus changes and Herdr startup do not re-enable it.
- **Hide Maeklog (keep collecting)** / panel `q`: hide only; collection and summarization continue. Opening an already-enabled panel does not restart or re-summarize its history.

The same actions can be invoked from a terminal in the intended Space:

```sh
herdr plugin action invoke maeklog.disable
herdr plugin action invoke maeklog.enable
```

Off preserves summaries, timeline and the existing queue until explicit reactivation. A running local reducer is cancelled on its next poll (normally about 40 ms, with up to 500 ms termination grace); output from before a preference change cannot commit after that change, even after a quick off/on. This cannot retract an API request already sent or guarantee remote billing stops immediately. No main Codex/Herdr process is stopped. Cancellation does not consume a retry attempt.

### Recent-only reactivation (v0.10)

In the intended Space press `prefix + m` (currently Ctrl+B, then m). This explicitly permits reading the latest **three completed turns** from its known current Codex session, including turns completed while Maeklog was off. One turn means a user request and final assistant reply; unfinished requests, commentary, tools and predecessor sessions are excluded. Fewer than three means only those available. If no transcript turns are available, use the latest three completed turns still in the Maeklog queue, or wait for future input if neither exists. A read error leaves the Space off rather than loading unbounded history.

The old pending queue is replaced, not caught up in the background. The previous brief and preview are not sent to Luna on this fresh start; the timeline remains on disk unless records are explicitly cleared. The seed is capped at six messages and 2,400 characters per message, fitting one normal batch; subsequent new turns are processed normally. A pending restart boundary separates concurrently arriving new dialogue from the initial three-turn summary and prevents duplicate latest-preview work. Earlier history is marked omitted, not implicitly approved. Repeated open/focus while enabled does not re-import or charge for the same seed.

Whole-transcript bootstrap and `b`/Rebuild stay disabled after an opt-out. Use `r`/Refresh for the currently retained queue and future collected turns. This limited, explicit reactivation replaces v0.9's policy of never importing any off-period dialogue. Codex's own transcript retention is unchanged; Maeklog opt-out is not a Codex logging switch.

For an explicit data-deletion request, `maeklog action clear-records` stops the current Space and deletes its Maeklog record files and matching shared diagnostic entries, without a recovery copy. Identity, visibility/off settings and lock files remain so focus/hooks cannot immediately recreate the deleted dialogue. Other Spaces and Codex source transcripts are untouched. This is not exposed as an easy-to-misclick menu item.

`participation.json` stores the preference separately from panel visibility, with private permissions. Old releases do not understand it: do not downgrade while relying on per-Space opt-out. Tests: `node scripts/test-opt-out.mjs /path/to/maeklog` runs isolated real hooks, actions, file locks and workers with mock Herdr/Luna; it makes no AI calls and changes no live Space preference.

`node scripts/test-panel-menu.mjs /path/to/maeklog` additionally drives real SGR mouse press/release events through a Python-stdlib pseudo-terminal, checking menu rendering, cancel/hide/disable effects, and pane-scoped routing cleanup with mock Herdr.
The same rebuild is available with `b` while the context panel is focused; `r` processes newly queued turns without importing history again.

A rebuild retains unanswered user requests and approvals as explicitly unfinished turns. This prevents asking for an approval already given, without treating the requested work as completed or consuming its pending event.

Interrupted work resumes on startup or the next space activation when no coordinator or worker for that space owns its lock. Cumulative records are processed chronologically in complete-turn batches, stopping at session boundaries, with up to 8 events / 16,000 content characters (one oversized completed turn is kept whole). Different spaces run concurrently under the limits below. A failed batch retries with just one completed turn, after 60 seconds and then 180 seconds, for at most three attempts per committed cursor. Incremental calls still have a 180-second timeout. The retry budget survives worker restarts and new events; `r` resets it for the selected space. Failed calls never advance the cursor or delete queued input. The coordinator schedules retries; visible panels also check for interrupted work every 15 seconds. Hidden panels retain their closed preference during recovery. The `render-once` diagnostic command never triggers a model call or changes the brief.

### Bounded parallel scheduling (v0.8)

`scheduler.rs` owns dispatch only; `worker.rs` retains the existing one-attempt selection, reduction, validation and retry rules. No asynchronous runtime or dependency was added.

- One `worker` coordinator holds `worker.lock`, scans eligible spaces and reaps child processes. Dispatch rotates after the last selected space so a busy early space cannot repeatedly take every free slot.
- Each internal `worker-space <registered-key>` process takes the space's `.worker.lock` and one of three `worker-slots/N.lock` files before attempting a reduction. These OS locks enforce **at most three simultaneous space attempts per state root, and one per space**, including latest and cumulative work. Invalid/unregistered keys cannot address arbitrary paths.
- Each child performs one attempt and exits. Retry backoff does not hold a slot. A slow space cannot prevent other available slots from progressing; a malformed space or failed child is isolated from healthy spaces for that coordinator's lifetime.
- Space workers own their leases independently of the coordinator. If the coordinator is killed/restarted while its children remain alive, those children continue and a replacement coordinator cannot duplicate them or exceed the occupied slots. A normal completed child releases both locks automatically. The panel's active-work check uses the space lock, not the coordinator lock.
- This does not increase batch size, timeouts, models or retry budgets. Parallelism can increase instantaneous service load; it is not a promise to avoid API rate limits. Existing calls are not preempted.

Validation: `node scripts/test-parallel.mjs /absolute/path/to/maeklog` runs mock models behind gates and tests three-way overlap, rejection of a fourth slot, same-space exclusion, slow/retry/corrupt-space isolation, coordinator SIGKILL/restart, and records arriving during a reduction. It makes no AI calls. Recovery tests do not simulate arbitrary SIGKILL of the individual `worker-space` process while its model survives; manual low-level process cleanup must account for both the space worker and its model process group. Never remove lock files while workers are running.

### Latest-first view (v0.7)

When the queue cannot fit one ordinary batch, a history rebuild is pending, or chronological retries have stopped, the worker prioritizes the newest completed turns for that space. It selects up to 8 events / 16,000 content characters from the tail, retaining chronological order inside the selected turns and respecting session boundaries. An unfinished latest prompt is not treated as a completed task. A failed latest reduction retries only the newest complete turn. Existing in-flight model calls are not preempted; latest and cumulative reductions for the same space remain serialized.

Two local snapshots separate freshness from completeness:

- `current.json` and its existing cursor/timeline remain the chronological, cumulative memory.
- `latest.json` stores a provisional latest-first view based on the previous visible brief and newest selected events. It never advances the chronological cursor or removes raw events. Intermediate older commits cannot replace the visible latest view. Once cumulative memory has incorporated events through the same sequence, the panel returns to cumulative memory automatically. The last preview file remains available locally but is no longer selected.

The preview can omit earlier approvals, constraints or decisions. It therefore shows `최신 작업 반영` separately from `과거 맥락 미완료` and the cumulative cutoff. `누적 정리 N개 대기` counts the entire remaining chronological queue, including records already previewed; it is not a claim that all N records have never been summarized. New records beyond the preview are counted separately. All next actions in the provisional view display `잠정`, never an unqualified `지금` label. Newer user decisions still take precedence within supplied evidence; a language-model summary is not a formal proof of semantic consistency.

Latest work has a separate persistent three-attempt budget in `latest-retry.json`, keyed by newest completed event sequence, and `latest-status.json`. An old chronological failure cannot block new latest work. Exhausted budgets do not reset on restart; genuinely new completed work gets a new latest key. A previous latest failure still forces the newest complete-turn-only fallback, even when new input changes the key; a successful latest call clears that fallback. During retry waits or after exhaustion, cumulative work may continue with its own budget. `r` resets both budgets for the selected space. A healthy queue that fits one regular batch uses a single ordinary reduction, avoiding preview duplication.

**Cost and retention:** The preview uses the same Luna Max model, read-only sandbox, 180-second timeout and privacy modes. Both preview and cumulative work share the parallel scheduler's space leases and global slots. Previewed events are processed again when chronological memory catches up, so a backlog can incur additional Luna calls. Continuous new work can delay historical catch-up indefinitely; no catch-up ETA is promised. Raw records remain until their cumulative commit succeeds, not merely until preview succeeds. No new dependencies or hooks were added. Restart preserves both views; older plugin versions ignore the preview and continue from the unchanged chronological cursor. Before rolling back to a sequential version, drain parallel space workers: old versions do not understand their independent leases.

Test latest-first recovery without AI calls: `node scripts/test-latest.mjs /absolute/path/to/maeklog`.

A 24-event catch-up ceiling was tested on demo-shop-1 on 2026-09-07: the 24-record / 6,547-content-character call timed out at 180 seconds. This is not proof that size caused the timeout, but it does not demonstrate improved throughput either. The larger ceiling was therefore not retained as the default. No concurrency, timeout, model or retry-budget increase was introduced.

Each space keeps the last 50 finished processing attempts in private `processing.json` (0600): start time, elapsed milliseconds, mode, attempt number, pending/selected event counts, selected character count, event sequence range, and `success` / `timeout` / `failed`. Dialogue, summaries, raw errors and credentials are not logged. Diagnostics are best-effort and cannot reject a successful commit. Rebuild character counts cover queued events only, not imported history. Crashes before an attempt finishes do not produce an entry. Compare these samples with cursor advancement before increasing limits further.

The companion `SessionStart` hook records a new/resumed Codex session without resetting the space summary, injecting context, or making a model call for the start event alone. `compact` starts are ignored. New session references take precedence over old queued events; a late old-session completion cannot replace the new connection. If Herdr reports an unknown session while hooks still refer to another, the panel asks the user to check `/hooks`. Known older Herdr session ids are not treated as new because restored command arguments can be stale. This detection cannot prove delivery of every hook; an undetected/stale Herdr id can hide a missing hook. Hook trust is never bypassed: review the new `SessionStart` handler through Codex `/hooks` after installation. Existing user/final hooks continue to reconnect on the next recorded turn.

Recovery integration checks use a mock reducer (no AI calls): `node scripts/test-recovery.mjs /absolute/path/to/maeklog`.

`connection.json` also retains `session_start_id` and `session_start_at` for the last actually received SessionStart hook. UserPrompt/Stop fallback reconnection does not manufacture this receipt. Older metadata stays readable with empty optional fields; an absent old receipt is not treated as proof of a current delivery failure.

### Verification scope and follow-up hygiene (v0.8.1)

A reported completion remains `[보고]` unless independently supported, but that uncertainty alone must not recreate the completed work as another action or blocker. The reducer distinguishes automated checks, representative real-session checks and all-space checks. It narrows unfinished actions to the untested scope, preserves explicitly required approvals/reviews, and treats an optional inactive-space MCP check as a conditional future action rather than an error stopping unrelated current work. These are model instructions, not a guarantee that every future natural-language classification will be correct.

The opt-in evaluation `node scripts/test-summary-scope.mjs --live /absolute/path/to/maeklog` makes **three real Luna Max calls** using synthetic fixtures in a separate temporary state root. It tests reported completion without recursive audits, optional inactive-space dependencies, and preservation of mandatory approval gates / remaining live-test coverage. There are no automatic retries in this evaluation. It retains its result file for inspection. Normal automated tests remain model-free. See `docs/verification-2026-09-07.md` for observed live-session coverage; do not equate passing unit tests with independently auditing all live spaces or all generated summaries.

After a cold Herdr restart, a saved panel can return as an ordinary shell. Maeklog now checks its actual foreground process and restarts the panel in the same saved pane when that pane is still labelled `Maeklog` and contains only its shell. A running panel or another program is left alone. Per-space lifecycle locks serialize concurrent open/restore/hide actions. Closing a restored panel also works when Herdr no longer retains its plugin ownership registration.

The restart fix was checked with an isolated headless Herdr server: stop/start restored the same pane and cached revision, repeated startup kept the same process, and hide followed by stop/start kept the panel hidden. The test's XDG directories and `HERDR_SOCKET_PATH` were isolated; no model call was needed to render the retained brief. This is a server restart test, not a physical machine power-cycle test.

## Data lifetime

The panel header shows the timestamp of the conversation included in the brief, not the time Luna generated it. Decisions and progress items carry timestamps from their cited hook events or transcript records. Dates display in Korean time (UTC+9); older years include the year. Unchanged items retain their original timestamp across reductions, while changed statuses use new supporting evidence. Luna never supplies timestamp fields.

Freshness is separate from the agent state (`Codex · 입력 대기` is not summary completion). The header always shows pending record counts, including today's count in Korean time, even without a pending user prompt. It distinguishes queued summaries, Luna processing, automatic retry waits, stopped summaries, and open turns awaiting completion. Only a saved brief with no queued records, rebuild, error or detected connection mismatch shows `반영 완료 · 수집된 대화까지 반영`. This confirms the collected queue, not undetectable missing hooks or omitted historical dialogue. Summary and queue are read under the existing event lock to avoid false completion during a concurrent commit.

Decision and progress sections show newest dated items first, before applying the display limit. Undated items follow dated items; equal timestamps retain their existing order. This is display-only ordering and does not rewrite saved history.

Blockers prioritize user decisions needed now, then current-work errors/external waits, then later-stage issues. Next actions display readiness and owner, for example `[지금 · 사용자] 계획 승인`, `[승인 후 · Codex] 구현 시작`, and `[구현 후 · Codex] 검증 실행`. Prerequisite order comes before recency; only items in the same priority or stage are sorted newest first, using locally resolved source-event times.

Luna supplies these small classifications in the next summary. Legacy unclassified blockers display `미분류` and actions display `순서 확인 필요` until then; no automatic model call or guessed readiness is added. The reducer continues to omit resolved blockers and completed actions from the next brief. Empty sections disappear; there is no new resolved-status workflow or history UI. Previous snapshots remain in the existing timeline.

v0.6.1 isolates inconsistent action timing: a contradictory or partially missing `step`/`when` pair becomes `(null, null)` and displays `순서 확인 필요`. The action text, owner, and evidence remain intact; it is never promoted to executable now. All other validation still runs before committing the brief, including timing field type/range/length checks. A bounded private diagnostic file, `runtime/diagnostics/action-timing.json`, retains the last 50 affected reductions with their session/event range, base revision, action index and conflicting values as seen by the validator. It does not save full conversations or action text. These diagnostics record a correction attempt, not proof that the rest of the summary passed or committed.

Older briefs without timestamp metadata remain readable and show `시각 미기록`; their summary-creation time is labelled separately. Maeklog does not guess historical event times or automatically spend model calls rebuilding every old brief. New reductions capture times automatically, and an explicit history rebuild can recover times from the available source records. Timestamp metadata commits atomically with each brief and also survives in its timeline.

The state directory is supplied by Herdr as `HERDR_PLUGIN_STATE_DIR`. Briefs do not disappear after a day or when the panel closes. Raw queued turn text is removed after a successful cumulative summary, never after a latest preview alone. Maeklog has no delete action; state removal is deliberately manual while the data model is still being validated.

See [architecture](architecture.md) and the [surface contract](ui-contract.md).

