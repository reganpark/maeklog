# Branch-memory historical captures

These files originated in synthetic dialogue and `fixture/demo-shop` issue/PR fixtures. They contain no production Space history and no real GitHub/model evaluation. During Maeklog preparation, identifying labels and temporary paths in the text/ANSI sources were replaced with generic values and the two PNGs were re-rendered. They are historical reconstructions rather than unchanged terminal output or fresh Maeklog runtime proof. See [shared provenance](../README.md).

- `panel-default.txt` / `panel-history.txt`: anonymized historical render-once / render-history text at 52 columns.
- `panel-default.ansi` / `panel-history.ansi`: anonymized historical 80×110 PTY frames, before and after `h`.
- `panel-default.png` / `panel-history.png`: terminal rasterizations of the sanitized ANSI sources. Theme/font rendering can differ on the user's machine.

To generate a new isolated fixture after the candidate binary is built:

```sh
node scripts/test-branch-memory.mjs /path/to/isolated/debug/maeklog /path/to/new/capture-directory
python3 scripts/render-terminal-capture.py /path/to/new/capture-directory/panel-default.ansi /path/to/new/capture-directory/panel-history.ansi
```

The fixture also resizes the PTY to 30 columns and back, uses Home/End scrolling, folds again, and checks journal bytes plus model/GitHub call counts remain unchanged. The large historical frame height allows the example history to remain visible.
