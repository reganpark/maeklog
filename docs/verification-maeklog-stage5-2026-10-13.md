# Maeklog language and menu verification — 2026-10-13

This record covers selectable app language, active menu shortcuts, the matching
source backport to the existing implementation, and isolated verification.
It supersedes the automatic-only language and key-free menu behavior in the dated
[stage 4 record](verification-maeklog-stage4-2026-10-13.md); that history and its
capture assets remain unchanged.

## Behavior

The language menu offers **자동 / Auto**, **English** and **한국어**. Auto is the
default; it uses the first nonempty `LC_ALL`, `LC_MESSAGES`, then `LANG`. Korean
locales resolve to Korean, and English, unsupported, C/POSIX or unset locales to
English. Explicit choices override Auto. Separate desktop preferences and
`LANGUAGE` are not queried. Auto's system locale is cached per process; changing
the environment requires restarting that process, while an app preference change
does not.

`language-settings.json` stores an object such as `{"language":"auto"}` with
`auto`, `en` or `ko` as the language value. It uses the same app
configuration-directory rules as model settings, with its own lock.
Preferences are shared by panels using that directory. Saving immediately
refreshes fixed text in the current panel; other panels observe it on their
approximately one-second poll. Invalid, oversized or nonregular preference files
and lock/write failures retain existing bytes and the last valid UI language.
Saving refuses to overwrite malformed preference data.

No summary runs on a language save. New live requests and completed-turn model
invocations freshly capture the resolved `ko` or `en` execution language; queued
and running work remains frozen. Same-snapshot reuse compares the resolved
language, so Auto and an explicit choice resolving to the same language can reuse
cached work. Original user text, quotes, evidence and saved summaries stay verbatim.

An open reader rebuilds its labels from its captured source snapshot, preserving
category, capture time and logical position rather than pulling newer journal
content into the frozen view. A language change clears text selection visibly;
transient notices refresh while input drafts remain intact.

The menu orders **View saved records**, **Summary model…**, **언어 / Language…**
and **View current progress** first. Its separate right-aligned shortcut column
advertises active `h/l/e/o/s/b/r/v/q` actions. These keys work inside the menu;
`q` hides the panel and `Esc` closes only the menu. Model, language and progress
rows use arrows and Enter without additional shortcuts. Narrow/short controls
retain explicit save/cancel, and actions that cannot be rendered safely are guarded.

## Source and isolated checks

The verified Maeklog release has SHA-256
`85d3b99208c5c05a48c64f51df6d69b8bc387037e848e606797bd8f95463681c`.
The independently built existing-namespace release has SHA-256
`8631390d5b32f9f7964b8c0c5a4cdebd59787805f1185e248ef3d5ad9ec633c6`.
Fixtures use isolated state/configuration and synthetic services.

| Check | Maeklog | Existing-namespace source |
| --- | --- | --- |
| Formatting and strict all-target Clippy | PASS | PASS |
| Full Korean unit suite | 270/270 PASS | 268/268 PASS |
| Full English/C.UTF-8 unit suite | 270/270 PASS | 268/268 PASS |
| Locked release build | PASS | PASS |
| Python model/language settings and execution profiles | PASS | PASS |
| Two-panel language picker PTY | PASS; 14 captured frames | PASS; 10 captured frames |
| Menu, model picker and reader-clarity suites | 3/3 PASS | Stage 4 baseline; menu/reader also exercised by language PTY |
| Selected collection/routing regressions | 6/6 PASS | Stage 4 full regression baseline retained |

Each profile fixture uses five live and one completed-turn fake model execution.
No actual inference was performed. It checks explicit overrides, missing Auto,
installed-hook and CLI settings coherence, malformed-file preservation, no model
call on save, same-effective cache reuse, queued-language freezing, fresh daily
language and stale-result protection.

The source backport changes the same twelve Rust paths in both implementations,
including two new language modules. Exact three-way deltas preserve the existing
implementation's binary/environment/plugin identity, canonical settings fallback,
hook/state paths and relay marker. Audited manifest, lockfile, schema and hook
files remain byte-identical to the original HEAD. No dependency change, broad
rename or state migration is required.

Detailed private evidence is retained under `$VALIDATION_ROOT` in
`system-locale/`, `pm-model-settings/`, `maeklog-ui/`, `regressions/` and the
individual picker/profile result directories. These artifacts are not checked in.

The six selected regressions cover client routing, structured task reports,
read-only record viewing, live progress, daily policy and work links. They are
additional to the menu/model/language/clarity fixtures above. The full unrelated
regression baseline from stage 4 was not repeated for the existing namespace.
Its new language and model-profile suites verify the namespace-specific port.
The existing-namespace language run predates four capture-only additions; that
final instrumentation was also ported and syntax checked.

The first clarity run observed resize and mouse-selection gestures before the
required completed frames. The fixture now waits for the completed footer, Home
repaint, mouse-down selection and reversed selected cells before release. The
unchanged production binary passes the corrected suite; content, scope and copy
assertions remain intact. The initial failure is retained separately. Rendered
menu, language, shared-evidence and reader-confirmation sheets were independently
reviewed. Full unit results use serial runs; the previously observed parallel
lock timing limitation remains documented in the stage 4 record.

## Runtime boundary

Only the owned Maeklog preview pane was replaced with the exact verified release.
Its actual menu exposed the language row and all three choices, and Esc closed
it without saving a preference or requesting a summary. Independent checks
confirmed the installed executable hash, preserved preferences/source content,
unchanged focus and unchanged Herdr key configuration. Prefix+m and
prefix+Shift+m continue to open/hide Maeklog.

All ten existing panels retained their process identities. The original binary,
hook, Codex integration and collection routes remain unchanged. The existing
running installation is not upgraded by this source backport, and its records
remain in place. The two implementations keep independent settings/state
identities. No real-model evaluation, hook handover, record migration, Git commit,
push or publication is included. Native macOS execution and terminal clipboard
delivery remain outside these checks.

Runtime evidence is retained privately in `backend-review/actual-ui.result.json`
and `root-runtime-final-audit.json` under `$VALIDATION_ROOT`. Historical capture
assets were not regenerated.
