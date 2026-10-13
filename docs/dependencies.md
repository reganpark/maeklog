# Dependency decision

> Historical dependency review from the original implementation. The dated audit and maintenance findings below are not a fresh advisory scan for Maeklog. Current-copy checks are recorded separately in [preparation verification](verification-maeklog-stage2-2026-10-13.md).

Reviewed on 2026-09-04 against crates.io metadata, the generated dependency tree, and the RustSec advisory database.

## Direct runtime dependencies

| Crate | Locked version | Purpose | Decision |
|---|---:|---|---|
| `serde` | 1.0.229 | Typed state and model schema | Keep; current, narrowly scoped, exact version |
| `serde_json` | 1.0.151 | JSON files, hooks, Herdr responses | Keep; current, narrowly scoped, exact version |
| `crossterm` | 0.29.0 | Raw terminal, keys, cursor and color | Keep with default features off and `events` and `bracketed-paste` on (paste is treated as text, not save/shortcut keys) |
| `unicode-width` | 0.2.2 | Correct Korean terminal-cell wrapping | Keep; small and narrowly scoped |

The locked Linux runtime graph contains 35 third-party packages and no duplicate versions. It contains no Git dependency. `cargo-audit` 0.22.2 scanned the lockfile against 1,239 RustSec advisories with `--deny warnings`; it reported no vulnerability, unmaintained-package, unsoundness, or yanked-version finding.

All resolved licenses are MIT, Apache-2.0, Unicode-3.0, Unlicense, or compatible combinations. This is a compatibility check, not legal advice.

## Rejected dependencies

| Crate | Reason |
|---|---|
| `fs2` | Latest release is 0.4.3 from 2018; Rust 1.89+ provides `File::lock` and `File::try_lock` directly |
| `tokio` | A full async runtime is unnecessary for one worker and a polling terminal panel |
| `clap` | The fixed Herdr manifest exposes only a handful of internal subcommands; manual parsing is smaller |
| `ratatui` | The panel is narrow and read-mostly; direct `crossterm` rendering avoids another UI abstraction |

“No known advisory” is not a proof of safety. The project additionally limits permissions, pins checksums through `Cargo.lock`, forbids `unsafe` in first-party code, caps input/output sizes, and treats all conversation text as untrusted model evidence.
