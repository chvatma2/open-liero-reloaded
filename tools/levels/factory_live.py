# Brings Liero Factory Reloaded alive, part 1 (issue #97): the crane's
# claw crossing its rail, humming while it moves, and the terminal left
# of it (issue #100; the console under the deck before it) going red
# (Alarm) while it does; part 2 (issue #98): three of the ceiling
# strips failing like faulty tubes, buzzing and ticking; through
# `olr-cli` commands only. Run it after
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
#    Issue #98: the Strip light's (its `lamps` layer, 29 x 2, at After
#    Dark's first strip, 778, 80): `strip-dim.png` every pixel at 60 %
#    brightness and `strip-off.png` the tube unlit, its white the pale
#    grey OFF and every other pixel the same fraction of it (a highlight
#    stays a highlight).
#    Issue #100: the Terminal's `terminal-alarm.png` (its `rock-bcn`
#    box, x 723-731 x y 352-359, every pixel #ff3020).
# 2. The Crane, the Terminal (issue #100) and the Strip light are made
#    again by their blocks in `content/assets/README.md` (each on a fresh copy of After Dark),
#    which read those pictures, `tools/levels/factory/sounds/` (`hum.wav`,
#    `buzz.wav`, `tick.wav`: `tools/sounds/factory_sounds.py`) and the
#    default scripts `tools/levels/factory/crane.rhai` and `tube.rhai`.
#    Issue #99: the Computer screen's camera pictures (`screen-*.png`,
#    `make_screen_art`: each part of After Dark drawn full bright with
#    `olr-cli look-preview`, box-downsampled to the screen's 12 x 11 and
#    mapped to the colours of its truck picture; the garden touched up by
#    hand in code, `GARDEN_TOUCH`), the Lit sign's (`sign-*.png` and their
#    glow masks), the Control desk's (`desk-*.png`) and the Gauge's
#    (`gauge-*.png`) state pictures; the Crane's block now paints its
#    warning beacon itself.
# 3. The level takes them (`level library update <pack> k e s c t d g`),
#    its `crane` and (issue #100) `terminal` copies, the strips `LIVE_STRIPS` and (issue
#    #99) `LIVE_DISPLAYS` are switched live (`level copy set … --live`;
#    the plant works: every strip and `LIVE_WORKS` too, the strips not in
#    `LIVE_STRIPS` starting Steady with `steady.rhai` as their own
#    script instead of their asset's), its level script is
#    `tools/levels/factory/level.rhai` (`level script --set`) and its
#    notes say so.
#
# Running it again makes the same bytes. Scratch files go to
# `target/factory-assets/`.

import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from factory_assets import AFTER_DARK, ASSETS, OUT, SCRATCH, blocks, folder_of, fresh_copy, run  # noqa: E402
from reloaded_materials import decode  # noqa: E402

import struct, zlib  # noqa: E402

ART = "tools/levels/factory/art"
LIVE_ASSETS = ["crane", "terminal", "strip-light", "computer-screen", "lit-sign", "control-desk", "gauge"]
# Issue #98: the faulty tubes: the hall's top floor's middle strip, the
# hall's strip left of the gauge and the lowest office floor's middle one.
LIVE_STRIPS = ["strip-2", "strip-4", "strip-7"]
# The night shift: the strips' colours by area (the Strip light's exposed
# `colour`): sodium in the warehouse and on the dock, cool white along the
# upper hall's top; the lounge's (`strip-4`, `strip-5`) stay warm.
STRIP_COLOURS = [("#ffb050", ["strip-6", "strip-7", "strip-8", "strip-9"]),
                 ("#dce8ff", ["strip-1", "strip-2", "strip-3"])]
