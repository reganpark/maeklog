# Maeklog product brief — v0.11.0

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

For a user returning to a branch after working elsewhere, the existing Maeklog panel should answer: what is decided, what was reported complete, which stage is current, what blocks progress, who acts next under which conditions, and which issue/PR the work belongs to.

The durable unit is repository + branch incarnation, shared across Spaces/worktrees. Collection participation remains per Space. Existing hook scope, fixed Luna model, concurrency/retry limits and recent-three-turn reactivation are retained.

Decisions/completions are cumulative evidence-backed records. Current progress/blockers/actions are a compact state summary. Folding is not deletion. No record expires because it was not mentioned again, the pane closed, the user changed branch/session, or its implementation was reported complete.

The surface is read-mostly and local-first. It neither injects summaries into the main Codex conversation nor executes or approves next steps. GitHub metadata is read only on explicit refresh and never claims diff verification.

The first version favors conservative unlinked preservation over speculative branch/history merging. Tests use mock model/GitHub replies and isolated real terminal input; running-plugin replacement and live-model quality evaluation are separate user decisions.
