# Branch memory implementation plan

> Anonymized historical reference from the original implementation. Names, paths and runtime IDs are generic substitutes; Maeklog names and paths do not denote an actual historical Maeklog installation. Reported tests, hashes and installations belong to that original version and do not verify or install this copy or supply its current file checksums. Private validation artifacts are not bundled.

Scope authorized by the user's 2026-09-14 request: local implementation, non-model tests and documentation only. No AGENTS.md was found in this repository or its ancestor directories. Existing checks are formatting, Clippy, tests and security review; no additional pre-implementation approval gate was found. No installed binary/hook replacement, live model evaluation, remote mutation, merge or deployment is authorized.

1. Stamp new events with capture-time repository/worktree/branch identity and source provenance. Keep Space participation separate from a shared branch journal. Ambiguous/legacy records remain unlinked.
2. Maintain durable decision/completion changes separately from current progress/blockers/actions. Validate evidence locally, retain ids/relations and original evidence times; never infer deletion from omission.
3. Add branch/issue/PR context, recent three completions and a keyboard-only history fold to the existing panel. GitHub is explicit, read-only and cached, never queried by rendering/folding.
4. Test with temporary Git repositories/worktrees, mock model/GitHub processes and real PTYs. Preserve legacy files; document shared deletion scope and capture gaps. Produce default/expanded panel captures without replacing live panels.

Identity policy: canonical Git common directory groups local worktrees. Branch name plus observable reflog incarnation separates renames/recreation conservatively; unavailable/ambiguous incarnation is unlinked. Separate clones and unresolved legacy contexts are not automatically merged. No model-generated dates or file mtime substitution for evidence timestamps.