# Issue #99: the copies whose displays change.
LIVE_DISPLAYS = ["screen", "sign", "desk", "gauge"]
# The plant works (2026-11-04): every strip is live (health, broken, a
# power cut); the faulty tubes (`LIVE_STRIPS`) run the Strip light's
# script, the others their own `steady.rhai`; and the pass's new copies.
STRIPS = [f"strip-{i}" for i in range(1, 10)]
STEADY = "tools/levels/factory/steady.rhai"
LIVE_WORKS = ["line", "siren", "generator", "lift", "cart", "forklift", "lab-flask", "pane-1", "pane-3", "pane-4"]
# Issue #99: the security camera's parts of After Dark, each drawn full
# bright centred on a world point, and the world box shrunk to the
# screen's 12 x 11 (about its aspect).
SCREEN = "content/assets/computer-screen/layers/ctrl-scr.png"
SCREEN_SIZE = (12, 11)
SCREEN_PARTS = [
    ("crane", (800, 300), (742, 252, 96, 88)),  # the claw under the conveyor deck
    ("offices", (500, 230), (470, 178, 110, 101)),  # the lab and the teal office below it
    ("garden", (500, 160), (440, 95, 130, 119)),  # the roof garden under the moon
]
# The garden's touch-up (the plants and the moon are lost at 12 x 11): the
# sky over rows 0-7, the moon a white pixel, four stalks under gold heads.
GARDEN_SKY_ROWS = 8
GARDEN_MOON = (10, 1)
GARDEN_STALKS = [1, 3, 5, 7]
# The Lit sign's `rock-disp` box (28 x 16): its unlit face colour, the
# dash between the digits, the "-LD-" line and the "7".
SIGN_FACE = (114, 114, 114)
SIGN_DASH = (13, 3, 3, 2)
SIGN_LD = (6, 10, 17, 6)
SIGN_SEVEN = (23, 0, 5, 8)
# The Control desk's `displays` (21 x 1): its four dots' columns, a dark
# dot's colour and state C's red.
DESK_DOTS = [(0, 2), (7, 1), (15, 1), (20, 1)]
DESK_DARK = (0, 20, 40)
DESK_RED = (255, 40, 32)
# The Gauge's `rock-disp` (33 x 16): its needle (cyan, row 4), its dial
# colour, and its two buttons (rows 14-15).
NEEDLE = (0, 255, 255)
DIAL = (255, 255, 0)
NEEDLE_ROW = 4
BUTTONS = [(0, 1), (31, 2)]
NOTES = (
    "Liero Factory After Dark rebuilt from assets (issue #96): the ground is the level's own "
    "asset, the night sky a copy of its own at depth 90, and the strip lights, the ceiling lamp, "
    "the control room's screen and desk, the lit sign, a roof beacon, the terminal, the crane, "
    "the console and the gauge are copies of the built-in assets where After Dark has them. Since issue #97 the "
    "crane's claw crosses its rail every couple of seconds, humming and pushing worms in its "
    "way, and the terminal left of it goes red while it moves (issue #100); since issue #98 three of the "
    "ceiling strips fail like faulty tubes, stuttering, buzzing and ticking; since issue #99 the "
    "control room's screen is a security camera, the 13-37 sign, the desk's lamps and the gauge "
    "change now and then, and an orange warning beacon under the crane's trolley turns while it "
    "moves. The night shift lights the mine's back wall, the lower hall, the red duct and the "
    "chimney, colours the strips by area and sets a city and its industry before the stars. "
    "The plant works: a horn starts the shift and the line under the sign stamps dirt into balls "
    "that roll up the incline into the crate, its belt red when stopped and white when running; "
    "a lift, a mine cart and a forklift go about their work; a big blast or a quick run of kills "
    "sounds the roof's sirens and tints the lamps red; shooting the generator by the gauge cuts "
    "the power (red emergency lamps, the machines stopped) until it is repaired; strips can be "
    "shot out and are mended now and then, the lab's flask bubbles and the office panes shatter."
)

# The Console's `rock-disp` box in the level (After Dark's place): its LED
# column at x 942, rows 279-297, and the slot under the sign.
DISP_AT, DISP_SIZE = (941, 279), (17, 51)
LED_X = 942
LED_ROWS = range(279, 298, 3)
SLOT = (941, 326, 17, 4)
LIT = (0xFF, 0xFF, 0x00)
RED = (0xFF, 0x30, 0x20)
# Issue #100: the Terminal's box in the level (After Dark's place, left
# of the crane), its `rock-bcn` layer's screen edge and lamp (9 glowing
# pixels) all red in Alarm.
TERMINAL = (723, 351, 15, 14)
# Issue #98: the Strip light's `lamps` box in After Dark, its Dim
# brightness (percent) and its unlit tube's grey.
STRIP_AT, STRIP_SIZE = (778, 80), (29, 2)
DIM = 60
OFF = (150, 150, 156)


def encode_rgba(path, w, h, rows):
    data = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)
    print(f"wrote {path}")


def encode_grey(path, w, h, rows):
    data = b"".join(b"\x00" + bytes(row) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)
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
    make_strip_art(front)
    make_terminal_art()


