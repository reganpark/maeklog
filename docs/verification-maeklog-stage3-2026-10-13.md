# Maeklog model settings and system locale verification — 2026-10-13

This record covers the local Maeklog source copy and isolated tests. Historical
verification and capture files are not reused as evidence for these changes.
Functional verification and final source privacy/preservation audits are complete.

## Changes

- The Maeklog menu adds **요약 모델… / Summary model…** with model selection,
  supported reasoning efforts, explicit save and cancellation. Settings apply to
  the shared Maeklog configuration directory; Codex's general configuration is not
  rewritten. Opening or saving this screen does not submit a summary request.
- `maeklog model show`, `model list` and `model set MODEL [--effort EFFORT]` expose
  the same settings. An absent file preserves `gpt-6-luna` / `medium` / Standard.
  Catalog failures, unavailable selections and invalid effort values retain the
  current settings instead of substituting a default.
- Fixed app labels, notices and help follow the process system locale
  automatically: first nonempty `LC_ALL`, then `LC_MESSAGES`, then `LANG`.
  Korean locale tags use Korean; English, unsupported, C/POSIX and unset locales
  use English. No language setting or switch was added. `LANGUAGE` and separate
  desktop language preferences are not queried, including on macOS. The language
  stays fixed for the running process.
- Existing user text, saved summaries, evidence quotations, titles, Git references
  and identifiers retain their original language and contents. New summary prompts
  request the automatically determined language while retaining original evidence.
  Korean intent-parsing patterns and old `지금` scheduling metadata remain valid;
  English `now` is supported with the same scheduling contract.
- Completed-turn inference freezes model, optional effort and language at launch.
  Explicit live-summary requests freeze the same profile at request time. Cache
  reuse requires an identical snapshot and profile; unique request identity prevents
  a stale result from replacing a newer request, including A → B → A changes.
  Ready legacy results remain readable but are not reused for a new request.

## Checks performed for this change

The final release binary has SHA-256
`2719600b1da42557af46e666d68471e9adb9ae4bfde540ab820e1000de224268`.
Final catalog and fake-runtime checks used this same binary.

| Check | Observed result |
| --- | --- |
| Final-source `cargo fmt --all --check` | PASS |
| Final-source strict Clippy, all targets | PASS |
| Full Rust unit suite, `LC_ALL=ko_KR.UTF-8` | 255/255 PASS; 5.70 seconds |
| Full Rust unit suite, `LC_ALL=C.UTF-8` | 255/255 PASS; 5.68 seconds |
| Existing isolated Node integration scripts | 15/15 PASS on the preceding release checkpoint; SHA-256 prefix `832cc54d` |
| Final locked release build | PASS; 87.04 seconds |
| Installed Codex read-only catalog smoke | PASS; 7 models with supported/default efforts; 0.14 seconds; no stderr, inference or settings save |
| Final model-profile/runtime checks | PASS; 5 live-summary and 1 completed-turn fake Codex executions; zero actual model inference |
| Final picker PTY and visual checks | PASS in Korean and English; 22 total rendered frames independently reviewed on the final binary |
| Final source privacy/document audit | PASS; 227 files reviewed (210 text and 17 unchanged PNGs), including raw text/comments; no known personal namespaces, private project labels or actual credential signatures/values found |
| Original installation/source and historical capture preservation | PASS; original source HEAD/clean state retained; 4 installed config/hook/launcher files retain hashes and resolved paths; historical captures unchanged |
| Dependency, links and tree checks | PASS; Cargo.lock unchanged, Cargo.toml description only; 0 broken local Markdown links, changed existing modes, added-line trailing whitespace or removed files |

The picker review includes loading, model selection, supported reasoning efforts,
no-effort selection, stale catalogs and an unlisted current model, an empty catalog, short terminal
height and the end of a longer model list. Both confirmation mosaics were reviewed
independently; controls, layout and state copy were accepted. The captured frames
use isolated synthetic data, not user conversations or historical capture assets.

The same final source passed both full parallel unit suites. One earlier Korean
parallel run reported `Busy` instead of the expected `Refreshed` in
`task_links::tests::lookup_failure_retains_last_success_and_backoff_skips_futile_calls`.
The unchanged isolated retest passed in 0.13 seconds, followed by the unchanged
full Korean and English passes above. The product cause of that intermittent
observation was not established.

The full 15-script Node regression baseline was run before the final changes to
new model-catalog/picker modules and test expectations. Final focused checks
validate those new modules on the final binary; the earlier full baseline is
reported as a separate checkpoint rather than relabelled as a final-binary run.

English assertion fixes distinguish app-authored labels from Korean fixture
content and normalize only whitespace introduced by terminal wrapping. Existing
checks of scope, counts, source conditions, complete body content and terminal
cell widths remain in place. The failure/revalidation test explicitly checks the
entire original Korean evidence after wrapping.

Installed catalog discovery initially rejected the built-in public ChatGPT base
URL emitted by `config/read`. That field was a serialized upstream default, not
custom user configuration. The guard now accepts the verified exact default
`https://chatgpt.com/backend-api/`, with or without its final slash, and retains
rejection of divergent/custom endpoints and incompatible catalog/provider
configuration. A dedicated unit test covers this compatibility correction.
Catalog metadata may be bundled or cached; it is not an account entitlement or
inference-success check.

The privacy review inspected four named-credential candidates as synthetic test
fixtures, and nine nonhex high-entropy candidates as generic paths or a base64
alphabet. No actual credentials were identified. The candidate contains no
`.git`, build `target`, `__pycache__` or `.codex` directory. Private QA logs,
synthetic PTY images and preservation manifests remain outside the public source
copy; dated historical capture assets retain their existing bytes.

## Boundary

No real model inference, global installation, hook registration, live panel
replacement, state migration or publication was performed for this change. The
Maeklog source copy was not initialized or committed as a Git repository. The historical ANSI/text/PNG capture assets were not
regenerated. macOS native execution and real-model output quality remain outside
these checks. The terminal/process locale contract is documented in
[README](../README.md), with interaction and persistence rules in the
[panel contract](ui-contract.md) and [security policy](../SECURITY.md).
