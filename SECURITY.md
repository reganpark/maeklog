# Security policy

Maeklog processes private prompts, final agent responses, repository metadata, and locally generated summaries. Treat its state directory as sensitive terminal history.

Maeklog uses its own plugin/config/state/share/cache namespace. A new installation does not discover or import another installation's records. Explicit state migration requires a user-supplied source path, stopped writers, a private backup, ownership/path checks and preserved tab attribution. No personal legacy namespace is shipped as a fallback.

Older dated verification documents describe anonymized history from the original implementation. Recent Maeklog verification records state their tested scope separately. Synthetic captures and isolated installation tests do not establish a clean-machine build or native support on every declared platform; see [release readiness](docs/release-readiness.md).

## Runtime boundary

- The Herdr plugin does not install Codex hooks automatically.
- Hook installation backs up `hooks.json`, preserves unrelated handlers, and requires Codex trust review.
- The UserPromptSubmit hook can supply a scoped task-report producer instruction and capability through Codex additionalContext. It does not inject stored project-memory text. Other supported lifecycle hooks emit no model context. Maeklog's own summary process is ignored.
- Producer instructions and structured report calls can add context tokens and tool activity to the user's normal Codex turn. Summary inference has its own selected model/effort profile and may incur additional usage.
- Codex hook approvals are a manual /hooks boundary. The updater's local preflight compares all four enabled hook definitions with Codex's current canonical trust hashes; it never writes or refreshes an approval hash.
- The checked client installer backs up replaced launchers and configuration, preserves unrelated hook definitions and trusted hashes, and disables only an unambiguous generated Herdr SessionStart handler. Multiple or ambiguous Herdr handlers require review before installation. A fresh home may have no such handler or local Codex launcher.
- The selected summary-model process is ephemeral, read-only, schema constrained, time limited, and launched with user configuration and hooks disabled.
- State directories are `0700`; state files and backups are `0600`.
- Live command progress excludes the entire command string, including arguments, environment assignments and executable paths. The receiver ignores that field from older clients too. This does not redact secrets repeated in user prompts or assistant prose, or erase previously collected records.
- HTTP Git remotes have embedded user information removed before model input.
- Raw completed inbox payloads are deleted after a successful cumulative commit. Short exact evidence quotes and Space/session/context provenance remain in the shared branch journal until explicit scoped deletion.
- v0.12 does not automatically import Codex transcripts on first open/focus/session connection or reactivation. Legacy bounded importers remain compatibility test code. Only collected complete turns wholly within today's local date are automatic; previous/unknown/cross-day input requires explicit date selection.
- Reactivation allows at most three already-collected today turns. Other collected input is retained for manual selection, not silently deleted or caught up. No opted-out transcript is automatically backfilled.
- Explicit historical requests freeze event IDs and participation generation. Navigation/folding does not authorize model work. Automatic in-flight work has a local-midnight cancellation deadline; already sent requests cannot be recalled.
- A stale or malformed model result cannot replace the current revision.
- Code-level guardrails reject transient objectives and downgrade assistant-only completion claims even when model output passes the JSON schema.
- The journal validates supplied event references, exact source quotes, decision user anchors, record relationships and source-derived timestamps. Structural validity does not establish semantic truth or independent verification; all completions remain reports.
- v0.12.3 source uses schema v4: new decisions require a model-classified explicit choice/approval basis anchored to exact first-user evidence; conditional approval requires a condition. Punctuation is not semantic authority. Model misclassification remains possible and needs separate quality evaluation. Existing records with no basis remain readable without inferred metadata. A bounded validator error string, not the rejected raw-response diagnostic, is supplied to the same work key's existing retry; two consecutive identical validation errors stop automatic retries, within the old three-attempt ceiling.
- Development diagnostic capture retains at most one rejected response per Space in `reducer-failure.json` (0600, maximum 2 MiB). It includes only selected collected events as sent to the model, their original forms, and the final response, not reasoning/tool output/stderr or a full transcript. Oversized/non-UTF8 payloads are omitted explicitly, never silently truncated and labelled replayable. Successful work does not erase the latest failure; the next failure replaces it. Space clear removes it. Never publish this file. Recording holds the participation guard so opt-out/clear cannot resurrect a payload. The diagnostic is not fed into retries or committed as memory.
- Pre-v0.14 same-repository/same-branch Spaces shared recorded decisions, completion reports and short evidence quotes. Those archives remain intact; v0.14 tab isolation below governs new collection.
- GitHub link lookups are read-only and may refresh in the background for the current task or observed branch. Authentication/network errors retain cached data with a visible failure marker and original successful fetch time. No diff verification or remote write is performed.
- Space deletion does not delete shared branch contributions. Shared deletion requires an exact-key confirmation and stopped/drained contributing Spaces. Scope/order/dedup tombstones are retained; source transcripts and separate Space snapshots require their own deletion scope. No built-in undo is provided.

