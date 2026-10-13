# Codex TUI live progress bridge

The Codex 0.162.1 patch observes the existing app-server notifications in the terminal client. It sends public progress already produced by Codex to Maeklog without making a model request. Existing `client attach` and `client detach` registration remains in place. Earlier source patches are retained as historical source references.

Only the attached primary thread is observed. Child, side-thread and hidden helper streams do not feed this bridge. An explicit `turn/started` opens progress capture; later events must match that turn. The optional upstream `rootTurnId` does not replace the active turn ID or enter the reduced payload. The cwd is frozen from the active terminal session at turn start, rather than inherited from the shared daemon or the hook process.

This document describes the current Maeklog source contract. See the
[user guide](../../docs/usage.md) for operation and the
[release readiness record](../../docs/release-readiness.md) for validation scope.

## Wire contract

The worker invokes the configured local hook directly, without a shell:

```text
maeklog-hook live event CLIENT_PID THREAD_ID
```

JSON arrives on stdin. Notification params retain their native camelCase thread and turn identifiers:

```json
{
  "sequence": 12,
  "cwd": "/actual/turn/cwd",
  "notification": {
    "method": "item/completed",
    "params": {
      "threadId": "attached-thread-id",
      "turnId": "turn-id",
      "item": {
        "id": "report-id",
        "type": "agentMessage",
        "phase": "commentary",
        "text": "The focused checks passed; I’m checking the full TUI suite."
      }
    }
  }
}
```

The accepted methods are `turn/started`, `turn/completed`, `turn/plan/updated`, `item/started`, and `item/completed`. Turn lifecycle params contain `threadId` and a reduced `turn` containing `id` and `status`; they never include the turn’s items. Plan and item params contain `threadId` and `turnId`.

Completed commentary, `partial_answer` and unphased assistant messages contain `id`, `type`, `text`, and the native optional `phase`; text is capped at 4,000 characters. A missing phase remains unknown. Explicit final-answer items, message starts and text deltas are omitted. Structured plans contain at most 16 steps of 400 characters each, plus at most 400 explanation characters.

Selected tool items contain their public identity and lifecycle metadata: command execution labels (300 characters), status and exit code; MCP server/tool and status; dynamic and collaboration tool names and status; file paths/kinds and status; image-view paths; and web-search queries. The native collaboration item type is `collabAgentToolCall`. Item IDs and thread/turn IDs are limited to 200 bytes. Tool arguments, tool results, command output, file diffs, reasoning, token deltas, hook prompts, user-message content, and raw response items are excluded.

## Delivery behavior

Observing an event does bounded projection and queue work synchronously. It never waits for the hook command. One background Tokio worker delivers commands in order, with null stdout/stderr and a two-second total timeout. A stalled or failing hook cannot delay the TUI notification path. Payloads at or above 32 KiB are discarded.

The queue holds at most 64 events. It coalesces the pending report and plan for each turn and the pending lifecycle state for each item, retaining the newest state. Turn starts and completions take precedence over tool floods. If all 64 slots contain lifecycle records, a new completion replaces the oldest lifecycle record; obsolete history is best effort rather than unbounded storage. Completed-turn tombstones retain the latest 32 turns and reject late updates or starts within that window. Maeklog’s persisted terminal hooks also close progress independently.

Sequence numbers increase per bridge instance. Coalescing leaves gaps, but delivery preserves order. A thread switch clears active capture while retaining pending events; a frozen Maeklog turn-origin route can accept an already queued completion after attachment changes. Detach keeps its existing PID/thread guard. Dropping the bridge aborts its worker, so shutdown does not wait for optional progress delivery.

## Verification

The upstream `herdr_bridge::live::tests` fixtures exercise private-field exclusion, completed-message filtering, attached-thread and late-event rejection, frozen cwd, Unicode/text/plan bounds, queue saturation, lifecycle coalescing and a slow executable fixture. The original attach/switch/detach regression remains under `herdr_bridge::tests`.

Run the focused tests from the patched Codex checkout’s `codex-rs` directory:

```sh
just test -p codex-tui -E 'test(herdr_bridge::)'
```

For a manual source port, also run `just test -p codex-tui`, then the upstream-required scoped lint/fix and formatting commands. The current patch is generated against official Codex 0.162.1 commit `092d3acd6bec3e3a14bdc7e7a2810ab628ab759d` and excludes local Cargo.lock and snapshot changes. The [historical 0.162.1 installation](../../docs/verification-codex-0.162.1-2026-10-12.md) of the original implementation passed the 7 focused bridge tests (5,719 unrelated tests skipped), scoped lint and a native progress fixture with `--partial-answer`; it did not run the full TUI suite.

Building the source alone does not activate the integration. Follow the
[installation guide](../../docs/getting-started.md) to review hooks and install a checked client package.
