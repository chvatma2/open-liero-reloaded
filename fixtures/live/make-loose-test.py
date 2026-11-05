#!/usr/bin/env python3
"""Builds fixtures/live/levels/loose-test (the loose dirt design): a
200 x 120 rock box, two shelves over a lower cave with a gap between
them, a thin dirt bridge over the gap, and loose dirt of two materials:

  sand    (repose 30): a block on the left shelf (it slides into a
          heap and spills over the shelf's edge), a heap on the bridge
          (it falls when the bridge is dug) and a layer over the floor;
  gravel  (the default rates): a block on the right shelf.

Standard library only; run from the repository root:
    python3 fixtures/live/make-loose-test.py
"""

import os
import struct
import zlib

W, H = 200, 120
OUT = os.path.join("fixtures", "live", "levels", "loose-test")

ROCK, BACKGROUND, DIRT = 0, 1, 2
# `olr_format::levels::MATERIAL_PALETTE`, indexed by `Material as u8`.
PALETTE = bytes([128, 128, 128, 0, 0, 0, 139, 90, 43])


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def png(colour_type, rows, palette=None):
    """An 8-bit PNG: rows of bytes, each row already in its pixel format."""
    raw = b"".join(b"\x00" + bytes(r) for r in rows)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, colour_type, 0, 0, 0))
    if palette is not None:
        out += chunk(b"PLTE", palette)
    return out + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def inside(x, y, x0, y0, x1, y1):
    return x0 <= x < x1 and y0 <= y < y1


def cell(x, y):
    """The material and loose kind (0 none, 1 sand, 2 gravel) of a pixel."""
    if x < 3 or x >= W - 3 or y < 3 or y >= H - 3:
        return ROCK, 0
    if inside(x, y, 3, 60, 85, 64) or inside(x, y, 115, 60, W - 3, 64):
        return ROCK, 0
    if inside(x, y, 85, 60, 115, 64):
        return DIRT, 0  # the bridge over the gap
    if inside(x, y, 20, 30, 80, 60) or inside(x, y, 88, 45, 112, 60):
        return DIRT, 1
    if inside(x, y, 125, 35, 175, 60):
        return DIRT, 2
    if inside(x, y, 3, 96, W - 3, H - 3):
        return DIRT, (1 if inside(x, y, 60, 96, 140, 104) else 0)
    return BACKGROUND, 0


def back(x, y):
    return (20, 24, 48 + (x + y) % 8, 255)


def front(x, y, m, k):
    if m == ROCK:
        v = 100 + (x * 7 + y * 13) % 20
        return (v, v, v + 4, 255)
    if m == BACKGROUND:
        return back(x, y)
    n = (x * 3 + y * 5) % 16
    if k == 1:
        return (200 + n, 170 + n, 90, 255)
    if k == 2:
        return (140 + n, 124 + n, 104, 255)
    return (120 + n, 80 + n, 40, 255)


def main():
    cells = [[cell(x, y) for x in range(W)] for y in range(H)]
    os.makedirs(OUT, exist_ok=True)
    files = {
        "material.png": png(3, ([m for m, _ in row] for row in cells), PALETTE),
        "loose.png": png(0, ([k for _, k in row] for row in cells)),
        "front.png": png(
            6, ([c for x, (m, k) in enumerate(row) for c in front(x, y, m, k)] for y, row in enumerate(cells))
        ),
        "back.png": png(6, ([c for x in range(W) for c in back(x, y)] for y in range(H))),
    }
    files["level.ron"] = b"""(
    name: "Loose Test",
    size: (200, 120),
    theme: "olx-theme-dirt",
    loose: [
        (
            id: "sand",
            name: "Sand",
            repose: 30,
        ),
        (
            id: "gravel",
            name: "Gravel",
        ),
    ],
)
"""
    files["pack.ron"] = b"""(
    format_version: 5,
    id: "olr-level-loose-test",
    name: "Loose Test",
    version: "0.1.0",
    kind: Level,
    authors: [],
    origin: None,
    license: None,
)
"""
    for name, data in files.items():
        with open(os.path.join(OUT, name), "wb") as f:
            f.write(data)


if __name__ == "__main__":
    main()
