# Primary-turn task reference reports

The supported Codex 0.162.1 source supports `UserPromptSubmit` hook
`hookSpecificOutput.additionalContext`. The Maeklog helper uses this official context
path to ask the current primary coding agent to relay known work references.
It starts on the next valid user prompt; it does not inject instructions into a
turn already running at helper replacement. A client without this context still
has conservative extraction from the prompt already collected by Maeklog.

The added context contains a source UID, revision, current thread ID, helper
executable and a private temporary spool path. It does not repeat the user
prompt, GitHub titles or issue bodies as developer instructions. The normal
coding agent handles the report; no classification model or extra Maeklog model call
is introduced. Context tokens and reporting tool calls still have a cost.

## Transport

The source-bound additional context uses `MAEKLOG_TASK_REPORT_CONTEXT_V1`.
This document describes the current Maeklog source contract. See the
[user guide](../../docs/usage.md) for operation and the
[release readiness record](../../docs/release-readiness.md) for validation scope.

The agent reads the minimal current receipt and revision:

```sh
/path/from/hook/maeklog task context /tmp/maeklog-task-reports-STATE_HASH/NONCE
```

It sends bounded JSON on stdin to the same helper's `task report SPOOL` command:

```json
{
  "source_uid": "from-current-context",
  "expected_revision": 7,
  "operation": "set",
  "references": ["owner/repository#142"],
  "quote": "owner/repository#142 작업을 진행해 주세요.",
  "worktree": null,
  "created": false
}
```

Operations are `set`, `replace`, `resume`, `clear`, `related` and `reference`.
Current-target operations require a direct present user instruction supported
by its exact current clause. The relay asks for a clause of at most 600
characters; retained evidence is capped at that length. A quote alone does not make
quoted examples, comparisons, exclusions or past work current. Related PRs
remain candidates until GitHub metadata and the verified worktree establish
their relationship to a bound issue. `created` records the agent's report; it
does not prove creation or completion. Multiple explicit targets retain their
roles instead of selecting an arbitrary primary.

Workspace-write tools cannot normally write the private Maeklog state outside the
repository. The report CLI therefore writes only a host-created 0700 temporary
queue, with 0600 JSON files. An existing host tool-lifecycle callback, Stop hook
or interactive panel background job drains it. No daemon, permission expansion
or escalation request is added. The sender immediately returns `queued`; it
does not wait for a tool-completed callback that can arrive only after it exits.
Before reporting again in the same turn, `task context` waits at most two
seconds for the latest receipt/revision. Pending or rejected reports do not
block the coding task and must not trigger retry or permission loops.

Messages are at most 16 KiB, with four pending requests and eight retained
receipts per spool. Messages expire after ten minutes and the source-bound path
after 24 hours. Stale requests get rejected receipts or a retired marker.
Duplicate payloads reuse their request/receipt. A missing temporary spool after
reboot is ignored and the stale private host record is removed; the next valid
prompt recreates the path. Render-once, history and navigation never drain.
Ordinary opt-out preserves references. Explicit private record deletion removes
that store's current and retired spools, including queued quotes, without removing
another tab's report data.

## Routing and intent

The private host record freezes the Herdr route, terminal PID, primary thread,
turn, source sequence and participation generation. The host rechecks the live
terminal's foreground Codex process, registered native thread, current source,
active thread and revision before applying a report. It holds the participation
gate through verification and apply. Lock order is host drain lease →
participation gate → queue → task state; a sender takes only the queue lock.

The CLI also requires runtime `CODEX_THREAD_ID` to equal the primary thread.
This catches a child tool inheriting the parent's context. Child hook markers
do not receive the producer context. Child results must be relayed by the
primary agent after checking their relationship. These checks address routing
and accidental inheritance; they are not attestation against the same local
user deliberately forging environment variables or private files.

Native attach/switch controls the active task pointer. A verified single already
registered client can initialize a missing pointer in a host background job or
explicit picker opening after upgrade. A late hook cannot restore an attachment.
New threads do not inherit the previous thread's card; resuming restores that
thread's own references. Manual selection and explicit none establish a source
barrier, so an older request cannot overwrite them by reading a newer revision.
Only a genuinely later explicit user choice may change the current target.

No report means no inferred clearing or success. Uncertain references remain
candidates; GitHub existence alone does not establish user intent. Worktree
association does not move historical branch records, and PR merge does not
mark the Maeklog work complete. Tool outputs, command output, file diffs and reasoning
remain excluded from the existing live progress capture.

## Isolated checks

`node scripts/test-task-reports.mjs PATH_TO_MAEKLOG_BINARY EVIDENCE_DIR` exercises
CLI/hook/live routing, stale and manual barriers, bounded queue, Stop drain and
missing-spool recovery with mock Herdr and no model.

`scripts/test-codex-task-reports.py` uses a supplied patched TUI candidate, an isolated
official app server, a fresh Codex home, mock Herdr and a localhost Responses
fixture. It checks the actual main model request containing hook context and
executes Set then Related through native workspace-write tools while no Maeklog
panel is open. The child check substitutes a different execution thread ID;
it does not claim to execute an actual spawned child turn. No external model or
user configuration is used.
