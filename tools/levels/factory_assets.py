# Builds Liero Factory Reloaded from assets (issue #96): Liero Factory
# After Dark's art as a ground, a night sky and copies of built-in assets
# at the pieces' old places, through `olr-cli` commands only:
#
#   python3 -I tools/levels/factory_assets.py target/debug/olr-cli
#
# 1. The new built-in assets (`NEW_ASSETS`: the Crane, the Console, the
#    Gauge and, issue #100, the Terminal) are made by their blocks in `content/assets/README.md`, each run
#    on a fresh copy of After Dark (a block may prepare its source level),
#    written to `content/assets/<folder>`.
# 2. The level is a copy of After Dark keeping its origin (`pack new
#    --keep-origin`), with its own name and notes.
# 3. The ground loses what the copies bring: the distant sky element, and
#    each piece's pixels in its layers (`level element erase --rect`, the
#    locked layers unlocked first and locked again); a layer left empty is
#    removed.
# 4. The night sky is cut from a copy of After Dark as an asset of the
#    level's own (`asset save --elements sky`) and placed at (0, 0),
#    distant at 90 as After Dark's sky is.
# 5. Each built-in asset is placed 1:1 where its pixels were (`level copy
#    add … --key <k> --id <id> --at X,Y`; a second copy of an asset reuses
#    it, so only the first names the key), its link recorded as built-in.
#
# Issue #97: the wall behind the crane. After Dark never painted its
# backdrop where the crane's rock stood, so a moving claw would uncover a
# hole: those pixels (After Dark's rock in the crane's box, air once the
# ground lost the crane) are painted on the ground's `backdrop` while the
# ground is alone (`level element draw --mask … --tile …`), each blended
# along its row between the nearest wall pixels left and right of it.
#
# The night shift (2026-11-03, the maintainer's look study): light where
# After Dark had none, each in a group of its own, on new ground layers
# while the ground is alone (`night_shift`):
# - `MINE_LAMPS`: 9 small lamps on the mine tunnels' back wall (a dark
#   plate round a 2 x 2 warm bulb, only the bulb glowing: `level element
#   glow --mask`), each at least 3 px from rock and dirt; group `mine`.
# - The lab floor's three one-row strips moved out of `lamps` into a layer
#   of their own (`level element extract`, `LAB_STRIPS`) in group `cool`.
# - `HIGH_BAYS`: two lamp faces on the deck's lowest rock rows over the
#   lower hall (a Rock layer, so the material stays), group `sodium`.
# - The red duct's elbow (`FLUE_BOX`'s saturated red rock pixels) moved
#   into a layer of its own glowing at 59 % in group `flue`.
# - `AVIATION`: a red light on the chimney's top, group `aviation`.
# Then the skyline (`factory_skyline.py`): the far city and the near
# industry as the level's own assets, each a level-sized image with its
# glow mask, placed after the sky at depths 75 and 45 (distant layers draw
# in element order). `factory_live.py` colours the strips by area.
#
# Every copy is static (`factory_live.py`, issue #97, then makes the crane
# and, since issue #100, the terminal live). The bake is After Dark's but where the crane's
# pixels were, under the crane (`olr-cli`'s `tests/reloaded.rs`). Scratch
# files go to `target/factory-assets/`. Running it again makes the same
# bytes.

import os, shlex, shutil, struct, subprocess, sys, zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reloaded_materials import decode  # noqa: E402
from factory_skyline import png, write as write_skyline  # noqa: E402

