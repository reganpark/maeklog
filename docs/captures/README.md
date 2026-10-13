# Anonymized historical terminal captures

All 17 bundled PNGs and their ANSI/text sources originate from synthetic terminal fixtures of the original implementation. They include fabricated dialogue, issue/PR metadata, sessions and tab identities; they are not production history and do not record a real GitHub or model evaluation.

For Maeklog preparation, the text and ANSI sources were sanitized to use generic project labels and local paths. The PNGs were reconstructed from those sources with `scripts/render-terminal-capture.py`. Terminal text is rendered from source; the previous image pixels were not edited. Original dates, historical UI behavior and synthetic evidence IDs may remain. These images illustrate the historical interface and do not prove that Maeklog or a renamed Codex package is installed or freshly runtime-tested.

The reconstruction retains explicit ANSI styles, terminal coordinates and the captured cell width. Early frames that used terminal default colors now use the renderer's Gruvbox palette; this is a rendering choice, not a claim of original pixel equivalence. Narrow frames that need more room for a generic label are reflowed within that width; see the preparation verification record for any reconstruction adjustments. Terminal font/theme rendering may differ on other machines.

Capture groups: branch memory (2 PNGs), corrections (3), daily policy (2), Gruvbox (5), live progress (3), selection (2). The additional ANSI/TXT files are preserved as historical source variants. No duplicate image was removed.

To rasterize a sanitized terminal source:

```sh
python3 scripts/render-terminal-capture.py --columns CAPTURED_CELL_WIDTH /path/to/sanitized/frame.ansi
```

Current candidate verification is separate from these historical examples: [Maeklog preparation checks](../verification-maeklog-stage2-2026-10-13.md).