## Summary configuration and locale

- Maeklog model/effort settings live in its own configuration directory and never rewrite Codex's general settings. The private settings file is lock-protected and replaced atomically. Missing settings use the original gpt-6-luna/medium default. Malformed settings and attempts to save unsupported selections fail without substituting another model.
- Catalog discovery is a bounded separate Codex app-server using initialize, narrow config/read and paginated model/list. It does not submit an inference request. Custom catalog/provider/base-URL configuration that differs from isolated summary execution fails closed, retaining current settings. A listed model is not proof of account access.
- Saving settings causes no replay, summary request, record rewrite or language conversion. Running inference retains its captured model/effort/language. Explicit live requests carry a unique request identity so older results cannot replace newer requests or reuse a cache under a different profile.
- App language preference is Auto (default), English or 한국어, stored privately and atomically in language-settings.json under its separate lock. Invalid/oversized/symlink settings and lock/write failures preserve existing preference bytes and the last cached UI language; save rejects malformed existing data.
- Auto follows LC_ALL, LC_MESSAGES, then LANG; explicit choices override the resolved language. No direct desktop-language lookup is performed. Fixed UI text refreshes across panels; source evidence and saved user/model text remain verbatim. Inference profiles freshly resolve the preference at invocation/request and retain that resolved language while queued or running. Changing preference does not replay or translate existing records.

## Tab isolation (v0.14)

- New runtime collection, queues, participation and UI identity use Herdr session + workspace + tab. Durable memory additionally retains repository/branch identity. Same-branch tabs do not share new journals, links or corrections.
- Hook routing prefers the caller's live pane identity; it never chooses an unrelated focused tab as source. Pre-tab queues/journals remain separate and are not reassigned or automatically backfilled into new tabs.
- Existing Space opt-out is inherited when a tab store is first created. Explicit open only reactivates that tab. Pre-tab mixed queues stop automatic processing once the workspace has tab stores.

## User corrections (v0.13)

- Local panel actions can amend the interpretation text/condition or exclude an erroneous decision/completion. They do not grant execution authority, reclassify a completion as independently verified, or alter source quotes, timestamps, stage or outcome.
- Every write requires a nonempty reason and an explicit before/after confirmation that names the shared branch scope. Corrections and undo entries remain in the same private journal under its existing atomic-write lock. Same-scope Spaces read the same overlay.
- A journal revision check rejects stale previews. A separate correction revision binds each model invocation to its input; a response from before a correction cannot commit, and that superseded attempt does not consume the existing failure retry allowance.
- Processed-event tombstones prevent replay of the same evidence under a new key. Excluded records cannot be model relation targets. Semantic mistakes from genuinely new evidence remain possible; this does not introduce an automatic quality guarantee or archive retrieval.
- Undo restores the preceding user overlay, retaining its audit history; it does not undo later real-world decision changes. Scoped record deletion removes correction history together with that scope's records.
- Do not run old journal writers alongside v0.13 after corrections have been saved: older binaries do not preserve these new fields. Drain workers before upgrading. Downgrade requires a full compatible state restore, not just swapping the executable.

## Dependency policy

- Direct dependency versions are exact and `Cargo.lock` is pinned.
- Registry packages only; no Git dependencies.
- No `unsafe` is allowed in Maeklog source.
- Default features are disabled for `crossterm`; only terminal events and bracketed paste are enabled.
- `cargo clippy`, tests, release build, and RustSec audit are release gates.
- Dependency additions require a purpose, current maintenance evidence, license review, transitive-tree review, and a clean advisory scan.

## Reporting

Report suspected vulnerabilities through [GitHub's private vulnerability reporting form](https://github.com/reganpark/maeklog/security/advisories/new). Include the affected version, a minimal reproduction and the expected impact. Remove credentials and personal conversation data from reports whenever possible.

Use [GitHub Issues](https://github.com/reganpark/maeklog/issues) for ordinary bugs and platform testing feedback. Do not publish prompts, state directories, hook configuration backups or private terminal captures in an issue. The older bundled synthetic captures are anonymized historical reconstructions, described in [capture provenance](docs/captures/README.md). Current synthetic demo assets state their own provenance in the user documentation.