def make_terminal_art():
    """Issue #100: the Terminal's Alarm picture of `rock-bcn` (its box
    within TERMINAL), every pixel red."""
    _, _, bcn = decode(os.path.join(AFTER_DARK, "editor/layers/rock-bcn.png"))
    tx, ty, tw, th = TERMINAL
    pts = [(x, y) for y in range(ty, ty + th) for x in range(tx, tx + tw) if bcn[y][x][3]]
    x0, y0 = min(p[0] for p in pts), min(p[1] for p in pts)
    w, h = max(p[0] for p in pts) - x0 + 1, max(p[1] for p in pts) - y0 + 1
    alarm = [[(0, 0, 0, 0)] * w for _ in range(h)]
    for x, y in pts:
        alarm[y - y0][x - x0] = RED + (255,)
    encode_rgba(os.path.join(ART, "terminal-alarm.png"), w, h, alarm)


def make_screen_art(cli):
    """Issue #99: the Computer screen's camera pictures."""
    # A layer's PNG stacks its states' pictures under its own: the first box.
    truck = decode(SCREEN)[2][:SCREEN_SIZE[1]]
    palette = sorted(set(p[:3] for row in truck for p in row))
    near = lambda c: min(palette, key=lambda p: sum((a - b) ** 2 for a, b in zip(p, c)))
    sat = lambda c: max(c) - min(c)
    sw, sh = SCREEN_SIZE
    for name, (cx, cy), (x0, y0, w, h) in SCREEN_PARTS:
        full = os.path.join(SCRATCH, f"screen-{name}-full.png")
        run(cli, "look-preview", AFTER_DARK, "--view", "full", "--at", f"{cx},{cy}", "--size", "427x192",
            "--out", full)
        _, _, px = decode(full)
        vx, vy = cx - 427 // 2, cy - 192 // 2
        rows = []
        for j in range(sh):
            row = []
            for i in range(sw):
                box = [px[y - vy][x - vx][:3]
                       for y in range(y0 + h * j // sh, y0 + h * (j + 1) // sh)
                       for x in range(x0 + w * i // sw, x0 + w * (i + 1) // sw)]
                c = [sum(p[k] for p in box) / len(box) for k in range(3)]
                # The box's most saturated colour wins where it covers a quarter of it.
                top = max(box, key=sat)
                if sat(top) > 80 and 4 * sum(1 for p in box if sum(abs(a - b) for a, b in zip(p, top)) < 60) >= len(box):
                    c = list(top)
                m = sum(c) / 3
                row.append(near(tuple(min(255, max(0, round(m + (v - m) * 1.6))) for v in c)) + (255,))
            rows.append(row)
        if name == "garden":
            sky, white, green, gold = truck[0][0][:3], (252, 252, 252), (0, 255, 0), (183, 129, 2)
            for y in range(GARDEN_SKY_ROWS):
                rows[y] = [sky + (255,)] * sw
            rows[GARDEN_MOON[1]][GARDEN_MOON[0]] = white + (255,)
            for x in GARDEN_STALKS:
                rows[GARDEN_SKY_ROWS - 4][x] = gold + (255,)
                for y in range(GARDEN_SKY_ROWS - 3, GARDEN_SKY_ROWS):
                    rows[y][x] = green + (255,)
        encode_rgba(os.path.join(ART, f"screen-{name}.png"), sw, sh, rows)


def first_box(path, h):
    """A layer's own picture: its PNG's first `h` rows (states stack theirs under it)."""
    w, _, px = decode(path)
    return w, h, px[:h]


def make_display_art():
    """Issue #99: the Lit sign's, the Control desk's and the Gauge's state
    pictures (and the sign's glow masks: its dark pixels 0)."""
    w, h, sign = first_box(os.path.join(ASSETS, "lit-sign/layers/rock-disp.png"), 16)
    def dark_sign(name, dark):
        art = [[p if p[3] and not dark(x, y) else (SIGN_FACE + (255,) if p[3] else p)
                for x, p in enumerate(row)] for y, row in enumerate(sign)]
        mask = [[0 if dark(x, y) or not p[3] else 255 for x, p in enumerate(row)] for y, row in enumerate(sign)]
        encode_rgba(os.path.join(ART, f"sign-{name}.png"), w, h, art)
        encode_grey(os.path.join(ART, f"sign-{name}-glow.png"), w, h, mask)
    inside = lambda box, x, y: box[0] <= x < box[0] + box[2] and box[1] <= y < box[1] + box[3]
    dark_sign("no-dash", lambda x, y: inside(SIGN_DASH, x, y))
    dark_sign("no-ld", lambda x, y: inside(SIGN_LD, x, y))
    # Every other lit pixel of the 7, in reading order: a failing LED.
    seven = [(x, y) for y in range(h) for x in range(w) if sign[y][x][3] and inside(SIGN_SEVEN, x, y)]
    dying = set(seven[1::2])
    dark_sign("dying-7", lambda x, y: (x, y) in dying)
    w, h, desk = first_box(os.path.join(ASSETS, "control-desk/layers/displays.png"), 1)
    def desk_state(name, lit, red=()):
        row = list(desk[0])
        mask = [0] * w
        for k, (x, n) in enumerate(DESK_DOTS):
            for i in range(x, x + n):
                row[i] = (DESK_RED if k in red else desk[0][i][:3]) + (255,) if k in lit else DESK_DARK + (255,)
                mask[i] = 255 if k in lit else 0
        encode_rgba(os.path.join(ART, f"desk-{name}.png"), w, h, [row])
        encode_grey(os.path.join(ART, f"desk-{name}-glow.png"), w, h, [mask])
    desk_state("b", (0, 2))
    desk_state("c", (1, 2, 3), red=(2,))
    w, h, gauge = first_box(os.path.join(ASSETS, "gauge/layers/rock-disp.png"), 16)
    needle = [x for x in range(w) if gauge[NEEDLE_ROW][x][:3] == NEEDLE]
    def gauge_state(name, shift, swap):
        art = [list(row) for row in gauge]
        for x in needle:
            art[NEEDLE_ROW][x] = DIAL + (255,)
        for x in needle:
            if x + shift < w and gauge[NEEDLE_ROW][x + shift][3]:
                art[NEEDLE_ROW][x + shift] = NEEDLE + (255,)
        if swap:
            (ax, an), (bx, bn) = BUTTONS
            a, b = gauge[14][ax][:3], gauge[14][bx][:3]
            for y in (14, 15):
                for x in range(ax, ax + an):
                    art[y][x] = b + (255,)
                for x in range(bx, bx + bn):
                    art[y][x] = a + (255,)
        encode_rgba(os.path.join(ART, f"gauge-{name}.png"), w, h, art)
    gauge_state("b", 2, False)
    gauge_state("c", 4, True)


def make_strip_art(front):
    """Issue #98: the Strip light's Dim and Off pictures of `lamps`."""
    (x0, y0), (w, h) = STRIP_AT, STRIP_SIZE
    dim = [[(0, 0, 0, 0)] * w for _ in range(h)]
    off = [[(0, 0, 0, 0)] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            p = front[y0 + y][x0 + x]
            assert p[3] == 255, (x0 + x, y0 + y, p)
            dim[y][x] = tuple(round(c * DIM / 100) for c in p[:3]) + (255,)
            k = max(p[:3]) / 255
            off[y][x] = tuple(round(c * k) for c in OFF) + (255,)
    encode_rgba(os.path.join(ART, "strip-dim.png"), w, h, dim)
    encode_rgba(os.path.join(ART, "strip-off.png"), w, h, off)


def make_assets(cli):
    for args_block in blocks():
        folder = os.path.basename(folder_of(args_block) or "")
        if folder not in LIVE_ASSETS:
            continue
        source = fresh_copy(AFTER_DARK, f"source-{folder}")
        for args in args_block:
            run(cli, *[source if a == AFTER_DARK else a for a in args])


def make_level(cli):
    run(cli, "level", "library", "update", OUT, "k", "e", "s", "c", "t", "d", "g")
    run(cli, "level", "copy", "set", OUT, "crane", "--live")
    run(cli, "level", "copy", "set", OUT, "terminal", "--live")
    for copy in STRIPS + LIVE_DISPLAYS + LIVE_WORKS:
        run(cli, "level", "copy", "set", OUT, copy, "--live")
    for copy in STRIPS:
        if copy not in LIVE_STRIPS:
            run(cli, "level", "copy", "set", OUT, copy, "--default-script", "off", "--state", "Steady")
            run(cli, "level", "copy", "script", OUT, copy, "--set", STEADY)
    for colour, strips in STRIP_COLOURS:
        for copy in strips:
            run(cli, "level", "copy", "set", OUT, copy, "--value", f"colour={colour}")
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
    make_screen_art(cli)
    make_display_art()
    make_assets(cli)
    make_level(cli)


main()
