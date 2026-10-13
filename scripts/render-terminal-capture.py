#!/usr/bin/env python3
"""Rasterize a captured PM terminal frame, not a design mockup. No screen content rewritten."""
import argparse
import pathlib
import re
import subprocess
import unicodedata
from PIL import Image, ImageDraw, ImageFont
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--columns", type=int, help="Captured terminal width, including blank cells")
parser.add_argument("paths", nargs="+")
args = parser.parse_args()
font_path = subprocess.check_output(
    ["fc-match", "Noto Sans Mono CJK KR", "-f", "%{file}"], text=True
).strip()
font = ImageFont.truetype(font_path, 20)
bold_path = subprocess.check_output(
    ["fc-match", "Noto Sans Mono CJK KR:style=Bold", "-f", "%{file}"], text=True
).strip()
bold_font = ImageFont.truetype(bold_path, 20)
background, foreground = "#282828", "#ebdbb2"
cell, line_height, margin = 12, 30, 24
for argument in args.paths:
    path = pathlib.Path(argument)
    data = path.read_text()
    rows, row, col = {}, 0, 0
    fg, bg, bold, reverse = foreground, background, False, False
    tokens = re.split(r"(\x1b\[[0-9;?]*[A-Za-z])", data)
    for token in tokens:
        if token.startswith("\x1b["):
            if token.endswith("H"):
                parts = token[2:-1].split(";")
                row = int(parts[0] or "1") - 1
                col = int(parts[1] or "1") - 1 if len(parts) > 1 else 0
            elif token == "\x1b[2J":
                rows = {}
            elif token.endswith("m"):
                codes = [int(v or 0) for v in token[2:-1].split(";")]
                i = 0
                while i < len(codes):
                    code = codes[i]
                    if code == 0:
                        fg, bg, bold, reverse = foreground, background, False, False
                    elif code == 1:
                        bold = True
                    elif code == 22:
                        bold = False
                    elif code == 7:
                        reverse = True
                    elif code == 27:
                        reverse = False
                    elif code == 39:
                        fg = foreground
                    elif code == 49:
                        bg = background
                    elif code in (38, 48) and codes[i + 1:i + 2] == [2] and len(codes) >= i + 5:
                        color = "#{:02x}{:02x}{:02x}".format(*codes[i + 2:i + 5])
                        if code == 38:
                            fg = color
                        else:
                            bg = color
                        i += 4
                    i += 1
            continue
        for character in token:
            if character == "\r":
                col = 0
            elif character == "\n":
                row += 1
            elif ord(character) >= 32:
                ink, paper = (bg, fg) if reverse else (fg, bg)
                span = 2 if unicodedata.east_asian_width(character) in ("W", "F") else 1
                cells = rows.setdefault(row, {})
                # Terminal writes replace both cells of an overlapping wide glyph.
                # Keeping the old trailing-cell entry creates false menu text ghosts.
                for old_col, old in list(cells.items()):
                    old_span = 2 if unicodedata.east_asian_width(old[0]) in ("W", "F") else 1
                    if old_col < col + span and col < old_col + old_span:
                        del cells[old_col]
                        for cleared in range(old_col, old_col + old_span):
                            if not col <= cleared < col + span:
                                cells[cleared] = (" ", old[1], old[2], False)
                cells[col] = (character, ink, paper, bold)
                col += span
    # Keep exact terminal coordinates, including blank rows and the fixed footer.
    height = (max(rows, default=0) + 1) * line_height + margin * 2
    columns = max((x + (2 if unicodedata.east_asian_width(v[0]) in ("W", "F") else 1)
                   for cells in rows.values() for x, v in cells.items()), default=80)
    if args.columns is not None:
        if args.columns < columns:
            raise ValueError(f"{path}: captured content exceeds --columns {args.columns}")
        columns = args.columns
    canvas = Image.new("RGB", (columns * cell + margin * 2, height), background)
    draw = ImageDraw.Draw(canvas)
    for y, cells in rows.items():
        for x, (character, ink, paper, heavy) in cells.items():
            left, top = margin + x * cell, margin + y * line_height
            span = 2 if unicodedata.east_asian_width(character) in ("W", "F") else 1
            draw.rectangle((left, top, left + cell * span - 1, top + line_height - 1), fill=paper)
            draw.text((left, top), character, font=bold_font if heavy else font, fill=ink)
    canvas.save(path.with_suffix(".png"))
    print(path.with_suffix(".png"))
