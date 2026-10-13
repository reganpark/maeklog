# Today/manual policy plan — 2026-09-14

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

User approved implementation and local application after explicitly waiving a separate record backup. This supersedes the older restriction on replacing the running Maeklog binary, but does not authorize remote changes, merges, publishing, or live-model evaluation.

1. Stop only previously enabled Maeklog collection/workers and record participation/visibility metadata in session memory. Preserve already-off Spaces, original dialogue and journals; do not create a record backup.
2. Limit automatic work to collected completed turns wholly within the local current date. Process newest bounded batches first. Previous/unknown/cross-midnight dates need an explicit dated request, frozen to current event IDs.
3. Commit sparse selected IDs atomically with state; never treat a high sequence as permission to delete older raw input. Retain record provenance/time/relations and stop automatic work across midnight.
4. Add b / Maeklog menu date selection and visible queue/failure states. Keep h strictly read-only. Reactivation permits at most three collected today turns; others remain manual, not deleted.
5. Run non-model regressions, PTY/date tests, release checks and RustSec audit, then atomically replace the main and hook binaries at their existing paths without hook re-registration. Restore only previously enabled/visible Maeklog state.

No applicable AGENTS.md was found in the repository or ancestors. Herdr environment and current CLI were verified. Impeccable hardening preserves terminal style and separates date eligibility from processing failure. Older whole-transcript import/duplicate latest-preview producers remain compatibility test code only; they are not production scheduling routes.
