# README demo assets

[panel.png](panel.png) is the actual English terminal UI of Maeklog 0.14.0, captured on 2026-10-13 with entirely fictional search-filter work. No real conversations or anonymized historical captures were used. The implementation and 12 passing tests are example reports, not evidence that those changes or tests actually ran.

The fixture contains one decision, one reported completion, one recorded progress item, and one next action owned by Codex. Its current commentary describes checking the narrow-screen layout before writing the PR summary. These records were seeded locally for illustration; the capture does not evaluate model-generated record quality.

- The existing binary ran in a real PTY at 80 × 28 cells. [panel.ansi](panel.ansi) contains its terminal frame; the PNG was mechanically rendered from that frame. No interface text or image pixels were rewritten.
- The capture isolates HOME, CODEX_HOME, all XDG paths, Maeklog state and settings, and the Herdr connection. English is selected in the fixture's own language-settings file. The Git repository is an empty local fixture.
- The worker stays locked and model execution is disabled. Guard executables block model, GitHub, and browser calls; the capture asserts that their call count is zero. Herdr responses come from a local mock.

[overview.svg](overview.svg) and [overview-mobile.svg](overview-mobile.svg) are wide and narrow layouts of **one concept illustration**, with [wide](overview.png) and [narrow](overview-mobile.png) PNG fallbacks. They show a long Codex conversation becoming a clear record that helps you pick up your work. Their fictional decision, work report, progress, and next action match the screenshot scenario. They are authored geometric diagrams, rather than product screenshots, and contain no scripts or external resources.

## Reproduce the terminal capture

Run from the source root. Requirements: a built Maeklog binary, Node.js, Python 3, Git, Pillow, fontconfig, and Noto Sans Mono CJK KR. This is an asset-authoring tool, not a Maeklog installation step.

```sh
node scripts/capture-readme-demo.mjs \
  target/release/maeklog docs/assets/demo /tmp/maeklog-readme-demo
```

The third argument is a private evidence directory. The capture writes `result.json` there with the binary, script, and PNG hashes, terminal dimensions, language, and call count. Temporary fixture paths and runtime hashes belong in that private evidence, not in public documentation. Capture timestamps and temporary fixture identifiers vary when rerun.

## Export the concept illustration

The SVG sources use Noto Sans, with local sans-serif fallbacks. The supplied PNGs were rendered with the installed Noto Sans font and ImageMagick; they contain the same wording and geometry.

```sh
magick -background none docs/assets/demo/overview.svg \
  docs/assets/demo/overview.png
magick -background none docs/assets/demo/overview-mobile.svg \
  docs/assets/demo/overview-mobile.png
```
