# Architecture — v0.12.0

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

Maeklog keeps two separate lifetimes: per-Space collection/queues and repository-branch memory. No database was added; private JSON files and existing file-lock/atomic-replacement helpers remain the storage mechanism.

## Evidence and identity

Hooks still admit only SessionStart, UserPromptSubmit, Stop and Interrupt. SessionStart updates connection metadata, not dialogue. User prompts, final answers and interruption markers retain their existing scope. No reasoning, commentary or tool output is newly collected.

At capture, `branch_context.rs` resolves the canonical Git common directory, full local branch and HEAD. Worktrees share the common directory. The first branch reflog record plus file identity/birth fingerprint separates branch incarnations conservatively. Missing Git context/reflog or imported pre-v3 events remain Space-owned unlinked records. Renaming, deletion/recreation, reflog replacement and separate clones are not automatically merged. Deleted scopes remain addressable with `memory list/show/history`.

A final event inherits the captured same-session/turn prompt context. The model receives the batch's captured context, not today's checkout facts. A context-bound batch cannot consume the following branch's events. Space `current-context.json` prevents prose from the last processed branch becoming the next branch's model base. Legacy unlinked prose is usable only in its unlinked scope.

Stable IDs use a session/turn/kind + payload fingerprint where turn IDs exist. Missing turn IDs fall back to Space/generation/sequence and cannot prove cross-Space identity. Root `event-origins.json` prevents the same event being delivered again after checkout into another scope. Branch receipts allocate shared order. Source sidecars preserve Space/session/turn/context/time after queue consumption. Hashes are identity/dedup hints, not cryptographic signatures.

