# Brings Liero Factory Reloaded alive, part 1 (issue #97): the crane's
# claw crossing its rail, humming while it moves, and the console going
# red (Alarm) while it does, through `olr-cli` commands only. Run it after
# `factory_assets.py` (issue #96), from the repository's root:
#
#   python3 -I tools/sounds/factory_sounds.py
#   python3 -I tools/levels/factory_live.py target/debug/olr-cli
#
# 1. The Console's painted state pictures (`tools/levels/factory/art/`,
#    its `rock-disp` layer's box, x 941-957 × y 279-329), from After Dark's
#    `front.png`: `console-normal.png` its LED column as today (the
#    yellow dots), `console-alarm.png` every dot of the column red
#    (#ff3020) and the grey slot under the sign (x 941-957, y 326-329)
#    a lit red lamp, its greys turned to reds of the same lightness.
# 2. The Crane and the Console are made again by their blocks in
#    `content/assets/README.md` (each on a fresh copy of After Dark),
#    which read those pictures, `tools/levels/factory/sounds/hum.wav`
#    (`tools/sounds/factory_sounds.py`) and the Crane's default script
#    `tools/levels/factory/crane.rhai`.
# 3. The level takes them (`level library update <pack> k o`), its
#    `crane` and `console` copies are switched live (`level copy set …
#    --live`), its level script is `tools/levels/factory/level.rhai`
#    (`level script --set`) and its notes say so.
#
# Running it again makes the same bytes. Scratch files go to
# `target/factory-assets/`.

import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from factory_assets import AFTER_DARK, ASSETS, OUT, SCRATCH, blocks, folder_of, fresh_copy, run  # noqa: E402
from reloaded_materials import decode  # noqa: E402

import struct, zlib  # noqa: E402

ART = "tools/levels/factory/art"
LIVE_ASSETS = ["crane", "console"]
NOTES = (
    "Liero Factory After Dark rebuilt from assets (issue #96): the ground is the level's own "
    "asset, the night sky a copy of its own at depth 90, and the strip lights, the ceiling lamp, "
    "the control room's screen and desk, the lit sign, a roof beacon, the crane, the console and "
    "the gauge are copies of the built-in assets where After Dark has them. Since issue #97 the "
    "crane's claw crosses its rail every couple of seconds, humming and pushing worms in its "
    "way, and the console under the deck goes red while it moves; issues #98 and #99 bring the "
    "rest alive."
)

# The Console's `rock-disp` box in the level (After Dark's place): its LED
# column at x 942, rows 279-297, and the slot under the sign.
DISP_AT, DISP_SIZE = (941, 279), (17, 51)
LED_X = 942
LED_ROWS = range(279, 298, 3)
SLOT = (941, 326, 17, 4)
LIT = (0xFF, 0xFF, 0x00)
RED = (0xFF, 0x30, 0x20)


def encode_rgba(path, w, h, rows):
    data = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)
    print(f"wrote {path}")


def lamp(grey):
    """A slot pixel's grey as the lit lamp's red: #ff3020 at the slot's
    lightest grey (#d4d4d4), darker greys darker reds."""
    k = grey[0] / 0xD4
    return tuple(min(255, round(c * k)) for c in RED)


def make_art():
    os.makedirs(ART, exist_ok=True)
    _, _, front = decode(os.path.join(AFTER_DARK, "front.png"))
    (x0, y0), (w, h) = DISP_AT, DISP_SIZE
    clear = (0, 0, 0, 0)
    normal = [[clear] * w for _ in range(h)]
    alarm = [[clear] * w for _ in range(h)]
    for y in LED_ROWS:
        p = front[y][LED_X]
        assert p[:3] in (LIT, (0, 0, 0)), (LED_X, y, p)
        if p[:3] == LIT:
            normal[y - y0][LED_X - x0] = LIT + (255,)
        alarm[y - y0][LED_X - x0] = RED + (255,)
    sx, sy, sw, sh = SLOT
    for y in range(sy, sy + sh):
        for x in range(sx, sx + sw):
            p = front[y][x]
            assert p[0] == p[1] == p[2] and p[3] == 255, (x, y, p)
            alarm[y - y0][x - x0] = lamp(p) + (255,)
    encode_rgba(os.path.join(ART, "console-normal.png"), w, h, normal)
    encode_rgba(os.path.join(ART, "console-alarm.png"), w, h, alarm)


def make_assets(cli):
    for args_block in blocks():
        folder = os.path.basename(folder_of(args_block) or "")
        if folder not in LIVE_ASSETS:
            continue
        source = fresh_copy(AFTER_DARK, f"source-{folder}")
        for args in args_block:
            run(cli, *[source if a == AFTER_DARK else a for a in args])


def make_level(cli):
    run(cli, "level", "library", "update", OUT, "k", "o")
    run(cli, "level", "copy", "set", OUT, "crane", "--live")
    run(cli, "level", "copy", "set", OUT, "console", "--live")
    run(cli, "level", "script", OUT, "--set", "tools/levels/factory/level.rhai")
    run(cli, "level", "notes", OUT, NOTES)
    run(cli, "level", "bake", OUT)
    run(cli, "level", "check-scripts", OUT)
    run(cli, "pack", "check", OUT)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: python3 -I tools/levels/factory_live.py <olr-cli>")
    cli = os.path.abspath(sys.argv[1])
    os.makedirs(SCRATCH, exist_ok=True)
    make_art()
    make_assets(cli)
    make_level(cli)


main()