AFTER_DARK = "content/reloaded/olr-level-liero-factory-after-dark"
OUT = "content/reloaded/olr-level-liero-factory-reloaded"
ID = "olr-level-liero-factory-reloaded"
NAME = "Liero Factory Reloaded"
NOTES = (
    "Liero Factory After Dark rebuilt from assets (issue #96): the ground is the level's own "
    "asset, the night sky a copy of its own at depth 90, and the strip lights, the ceiling lamp, "
    "the control room's screen and desk, the lit sign, a roof beacon, the terminal, the crane, "
    "the console and the gauge are copies of the built-in assets where After Dark has them. It plays "
    "as After Dark; issues #97, #98 and #99 bring it alive."
)
ASSETS = "content/assets"
README = os.path.join(ASSETS, "README.md")
SCRATCH = "target/factory-assets"
NEW_ASSETS = ["crane", "console", "gauge", "terminal"]
# Issue #97: the crane's box, whose wall is painted behind its pixels.
CRANE_BOX = (762, 269, 114, 50)
WALL_EDGE = 2
# The night shift's groups: id, colour, intensity, reach, animation.
GROUPS = [
    ("mine", "ffb060", 1000, 110, "Flicker(speed: 3, depth: 10)"),
    ("cool", "dce8ff", 500, 90, None),
    ("sodium", "ffb050", 520, 95, None),
    ("flue", "ff5a20", 320, 70, "Pulse(period_ms: 4200, low: 35, high: 100)"),
    ("aviation", "ff2a14", 400, 22, "Pulse(period_ms: 1500, low: 0, high: 100, shape: Square)"),
]
# The mine lamps' bulbs' top-left pixels, and their colours.
MINE_LAMPS = [(70, 457), (205, 443), (340, 434), (475, 436), (610, 433),
              (745, 420), (880, 421), (1015, 403), (1150, 441)]
PLATE, BULB = "28282e", "ffd890"
LAB_STRIPS = (318, 183, 248, 1)
# The high-bays' left edges and width; the plate on the deck's rock row
# 267, the glowing face on its lowest, 268.
HIGH_BAYS, BAY_WIDTH, BAY_ROW = [895, 1010], 24, 267
BAY_PLATE, BAY_FACE = "5a5c62", "ffd696"
FLUE_BOX, FLUE_GLOW = (1080, 250, 120, 42), 59
AVIATION, AVIATION_ART = (1082, 36, 2, 2), "ff3c28"
# The skyline's layers: the asset's name, its picture (and its element's
# name, kept short for the shipped paths), key, id, depth, group (id,
# colour, intensity, reach, animation).
SKYLINE = [
    ("Night city", "city", "n", "city", 75, ("city", "own", 120, 8, "Flicker(speed: 1, depth: 25)")),
    ("Night industry", "industry", "p", "industry", 45,
     ("pylon", "ff3020", 200, 10, "Pulse(period_ms: 2000, low: 0, high: 100, shape: Square)")),
]

# The pieces: key, built-in folder, the ground's layers its pixels come
# from, its size and its copies (id, top-left), in placing order (later on
# top: the sign above the console).
PIECES = [
    ("s", "strip-light", ["lamps"], (29, 2), [
        ("strip-1", (778, 80)), ("strip-2", (1004, 80)), ("strip-3", (1119, 80)),
        ("strip-4", (984, 177)), ("strip-5", (1067, 177)), ("strip-6", (314, 286)),
        ("strip-7", (427, 286)), ("strip-8", (524, 286)), ("strip-9", (187, 342)),
    ]),
    ("l", "ceiling-lamp", ["rock", "lamps"], (13, 6), [("ceil-lamp", (319, 137))]),
    ("c", "computer-screen", ["backdrop", "ctrl-scr"], (14, 14), [("screen", (906, 83))]),
    ("d", "control-desk", ["backdrop", "screens", "displays"], (30, 11), [("desk", (903, 119))]),
    ("w", "warning-beacon", ["backdrop", "beacons"], (25, 28), [("beacon", (951, 45))]),
    ("e", "terminal", ["rock", "rock-bcn"], (15, 14), [("terminal", (723, 351))]),
    ("k", "crane", ["rock"], (114, 50), [("crane", (762, 269))]),
    ("o", "console", ["rock", "rock-disp"], (33, 64), [("console", (933, 269))]),
    ("t", "lit-sign", ["rock", "rock-disp"], (31, 21), [("sign", (934, 299))]),
    ("g", "gauge", ["rock", "rock-scr", "rock-disp"], (48, 27), [("gauge", (1059, 144))]),
]


