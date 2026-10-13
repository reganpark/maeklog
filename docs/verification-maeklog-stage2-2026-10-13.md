# Maeklog local preparation verification — 2026-10-13

This record covers the locally prepared Maeklog source copy. Dated historical records elsewhere describe the original implementation and are not reused as proof that renamed artifacts were installed or tested.

## Changes

- Product and executable contracts use Maeklog / 맥로그, `maeklog`, `maeklog-hook`, `maeklog-update`, `codex-maeklog` and `MAEKLOG_*`.
- The Maeklog config/state/share/cache namespace is independent. No existing installation, user hooks, configuration or state was migrated.
- Public prose and terminal fixture sources use generic paths, project labels and recorded runtime identity placeholders. Official upstream links and attribution are retained.
- Historical tests and installation claims are explicitly labeled. Private historical validation artifacts are not bundled.

## Checks performed for this copy

The Maeklog release binary used for the isolated checks has SHA-256
`b4cd13b37c62d994984befa67aa1b8c68a74448a56b18bbdca568eb1735c67ea`.

| Check | Current-copy result |
| --- | --- |
| `cargo fmt --all --check`, strict Clippy, locked release build | PASS |
| Rust unit tests | 233/233 PASS |
| Python installer, launcher and updater tests | 18 + 11 + 13 = 42/42 PASS |
| Isolated Node integration scripts against the candidate binary | All 15 PASS across the initial run and focused reruns |
| Five versioned Codex patches | 5/5 apply/reverse-apply and added Rust bridge formatting checks PASS; source checks only |
| Isolated hook installation | Three cases PASS: default home, XDG paths, explicit paths with spaces/apostrophes; precedence, idempotence, unrelated hooks and existing state preserved |
| Public source privacy scan | 217 files, including 200 text files and 17 PNGs reviewed; no known personal namespace, owner handle, personal home path, recorded real runtime IDs, private project labels or real credential signatures found |
| Documentation links, whitespace and file modes | No broken local links, trailing whitespace or unexpected modes |
| Terminal capture reconstruction | 25 ANSI sources and 10 text variants preserved; all 17 PNGs re-rendered and visually reviewed before and after reconstruction, with independent final review |
| Capture geometry and metadata | All 17 original PNG dimensions preserved; no ANSI width overflow; PNGs contain only `IHDR`, `IDAT` and `IEND` chunks |
| Preservation | Original source revision and clean state unchanged; installed configuration, hooks, launchers and hook binary hashes/resolved paths unchanged; third-party dependency graph unchanged |

The first Rust test run encountered one transient `Busy` lock failure. The focused
check and an unchanged full rerun passed. The integration run exposed stale brand,
footer-key and asynchronous reader traversal assumptions in fixtures. Focused
retests passed after the fixtures were aligned with the existing full-text UI and
waited for complete UTF-8 PTY frames and actual category changes. UI clarity's
final retest checked all nine reader categories, oldest retained records, gap reason
and approval condition. These fixture corrections did not change product behavior
or require a new binary build.

The bundled screenshots remain historical reconstructions. Generic labels required
limited reflow in four narrow ANSI variants; two of those variants have PNGs.
Early default-color frames now use the renderer's Gruvbox palette. Fourteen PNGs
changed bytes and three are byte-identical after re-rendering. No image was removed
or original PNG pixel data edited. See [capture provenance](captures/README.md).
Official upstream links remain intact; an older README link was relocated into the
anonymized update history. Historical advisory audits and native Codex results are
not counted as fresh checks for this copy.

## Boundary

No Git initialization, commit, publication, global installation, state import or
live runtime replacement was performed. No renamed native Codex build or live
Herdr lifecycle test was performed in this stage; those checks and later migration
remain separate work. A public repository URL, security reporting contact and
copyright attribution have not been invented.
