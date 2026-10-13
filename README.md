<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/brand/maeklog-02-horizontal-white.svg">
    <img src="docs/assets/brand/maeklog-02-horizontal-black.svg" width="420" alt="Maeklog">
  </picture>
</p>

<p align="center">
  <strong>Pick up your Codex work where you left off.</strong><br>
  Decisions, progress and next actions beside your terminal.
</p>

<p align="center">
  <a href="#what-is-maeklog">What it is</a> ·
  <a href="#see-it-in-action">See it in action</a> ·
  <a href="#day-to-day-use">Everyday use</a> ·
  <a href="docs/setup.md">Setup guide</a>
</p>

## What is Maeklog?

Maeklog is a terminal side panel for **Codex in Herdr**. It turns a long conversation into a readable record of decisions, reported completions, current progress, blockers and next actions—with source quotations and timestamps you can check.

Each Herdr tab keeps its own records, organized by repository and branch. Start a new Codex session in the same tab and those records remain available for you to read.

## The problem it solves

You switch to another task. Hours later, you return to a long Codex conversation: which approach did you choose, what did Codex say was finished, and what still needs your attention?

Maeklog puts those answers beside the work, so you can regain context without rereading the whole conversation.

<picture>
  <source media="(max-width: 640px)" srcset="docs/assets/demo/overview-mobile.svg">
  <img src="docs/assets/demo/overview.svg" alt="Concept example: a long Codex conversation becomes a clear record of a decision, reported work, current progress and the next action, helping you pick up your work.">
</picture>

## Who it is for

- **People juggling Codex tasks in several Herdr tabs**, including tasks in the same repository and branch.
- **People returning to long-running work** who need a clear place to find the decisions and next actions.
- **People who want to check what a summary is based on**, with evidence and corrections close at hand.

## See it in action

![Maeklog in English, showing public progress and saved decisions, reported completions and next actions for a fictional search-filter task.](docs/assets/demo/panel.png)

*Actual Maeklog terminal UI, in English, with fictional demo data. The work and test counts are example reports. [How this demo was captured](docs/assets/demo/README.md).*

In this example, the decision is to **preserve selected filters** when the search changes. Codex has **reported implementing the change and passing 12 tests**. A small-screen layout check is still in progress, and Codex's next action is to write the PR summary **after that check**.

The panel separates **current public progress** from **saved records**. A response ending is not treated as proof that the task is complete. Important decisions and completion reports can be opened to inspect their evidence and correct mistakes.

## Day-to-day use

1. **Open the panel in your project tab.** Work in Codex beside it. Records from other tabs stay separate.
2. **Catch up before you continue.** Read the active decisions and next actions; press `h` to browse all saved records without a model call.
3. **Check the details that matter.** Press `e` to inspect source evidence, correct or exclude a record, or undo a correction.
4. **Hide it when you need the space.** Press `q` to hide the panel. Collection continues; use **Stop recording in this tab** in the menu when you want collection and summaries to stop.

Click the panel to focus its keys. Open the menu with `m`, `F10` or a right-click. Today's completed turns are summarized automatically; `r` processes pending turns from today or retries failures, and `b` lets you select earlier or held dates. [Full controls and summary behavior](docs/setup.md#everyday-controls).

## Setup and configuration

Maeklog is currently available as a **manual source build**. Setup includes building the panel, trusting its four Codex hooks, and building a Maeklog-compatible **Codex 0.162.1** client. You will need Herdr 0.8.2 or later and the build tools listed in the guide. Linux x86_64 is validated; macOS is a declared target that has not yet been verified on real hardware.

**[Follow the English setup guide →](docs/setup.md)**

Once installed, use the panel menu to choose the summary model and reasoning effort, set the language to **Auto, English or Korean**, and review or correct issue and PR links. Settings affect future summaries; changing them does not call a model or rewrite existing records. The default summary profile is `gpt-6-luna`, `medium` effort, Standard service. [Configuration and shortcuts](docs/setup.md#configuration).

## Privacy and model usage

Records are stored locally. Summarization sends **selected conversation text and a limited index of existing records** to a model service through Codex, and consumes additional model usage. **There is no monetary budget cap.** Showing public progress or reading saved records does not itself run a summary.

Storage is not encrypted. Pending conversation text, evidence and diagnostic files can contain sensitive information. Model summaries can also be wrong—check the evidence for important records. [What records can tell you](docs/setup.md#what-records-can-tell-you) · [Storage details](docs/setup.md#storage-and-privacy) · [Security policy](SECURITY.md).

## Documentation

- [English setup, controls and troubleshooting](docs/setup.md)
- [Getting started (Korean)](docs/getting-started.md) · [User guide (Korean)](docs/usage.md)
- [Codex integration and updates](integrations/codex/README.md)
- [Current UI contract](docs/ui-contract.md) · [Release validation status](docs/release-readiness.md)
- [MIT license](LICENSE) · [Historical architecture notes](docs/architecture.md)

Earlier design notes, dated verification records and older captures describe their own historical scope. Use the setup and user guides for current behavior.