def run(cli, *args):
    print("$ olr-cli " + " ".join(shlex.quote(a) for a in args), flush=True)
    r = subprocess.run([cli, *args], capture_output=True, text=True)
    sys.stdout.write(r.stdout)
    if r.returncode != 0:
        sys.exit(f"olr-cli {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def fresh_copy(src, name):
    """A fresh copy of the pack at `src` under the scratch directory."""
    dst = os.path.join(SCRATCH, name)
    shutil.rmtree(dst, ignore_errors=True)
    shutil.copytree(src, dst)
    return dst


def blocks():
    """The README's blocks of `olr-cli` lines before its first `## `
    heading, each a list of argument lists."""
    out, block = [], []
    for line in open(README, encoding="utf-8").read().split("\n"):
        if line.startswith("## "):
            break
        line = line.strip()
        if line.startswith("olr-cli "):
            block.append(shlex.split(line)[1:])
        elif block:
            out.append(block)
            block = []
    if block:
        out.append(block)
    return out


def folder_of(block):
    """The folder a block's `asset save … --out` writes."""
    for args in block:
        if args[:2] == ["asset", "save"]:
            return args[args.index("--out") + 1]
    return None


def make_assets(cli):
    for args_block in blocks():
        out = folder_of(args_block)
        folder = os.path.basename(out or "")
        if folder not in NEW_ASSETS:
            continue
        source = fresh_copy(AFTER_DARK, f"source-{folder}")
        for args in args_block:
            run(cli, *[source if a == AFTER_DARK else a for a in args])


def locked_layers(cli, pack):
    """The ground's locked element ids, from `level element list`."""
    out = []
    for line in run(cli, "level", "element", "list", pack).splitlines():
        words = line.split()
        if len(words) > 1 and " locked" in line.split('"')[-1]:
            out.append(words[1])
    return out


def empty(pack, layer):
    _, _, px = decode(os.path.join(pack, "editor/layers", layer + ".png"))
    return all(p[3] == 0 for row in px for p in row)


def encode_rgba(path, w, h, rows):
    """An 8-bit RGBA PNG of `rows` (RGBA tuples)."""
    data = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)
    print(f"wrote {path}")


def wall_holes():
    """Issue #97: the crane's pixels the ground lost (After Dark's rock in
    the crane's box, air in the level's ground now) and WALL_EDGE pixels
    around them, each with its wall
    colour: blended along its row between the nearest other pixels of the
    ground's front left and right of it."""
    _, _, ad = decode(os.path.join(AFTER_DARK, "material.png"))
    _, _, ours = decode(os.path.join(OUT, "material.png"))
    _, _, front = decode(os.path.join(OUT, "front.png"))
    rock, air = (128, 128, 128), (0, 0, 0)  # material.png's palette
    bx, by, bw, bh = CRANE_BOX
    lost = {(x, y) for y in range(by, by + bh) for x in range(bx, bx + bw)
            if ad[y][x][:3] == rock and ours[y][x][:3] == air}
    # The ground's pictures are flattened: the claw's art reaches up to
    # WALL_EDGE pixels beyond its rock, so the holes do too.
    holes = {(x + dx, y + dy) for x, y in lost
             for dy in range(-WALL_EDGE, WALL_EDGE + 1) for dx in range(-WALL_EDGE, WALL_EDGE + 1)
             if bx <= x + dx < bx + bw and by <= y + dy < by + bh and ours[y + dy][x + dx][:3] == air}
    fill = {}
    for x, y in sorted(holes):
        l, r = x - 1, x + 1
        while (l, y) in holes:
            l -= 1
        while (r, y) in holes:
            r += 1
        a, b, k = front[y][l], front[y][r], (x - l) / (r - l)
        fill[(x, y)] = tuple(round(a[i] + (b[i] - a[i]) * k) for i in range(3)) + (255,)
    return fill


def paint_wall(cli):
    """Issue #97: the wall behind the crane, on the ground's backdrop."""
    fill = wall_holes()
    w, h = 1200, 480
    mask = [[(0, 0, 0, 0)] * w for _ in range(h)]
    tile = [[(0, 0, 0, 0)] * w for _ in range(h)]
    for (x, y), c in fill.items():
        mask[y][x] = (255, 255, 255, 255)
        tile[y][x] = c
    m, t = os.path.join(SCRATCH, "wall-mask.png"), os.path.join(SCRATCH, "wall-tile.png")
    encode_rgba(m, w, h, mask)
    encode_rgba(t, w, h, tile)
    run(cli, "level", "element", "draw", OUT, "backdrop", "--mask", m, "--tile", t)