Git common-directory behavior follows [git rev-parse](https://git-scm.com/docs/git-rev-parse). Reflog/file identity is a conservative local heuristic, not a Git-issued immutable branch ID. A log rewrite may split one logical branch; this is reported/documented rather than guessed away.

## Reduction and durable commit

v0.12 eligibility is per Space and per local calendar date. Only collected complete turns whose source events are all today are automatic. Unknown/cross-midnight/earlier dates and reactivation-excluded today turns are manual. No first-open/focus/SessionStart bootstrap transcript is scheduled; legacy rebuild.json is not authority. The old bounded importer/preview producer remain compatibility test code, not current runtime entry points.

The date picker (b or the Maeklog menu) is read-only until Enter. A dated request freezes its exact pending event IDs and participation generation. It has priority after the current in-flight batch; fresh arrivals are not implicitly included. Opt-out invalidates that request. r cancels remaining manual selection and requests today's retry only.

The scheduler retains three global slots and one lease per Space. A branch reduction lock additionally serializes model work against the same journal. Waiting for that lock does not consume a retry attempt. Batches are newest-first, bounded by the existing 8-event/16,000-character limit (one oversized turn stays whole). Each selected terminal event has a durable retry budget in daily-retries.json so newer arrivals do not reset older failures. Active automatic work has a local-midnight cancellation deadline; unfinished raw input is retained.

Schema v3 retains legacy summary fields for import compatibility and adds `record_updates` and `state_updates`. A partial catalog contains at most 24 records / 12 current states and 12,000 serialized content characters total, excluding the small scope wrapper. Whole entries are omitted to fit; conditions are never cut halfway. The whole archive is never model input.

`record_updates` operations:

- add: stable kind/key/stage ID; matching key or same evidence/stage deduplicates.
- restate: existing ID/key, unchanged kind/stage/condition; preserves original wording and time.
- replace / withdraw: explicit decision relationship/version; require confirmed user anchor.
- follow_up: completion-to-completion relation; implementation, verification, merge and deployment remain distinct stages.

All completion outcomes are reports, including passed/failed. The UI follows completion descendants so later failure/rework remains visible even when the initial completion has moved into folded history. A later linked pass changes the displayed follow-up, never erases the original report/failure.

Processed source-event IDs are tombstoned after the entire successful delta batch, not after its first record. Thus one event can initially contribute multiple facts/stages, but replaying it with a different model key/quote cannot create duplicate facts. The first accepted extraction is not automatically supplemented by re-summarizing the same event; new evidence is needed for later changes. Preview never marks this set.

First evidence quote is the timestamp/order anchor; additional quotes are support. Every reference must resolve to a supplied event and every exact short quote must occur in that event. Decisions require a user anchor; obvious proposal/question quotes are rejected. Dates are not accepted from the model. These structural checks do not prove semantic entailment; model quality remains a separate evaluation gate.

Current states are keyed independently. Repeated/same state retains its timestamp. A newer batch that omits an active blocker marks its current status unconfirmed, not resolved. A resolve update requires cited evidence. Older branch-order results cannot overwrite newer current state. Decision relation_order similarly prevents stale replacements overriding newer relationships.

A cumulative commit holds the Space commit/event locks, merges under the branch journal lock, then atomically binds exact processed sequence IDs to its CurrentState in selected-current.json. current.json is a compatibility mirror. The cursor is only a contiguous processed range, not a high-sequence deletion instruction. Only selected input is removed; older pending raw input remains. Replay after a crash between state commit and inbox rewrite is filtered by these sparse receipts. An older selected batch cannot replace the newer Space display snapshot. Journal operations are idempotent if a later Space commit step fails. This is not a database-wide multi-file transaction or a formally verified arbitrary power-loss protocol.

New batches commit the newest eligible work directly, without an extra latest-preview call for the same events. Legacy latest.json remains read-compatible until v0.12 selected state exists. Existing worker recovery every 15 seconds is separate from h/b navigation; neither viewing history nor opening the date picker authorizes historical work.

## Time

UTC epoch fields explicitly use *_unix_utc. Source timestamps are never filled from now, file modification time or migration time. Unknown anchor time stays unknown even if support has a known date. Transcript reactivation preserves user/final timestamps separately; merged multiple user messages have unknown anchor time.

Journal item times are conversation-confirmation times. Journal updated_at, through-event time and GitHub fetch time are separate metadata. Rendering uses the host date utility with local TZ and includes year/date/time/offset. GNU date runs batched; macOS has a BSD date path, untested on hardware in this change. Formatting failure displays the retained UTC epoch and a conversion warning, never invents a local date.

## Files and locks

See README for the file inventory. Directories are 0700 and files 0600. The cross-Space journal contains short source quotes, not only anonymous status.

Capture order: Space participation → Space event → root origins → branch capture. Commit order: Space participation → Space commit/event → branch journal. Model inference runs outside participation locks. Branch reducer and Space worker leases prevent duplicate in-flight reductions. Explicit shared deletion locks participation gates, checks all contributing Spaces are off/queues empty, then fails fast if the reducer is busy.

GitHub links have a separate lock/cache. Only explicit `links refresh` invokes readonly gh commands, with an 8-second / 1 MiB limit per command. Rendering does not wait for GitHub or hold the links lock. Exact number/URL responses are checked before caching; failures retain previous success time and data. [gh pr list](https://cli.github.com/manual/gh_pr_list) and [gh issue view](https://cli.github.com/manual/gh_issue_view) document the metadata surfaces; no PR diff, mutation or approval API is used.

## Migration and deletion

Before the first source-changing operation, legacy current.json/timeline.jsonl are copied into legacy-snapshots/SPACE without inferring facts or times. Future legacy-only imported summaries append snapshots there. The TUI shows parseable summaries as unlinked; memory legacy also returns original files, including data not parseable as individual summaries. Original files are not rewritten during the copy.

Space clear removes private queue/summary/timeline/provenance, its legacy archive and its private unlinked visible records; shared branch contributions survive. Shared clear requires the exact scope key repeated with --confirm-shared, all contributing Spaces off and no queued input for that scope. It clears shared records/states/links, retaining dedup/order/operation tombstones and scope identity. It does not delete other Spaces' snapshots or Codex transcripts. There is no built-in undo.

No age/count expiry applies to stored journal records. History pagination only bounds the rendered previous-history segment. File parse/write and record traversal costs still grow with retained history; this release does not introduce indexing or compaction.
