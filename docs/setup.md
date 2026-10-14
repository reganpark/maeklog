# Set up Maeklog from source

[English](setup.md) · [한국어](setup.ko.md)

[← Back to the overview](../README.md)

Maeklog is available as source in [reganpark/maeklog](https://github.com/reganpark/maeklog), with automatic discovery enabled in [Herdr Marketplace](https://herdr.dev/plugins/). The marketplace index refreshes every 30 minutes. This guide describes manual setup from a local checkout; installation still requires the hook approvals and integrated Codex client below.

The order is: build and link Maeklog → register and trust the four hooks → check prerequisites → build the integrated Codex client → check a new conversation. The client patch supplies the tab identity and public progress the panel needs.

## Before you start

| Requirement | Details |
| --- | --- |
| Operating system | Native validation covers Linux x86_64. macOS is an experimental target and has not been verified on real hardware. |
| Herdr | 0.8.2 or later. Run shell commands in a regular shell inside Herdr. |
| Codex | A signed-in official CLI with hooks enabled. The current integration patch targets 0.162.1. |
| Build tools | Python 3.11+, Git, GitHub CLI (`gh`), Rust/rustup, `just`, `cargo-nextest`, Node.js/npm, and Bazel 9.2.0 or Bazelisk providing a `bazel` command on PATH. Maeklog pins Rust 1.97.1; the integrated Codex build uses its upstream source's toolchain. |
| Terminal | UTF-8 display. Mouse and clipboard features depend on terminal support. |

Allow time and disk space for source builds and dependency downloads. Local linking does not run the build for you.

Codex 0.162.1 pins **Bazel 9.2.0** in its source. Bazelisk can select that version automatically; a fixed `bazel` executable must report the matching version. Preflight checks that Bazel is available, and the source-build step checks its exact version.

Summaries consume **additional model usage**, with **no monetary budget cap**. Records are stored locally, but selected conversations and a limited index of existing records are sent to a model service through Codex. Hiding the panel keeps collection running; choose **Stop recording in this tab** in the menu to stop collection and summaries. See [storage and privacy](#storage-and-privacy).

## 1. Build and link Maeklog

In a **regular Herdr shell**, clone and build the source, link the checkout, then install its collection hooks:

```sh
git clone --branch v0.14.0 --single-branch https://github.com/reganpark/maeklog.git
cd maeklog
cargo build --locked --release
maeklog_root="$PWD"
herdr plugin link "$maeklog_root"
herdr plugin action invoke install-codex-hooks --plugin maeklog
```

Keep this checkout available after setup: the updater uses its scripts and patches. Keep using the same shell through step 4 so `maeklog_root` remains set. If you return later, set it again to the full path of this actual checkout.

The command above selects the **v0.14.0 public preview**. To work on development changes instead, clone `main` explicitly; its contents may differ from the released version.

The hook action creates the Maeklog hook executable and runtime configuration, and registers four handlers in Codex's `hooks.json`. It backs up existing hook files before changing them and preserves other handlers. It does not grant hook trust.

## 2. Review and trust the Codex hooks

If hooks are disabled, enable them in the shell and open a new Codex session:

```sh
codex features enable hooks
codex
```

**Inside Codex**, open `/hooks` and review and trust **all four Maeklog handlers**:

| Event | Collected information |
| --- | --- |
| `SessionStart` | Session connection |
| `UserPromptSubmit` | User request |
| `Stop` | Final response |
| `Interrupt` | Interruption |

Each handler runs `bin/maeklog-hook` in Maeklog's configuration directory, with `hook` as the final argument. If the source or command changes and the trust is no longer valid, review the new definitions in `/hooks` again. The installer does not approve them on your behalf.

Finish active work and exit Codex normally before continuing.

## 3. Check the prerequisites

Return to the same **regular Herdr shell**, outside the Codex input screen:

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" --preflight
```

`--preflight` checks the required commands, Herdr context, installed runtime configuration and hooks, hooks activation, and manual trust for all four current definitions. It does **not** download, build or call a model. Resolve any reported failures and run it again. `--check` only checks version information and does not replace this prerequisite check.

If you customize `XDG_CONFIG_HOME` or `CODEX_HOME`, keep the same environment for hook installation, trust, updates and Codex execution. A custom `HERDR_PLUGIN_CONFIG_DIR` outside the bridge path `$XDG_CONFIG_HOME/herdr/plugins/config/maeklog` is not supported by this installation flow; preflight stops rather than guessing. Without an XDG setting, the default is `~/.config/herdr/plugins/config/maeklog`.

## 4. Build and activate the integrated Codex client

Run in the **regular Herdr shell**:

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" 0.162.1 --apply
```

The updater downloads the official Codex source and matching helper/resource packages, applies the Maeklog patch, builds, runs validation, then connects the launchers. Validation uses local response fixtures and a temporary Herdr pane; it does not call an external model. If prerequisites or validation fail, the updater does not replace the active installation.

To build and validate without activating the installation, omit `--apply`. This still downloads, builds and validates:

```sh
python3 "$maeklog_root/scripts/update-codex-maeklog.py" 0.162.1
```

Activation backs up the existing executables, launchers and configuration. If an older Herdr server-side `SessionStart` handler is present, it disables only that confirmed handler and uses the Maeklog terminal client's reports. It does not restart existing Codex processes or the shared Herdr server.

### Check the launcher in a new shell

Open a **new regular Herdr shell** and put `~/.local/bin` first on PATH:

```sh
export PATH="$HOME/.local/bin:$PATH"
command -v codex
command -v codex-maeklog
codex --version
codex-maeklog --version
```

The paths should be `~/.local/bin/codex` and `~/.local/bin/codex-maeklog`. Both should report `codex-cli 0.162.1`. The official CLI reports the same version, so also check the paths, preflight and first conversation below.

The PATH change applies to the current shell. To retain it, add the same `export` line to your shell's startup file, such as `~/.bashrc` for Bash or `~/.zshrc` for Zsh, following any login-shell configuration you use. If a shell remembers an earlier executable, open a new terminal or refresh its command cache.

`codex-maeklog` is a package pointer for path and version diagnostics. **Use the installed `codex` launcher for interactive work.** If PATH selects another command, run `~/.local/bin/codex` directly. If that file itself has been overwritten, use the installed launcher script:

```sh
python3 "$HOME/.local/share/maeklog/codex-launcher.py"
# Resume the same conversation:
python3 "$HOME/.local/share/maeklog/codex-launcher.py" resume
```

Existing Codex processes keep their original code. Finish active work, exit normally, then resume with the new launcher.

## 5. Check your first conversation

In a regular Herdr shell in your project tab, open the panel:

```sh
herdr plugin action invoke open --plugin maeklog
```

Start `codex` or `codex resume` from that shell and exchange a short, nonsensitive request. This can incur additional summary usage when the response ends.

Look for public commentary in the current-progress area, then check the saved-record update status after the response ends. A response without public commentary may show **No progress information**. A turn may contain no confirmed decision or completion report, so a new record is not guaranteed.

Click the panel to focus it. Press `h` to browse records, and `e` to inspect evidence when an item exists. Use `r` if today's completed turns are still pending.

Earlier conversations and conversations from periods when recording was off are **not imported automatically**. A response ending is not proof that the task is complete.

## Configuration

Open the panel menu with `m`, `F10` or a right-click.

- **Summary model:** choose the model and supported reasoning effort. The default is `gpt-6-luna`, effort `medium`, Standard service.
- **Language:** choose Auto, English or Korean. Existing records are not translated again; source quotations retain their original language.
- **Issue and PR links:** review the current conversation's references; use `l` to correct a link and `o` to open it.

Changing model or language settings does not run a summary or rewrite existing records. Changes apply to future summaries. Summaries of completed turns and requests made with `r`, `b` or `s` can incur additional model usage. Model and language settings apply to tabs sharing the same Maeklog configuration directory.

### Optional open and hide shortcuts

After the `open` action works, add these entries to Herdr's key configuration. The default file is `~/.config/herdr/config.toml`; `HERDR_CONFIG_PATH` can change it. Check the actual path in the `Config` section of `herdr --help`. If these keys already exist, edit the existing entries rather than adding duplicates.

```toml
[[keys.command]]
key = "prefix+m"
type = "plugin_action"
command = "maeklog.open"
description = "open Maeklog"

[[keys.command]]
key = "prefix+shift+m"
type = "plugin_action"
command = "maeklog.hide"
description = "hide Maeklog"
```

Check and apply the configuration:

```sh
herdr config check
herdr server reload-config
```

The default prefix is `Ctrl+B`: press `Ctrl+B`, then `m` to open; press `Ctrl+B`, then uppercase `M` to hide. Use your own prefix if you changed Herdr's configuration.

## Everyday controls

Click the panel to focus it before using its keys. The menu contains all actions.

| Input | Action |
| --- | --- |
| `h` | Read all saved records without a model call. |
| `e` | Inspect evidence, correct or exclude a record, or undo a correction. |
| `r` | Summarize pending completed turns from today, or retry failed work. |
| `b` | Select an earlier or held date; `Enter` requests its summary. Opening the selection alone does not call a model. |
| `s` | Request a draft summary of the current public progress. |
| `l` / `o` | Correct issue/PR links / open a linked reference. |
| `v` | Refresh issue and PR information without requesting a summary. |
| `q` | Hide the panel while collection continues. |
| Arrow keys, `j`/`k`, mouse wheel | Scroll. |

Completed turns collected today are summarized automatically, newest batch first. Earlier dates, unknown timestamps and turns spanning midnight require selection with `b`. The same work receives at most three automatic attempts before its source is retained for an explicit retry.

Hiding the panel keeps collection running. To stop collection and summaries, use **Stop recording in this tab** in the menu. Reopening the panel in a tab whose recording was stopped, or restarting collection, processes up to the three most recent collected, completed turns from today; the remainder are held for manual selection with `b`. Other tabs retain their own recording settings. See [dates, retries and collection scope (Korean)](usage.md#언제-자동으로-요약하나요).

## What records can tell you

Maeklog helps you regain context by organizing collected Codex conversations. Its saved records do not automatically restore that context into Codex as memory. The integration described here supports Codex, rather than arbitrary agents.

Saved completion records distinguish reports of implementation, verification, merge and deployment. They remain reports from the conversation: Maeklog does not independently verify the work, approve it or execute a next step. Decisions and completion reports keep their change history; next actions retain their owner and conditions. Use `e` to inspect evidence and correct mistakes.

Public commentary, plans and activity appear alongside recorded progress, blockers and next actions. Records are stored locally; team cloud sync is not provided.

## Storage and privacy

The actual record directory is the `state_dir` value in `hook-runtime.json` inside Maeklog's configuration directory. Herdr's default plugin state path is `~/.local/state/herdr/plugins/maeklog`; configuration defaults to `~/.config/herdr/plugins/config/maeklog`. XDG paths and explicit settings can change these locations.

Directories use `0700` permissions and files use `0600`. Storage is **not encrypted**. Pending source text, evidence quotations and diagnostic files can contain sensitive conversations.

For completed turns, Maeklog collects user requests, final responses, interruptions and session connections. Its current-progress cache separately stores public commentary, plans and tool-activity metadata. Private reasoning and tool-output bodies are not collected. Summarization sends the selected conversation and a limited index of existing records to a model service through Codex; it does not send the entire journal.

Summaries consume additional model usage, with no monetary budget cap. Merely displaying public progress, browsing saved records or changing settings does not run a summary. Model interpretation can be wrong; use `e` to inspect evidence and correct important records.

See [storage and deletion (Korean)](usage.md#모델-사용량과-저장) and the [security policy](../SECURITY.md).

## Updates

Run in a regular Herdr shell:

```sh
codex update --check
codex update 0.162.1
```

`codex update` uses Maeklog's validated update flow. If the patch no longer matches upstream or validation fails, it stops the installation. A normal npm update does not update this integration and can change which `codex` launcher is used. Keep the source checkout available and see the [Codex integration and update guide](../integrations/codex/README.md).

## Troubleshooting

| Symptom | Check first |
| --- | --- |
| Python cannot find `tomllib` | Use Python 3.11 or later. |
| A required command or Bazel is missing | Install the tool named by preflight. Bazelisk must provide a `bazel` command on PATH. |
| Maeklog is missing or more than one install is listed | Inspect `herdr plugin list --plugin maeklog --json` for the plugin ID and actual path. Resolve the installation/link state; do not guess the path. |
| The Maeklog binary is missing | Check the release build output. For a Herdr-managed build, inspect `herdr plugin log list --plugin maeklog`. |
| Hooks are missing or their trust does not match | Invoke `install-codex-hooks`, review all four current definitions in `/hooks` in the same Codex environment, then rerun `--preflight`. |
| The panel opens but new conversation data is missing | Check that the new terminal uses the integrated `codex` launcher, review all four handlers in `/hooks`, and rerun `--preflight`. |
| Today's records have not been reflected | Wait for the Codex response to finish, then press `r`. Use `b` for earlier or held turns. |
| Summary processing failed or automatic retries stopped | Open summary and processing details with `h`. Check model access, network and settings. After fixing the cause, use `r` for today or reselect the earlier date with `b`. |
| One conversation is open in two terminals and collection stops | New input is held when its submitting terminal is ambiguous. Continue from one terminal. |
| Dragging does not copy to the clipboard | Check OSC 52 support or use Herdr's `prefix+[` copy mode. |

The [full getting-started guide (Korean)](getting-started.md#문제-해결) covers further installation errors. For current behavior, see the [user guide (Korean)](usage.md).