def added(out):
    """The element id an `olr-cli` line `added element <id>` names."""
    return next(l.split()[-1] for l in out.splitlines() if l.startswith("added element "))


def add_group(cli, pack, gid, colour, intensity, reach, animation):
    args = ["level", "group", "add", pack, gid, "--colour", colour, "--intensity", str(intensity),
            "--reach", str(reach)]
    run(cli, *(args + (["--animation", animation] if animation else [])))


def grey_mask(name, pixels, value=255):
    """A level-sized grey PNG, `value` at `pixels`, 0 elsewhere."""
    rows = [[0] * 1200 for _ in range(480)]
    for x, y in pixels:
        rows[y][x] = value
    path = os.path.join(SCRATCH, name)
    png(path, 1200, 480, rows, False)
    return path


def white_mask(name, pixels):
    """A level-sized RGBA PNG, white at `pixels` (a `--mask` shape)."""
    rows = [[(0, 0, 0, 0)] * 1200 for _ in range(480)]
    for x, y in pixels:
        rows[y][x] = (255, 255, 255, 255)
    path = os.path.join(SCRATCH, name)
    encode_rgba(path, 1200, 480, rows)
    return path


def night_shift(cli):
    """The night shift's lights, on the ground while it is alone."""
    for g in GROUPS:
        add_group(cli, OUT, *g)
    el = lambda *a: run(cli, "level", "element", *a)
    # The mine lamps.
    mine = added(el("add-layer", OUT, "--material", "background", "--name", "Mine lamps"))
    bulbs = []
    for x, y in MINE_LAMPS:
        for r in ((x - 1, y - 1, 4, 1), (x - 1, y, 1, 2), (x + 2, y, 1, 2)):
            el("draw", OUT, mine, "--rect", ",".join(map(str, r)), "--colour", PLATE)
        el("draw", OUT, mine, "--rect", f"{x},{y},2,2", "--colour", BULB)
        bulbs += [(x + dx, y + dy) for dx in (0, 1) for dy in (0, 1)]
    el("set", OUT, mine, "--glow", "100", "--glow-group", "mine")
    el("glow", OUT, mine, "--mask", grey_mask("mine-glow.png", bulbs), "--strength", "100")
    # The lab strips.
    lab = added(el("extract", OUT, "lamps", "--rect", ",".join(map(str, LAB_STRIPS)), "--name", "Lab strips"))
    el("set", OUT, lab, "--glow-group", "cool")
    # The high-bays.
    bays = added(el("add-layer", OUT, "--material", "rock", "--name", "High-bays"))
    faces = []
    for x in HIGH_BAYS:
        el("draw", OUT, bays, "--rect", f"{x},{BAY_ROW},{BAY_WIDTH},1", "--colour", BAY_PLATE)
        el("draw", OUT, bays, "--rect", f"{x},{BAY_ROW + 1},{BAY_WIDTH},1", "--colour", BAY_FACE)
        faces += [(x + dx, BAY_ROW + 1) for dx in range(BAY_WIDTH)]
    el("set", OUT, bays, "--glow", "100", "--glow-group", "sodium")
    el("glow", OUT, bays, "--mask", grey_mask("bays-glow.png", faces), "--strength", "100")
    # The flue: the duct's saturated reds.
    _, _, rock = decode(os.path.join(OUT, "editor/layers/rock.png"))
    fx, fy, fw, fh = FLUE_BOX
    reds = [(x, y) for y in range(fy, fy + fh) for x in range(fx, fx + fw)
            if rock[y][x][3] and rock[y][x][0] > 150 and rock[y][x][1] < 60 and rock[y][x][2] < 60]
    flue = added(el("extract", OUT, "rock", "--mask", white_mask("flue.png", reds), "--name", "Flue"))
    el("set", OUT, flue, "--glow", str(FLUE_GLOW), "--glow-group", "flue")
    # The aviation light.
    avi = added(el("add-layer", OUT, "--material", "background", "--name", "Aviation"))
    el("draw", OUT, avi, "--rect", ",".join(map(str, AVIATION)), "--colour", AVIATION_ART)
    el("set", OUT, avi, "--glow", "100", "--glow-group", "aviation")
    return [mine, lab, bays, flue, avi]


def make_skyline(cli):
    """The skyline's two layers as the level's own assets, each placed
    after the sky (distant layers draw in element order)."""
    pictures = write_skyline(os.path.join(SCRATCH, "skyline"))
    for name, picture, key, cid, depth, group in SKYLINE:
        art, glow = pictures[picture]
        source = fresh_copy(AFTER_DARK, f"source-{picture}")
        add_group(cli, source, *group)
        el = added(run(cli, "level", "element", "add-image", source, art, "--at", "0,0", "--size", "1200x480",
                       "--material", "background", "--name", picture.capitalize()))
        run(cli, "level", "element", "set", source, el, "--glow", "100", "--glow-group", group[0])
        run(cli, "level", "element", "glow", source, el, "--mask", glow, "--strength", "100")
        asset = os.path.join(SCRATCH, f"night-{picture}")
        shutil.rmtree(asset, ignore_errors=True)
        run(cli, "asset", "save", source, "--elements", el, "--name", name, "--out", asset)
        run(cli, "level", "copy", "add", OUT, asset, "--at", "0,0", "--key", key, "--id", cid)
        run(cli, "level", "copy", "set", OUT, cid, "--distant", str(depth))


def make_level(cli):
    shutil.rmtree(OUT, ignore_errors=True)
    run(cli, "pack", "new", "level", ID, "--from", AFTER_DARK, "--keep-origin", "--name", NAME, "--out", OUT)
    run(cli, "level", "notes", OUT, NOTES)
    # The ground: no sky, and no piece's pixels.
    locked = locked_layers(cli, OUT)
    for layer in locked:
        run(cli, "level", "element", "set", OUT, layer, "--locked", "false")
    run(cli, "level", "element", "remove", OUT, "sky")
    cut = []
    for _, _, layers, (w, h), copies in PIECES:
        for _, (x, y) in copies:
            for layer in layers:
                run(cli, "level", "element", "erase", OUT, layer, "--rect", f"{x},{y},{w},{h}")
                cut.append(layer)
    for layer in sorted(set(cut)):
        if empty(OUT, layer):
            run(cli, "level", "element", "remove", OUT, layer)
    paint_wall(cli)
    locked += night_shift(cli)
    gone = {"sky"} | {l for l in cut if not os.path.exists(os.path.join(OUT, "editor/layers", l + ".png"))}
    for layer in locked:
        if layer not in gone:
            run(cli, "level", "element", "set", OUT, layer, "--locked", "true")
    # The night sky, the level's own, distant as After Dark's.
    sky_source = fresh_copy(AFTER_DARK, "source-sky")
    run(cli, "level", "element", "set", sky_source, "sky", "--locked", "false", "--distant", "0")
    sky = os.path.join(SCRATCH, "night-sky")
    shutil.rmtree(sky, ignore_errors=True)
    run(cli, "asset", "save", sky_source, "--elements", "sky", "--name", "Night sky", "--out", sky)
    run(cli, "level", "copy", "add", OUT, sky, "--at", "0,0", "--key", "y", "--id", "sky")
    run(cli, "level", "copy", "set", OUT, "sky", "--distant", "90")
    make_skyline(cli)
    # The pieces, built in.
    for key, folder, _, _, copies in PIECES:
        for i, (cid, (x, y)) in enumerate(copies):
            args = ["level", "copy", "add", OUT, os.path.join(ASSETS, folder), "--at", f"{x},{y}", "--id", cid]
            run(cli, *(args + (["--key", key] if i == 0 else [])))
    run(cli, "level", "library", OUT)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: python3 -I tools/levels/factory_assets.py <olr-cli>")
    cli = os.path.abspath(sys.argv[1])
    os.makedirs(SCRATCH, exist_ok=True)
    make_assets(cli)
    make_level(cli)


if __name__ == "__main__":
    main()
