# Liero Factory Reloaded, the plant works (2026-11-04, the maintainer's go
# on the "Factory Pass Mockups" page): the art, the ground's edits and the
# new copies of the pass, imported by `factory_assets.py` (no `olr-cli`
# runs here but through the `run` it is handed):
#
# - `make_art()` writes the new assets' inputs into `tools/levels/factory/
#   art/` from After Dark's layers (committed, read by the blocks in
#   `content/assets/README.md`): level-sized `<name>-mask.png` and
#   `<name>-tile.png` pairs that `level element draw --mask … --tile …`
#   paints (white where painted, the colours in the tile), level-sized
#   grey glow masks (`<name>-glow.png`) and the state pictures, each the
#   size of its layer's box.
# - `ground(run, out)` edits the ground while it is alone (issue #75):
#   the pixels the Dirt-ball line takes but its stamp's (the belt's stripe,
#   the drive's casing, the dirt balls painted on the incline: the stamp
#   is the Console's piece), the lift's shaft at the hall's far right down
#   into the mine's big chamber, the mine cart's rails, the truck's hazard
#   lights and lit cab, and the line's end crate made rock. (The emergency
#   lamps, red bulbs lit in a blackout, are gone: the user's playtest,
#   2026-11-05, their bulbs read as red dots hanging in the air.)
# - `place(run, out)` places the copies new to the level (the lift, the
#   mine cart, the forklift); the others take pieces' places
#   (`factory_assets.py`'s `PIECES`).
#
# Running it again makes the same bytes.

import os, struct, sys, zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reloaded_materials import decode  # noqa: E402

AFTER_DARK = "content/reloaded/olr-level-liero-factory-after-dark"
ART = "tools/levels/factory/art"
SCRATCH = "target/factory-assets"
W, H = 1200, 480

# The Dirt-ball line (one copy, so one script keeps the stamp, the belt and
# the balls in step): the stamp's housing (After Dark's rock under the 13-37
# sign's place), its die (the cylinder and the pressing plate) and a ram
# drawn behind the housing that shows as the die drops; the LED column as
# the stamp's countdown; the belt's red stripe (flat run and incline) as
# the line's status light; six dirt balls; the drive's casing over them
# (they roll through it, the user's playtest, 2026-11-05) and its running
# lamp on it.
LINE_RECT = (909, 269, 150, 88)
HOUSING = (933, 269, 33, 57)
DIE = (933, 326, 33, 7)
DROP = 18  # the die's travel: the plate's bottom row (332) onto the belt (351)
RAM = (941, 308, 17, 18)  # behind the housing, the die's width, DROP rows
LEDS = (942, 279, 1, 19)
LED_ROWS = range(279, 298, 3)
LED_LIT, LED_DARK = (255, 255, 0), (24, 24, 16)
STRIPE_BOXES = [(909, 352, 80, 5), (1022, 326, 29, 30)]  # flat run, incline
STRIPE_RUN, STRIPE_EDGE_RUN = (236, 236, 244), (120, 122, 132)
STRIPE_GLOW = 170
DRIVE = (989, 342, 33, 15)  # the drive's casing within the line's box (down to row 364)
DRIVE_LAMP = (1015, 345, 2, 1)
LAMP_OFF, LAMP_ON = (30, 58, 34), (90, 255, 110)
BALL_AT = (958, 346)  # a ball's top-left leaving the stamp, on the belt
BALL = (1024, 343)  # the lowest of the incline's painted balls (4 x 4)
INCLINE_BALLS = (1015, 315, 38, 37)  # their box (the crate's dirt from x 1053)
BALLS = 6
BALL_STEP = 4  # each ball's own place along the flat run, ball i at BALL_AT + 4 (i - 1)
# The crate at the line's end, which the balls fall into: After Dark's dirt,
# made rock (the user's playtest, 2026-11-05: a crate dug away left the
# balls vanishing in the air), wood.
CRATE = (1053, 330, 60, 35)

# The lift: a shaft at the hall's far right, right of the ramp's mouth
# (open to x 1184 on the hall's floor row, 366), from the floor down
# through the mine's big chamber's right wall to a landing (rows 456-458);
# its cage's floor flush with the hall's at the top.
SHAFT = (1186, 352, 12, 104)  # the copy: the cage's frame above the floor, its travel
SHAFT_CUT = (1186, 366, 12, 90)  # the rock taken out
PIT = (1186, 456, 12, 3)  # the landing, drawn level
FLOOR_TOP = 366
LIFT_TRAVEL = 87  # the cage floor's top from row 366 to 453
CAGE = 1187  # the cage's left column (10 wide)
SHAFT_WALL, SHAFT_RAIL, SHAFT_RAIL_D = (44, 46, 52), (110, 114, 122), (64, 66, 72)
STEEL, STEEL_D, STEEL_L = (110, 114, 122), (64, 66, 72), (160, 164, 172)
HAZ_Y, HAZ_K = (230, 180, 30), (30, 30, 30)

# The mine cart: rails on the left tunnel's floor (row 473, bridging the
# pit at x 140-175) from x 10 to where the floor rises (x 190).
RAILS = (10, 473, 181)
RAIL, SLEEPER = (96, 80, 64), (70, 50, 30)
CART_RECT = (12, 456, 187, 17)
CART_AT = 14  # the cart's left column at the tunnel's left end

# The forklift at the warehouse's right end (floor row 404), its forks
# towards the crate stack (its right edge x 533), the crate on them.
FORK_RECT = (536, 374, 52, 30)
FORK_TRAVEL = 14  # back towards the ramp (rock from x 588 at row 402)
# The doorway's threshold plate behind the forklift (rock on row 403): made
# flush with the floor (its art on the backdrop), so the wheels roll over.
THRESHOLD = (567, 403, 13)

# The generator: the gauge's mounting (After Dark's rock left and right of
# it), status lamps on its left bracket, a vent right of the gauge.
GEN_RECT = (1046, 146, 72, 25)
GEN_LAMPS = [(1052, 154, (60, 255, 90)), (1052, 157, (60, 255, 90)), (1052, 160, (255, 190, 40)),
             (1052, 163, (60, 255, 90))]
GEN_VENT = (1108, 147, 9, 7)
SOOT = 35  # percent of its colours a wrecked part keeps

# The siren: the roof's two speaker rings (the warning beacon's and the one
# right of it), two lenses each at its middle for turning beams.
SIREN_RECT = (951, 45, 62, 28)
LENSES = [("lens-a", 961, 180), ("lens-b", 964, 0), ("lens-c", 997, 180), ("lens-d", 1000, 0)]
LENS_Y, LENS_ART = 55, (255, 120, 80)

# The lab bench: the flask's liquid (After Dark's olive dirt) and a burner
# flame over the red heater under the still.
LAB_RECT = (370, 205, 22, 17)
FLASK = (372, 208, 9, 14)
FLASK_LIQUID = (120, 230, 110)
BURNER = (382, 217, 8, 1)
BURNER_ART = (90, 140, 255)

# The window panes that shatter (the intact ones of the upper hall's big
# window; its second, fifth and sixth are broken already).
PANE = (719, 151, 19, 22)
PANES = [("pane-1", (719, 151)), ("pane-3", (759, 151)), ("pane-4", (779, 151))]

# The truck: hazard lights front and rear, its dark cab window lit dimly.
HAZARDS = [(43, 405, 1, 2), (175, 404, 2, 2)]  # beside the headlight, above the tail light: on the truck's rock
HAZARD_GROUP = ("hazard", "ffa020", 300, 24, "Pulse(period_ms: 1000, low: 0, high: 100, shape: Square)")
HAZARD_ART = "ffaa28"
CAB_BOX = (50, 378, 23, 16)
CAB_GROUP = ("cab", "ffd8a0", 140, 22, None)
CAB_GLOW = 60

# The Strip light's broken tube: its unlit grey, the middle shattered.
STRIP_OFF = (150, 150, 156)
STRIP_GAP = (11, 7)  # the missing glass's first column and width


def png(path, w, h, rows, colour):
    """An 8-bit PNG of `rows`: RGBA tuples (`colour`) or grey values."""
    if colour:
        data = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
        ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    else:
        data = b"".join(b"\x00" + bytes(row) for row in rows)
        ihdr = struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b""))
    print(f"wrote {path}")


def paint(name, pixels, folder=ART):
    """`<name>-mask.png` and `<name>-tile.png`, level-sized: white and the
    colour (RGB) at each of `pixels` ({(x, y): rgb})."""
    mask = [[(0, 0, 0, 0)] * W for _ in range(H)]
    tile = [[(0, 0, 0, 0)] * W for _ in range(H)]
    for (x, y), c in pixels.items():
        mask[y][x] = (255, 255, 255, 255)
        tile[y][x] = tuple(c) + (255,)
    png(os.path.join(folder, f"{name}-mask.png"), W, H, mask, True)
    png(os.path.join(folder, f"{name}-tile.png"), W, H, tile, True)


def glow_mask(name, pixels, folder=ART):
    """`<name>-glow.png`, level-sized grey: each of `pixels` ({(x, y): v})."""
    rows = [[0] * W for _ in range(H)]
    for (x, y), v in pixels.items():
        rows[y][x] = v
    png(os.path.join(folder, f"{name}-glow.png"), W, H, rows, False)


def white(name, pixels, folder):
    """A level-sized RGBA mask (a `--mask` shape), white at `pixels`."""
    rows = [[(0, 0, 0, 0)] * W for _ in range(H)]
    for x, y in pixels:
        rows[y][x] = (255, 255, 255, 255)
    path = os.path.join(folder, name)
    png(path, W, H, rows, True)
    return path


def picture(name, box, pixels, colour=True):
    """A state picture of a layer's box (x, y, w, h): `pixels` ({(x, y):
    RGBA or grey}, world places), the rest uncovered (or 0)."""
    x0, y0, w, h = box
    empty = (0, 0, 0, 0) if colour else 0
    rows = [[empty] * w for _ in range(h)]
    for (x, y), c in pixels.items():
        rows[y - y0][x - x0] = c
    png(os.path.join(ART, name), w, h, rows, colour)


def inside(box, x, y):
    return box[0] <= x < box[0] + box[2] and box[1] <= y < box[1] + box[3]


def layer(name):
    return decode(os.path.join(AFTER_DARK, "editor/layers", name + ".png"))[2]


def bbox(pixels):
    xs, ys = [p[0] for p in pixels], [p[1] for p in pixels]
    return (min(xs), min(ys), max(xs) - min(xs) + 1, max(ys) - min(ys) + 1)


def stripe_pixels(rock):
    """The belt's stripe: its red (bright) and dark red (edge) rock pixels."""
    out = {}
    for bx, by, bw, bh in STRIPE_BOXES:
        for y in range(by, by + bh):
            for x in range(bx, bx + bw):
                p = rock[y][x]
                if p[3] and p[1] < 70 and p[2] < 70 and p[0] > 90:
                    out[(x, y)] = p[:3]
    return out


def incline_balls(dirt):
    """The dirt balls painted on the incline (left of the crate's dirt)."""
    x0, y0, w, h = INCLINE_BALLS
    return [(x, y) for y in range(y0, y0 + h) for x in range(x0, x0 + w) if dirt[y][x][3]]


# --- the art --------------------------------------------------------------
def make_line_art(rock, dirt):
    stripe = stripe_pixels(rock)
    white("line-stripe-mask.png", stripe, ART)
    glow_mask("line-stripe", {p: STRIPE_GLOW if c[0] > 180 else 0 for p, c in stripe.items()})
    box = bbox(stripe)
    picture("line-stripe-run.png", box,
            {p: (STRIPE_RUN if c[0] > 180 else STRIPE_EDGE_RUN) + (255,) for p, c in stripe.items()})
    # The LEDs: n lit from the bottom (the count), the rest dark.
    rows = list(LED_ROWS)
    for n in range(len(rows) + 1):
        lit = set(rows[len(rows) - n:])
        picture(f"line-leds-{n}.png", LEDS, {(LEDS[0], y): (LED_LIT if y in lit else LED_DARK) + (255,) for y in rows})
        picture(f"line-leds-{n}-glow.png", LEDS, {(LEDS[0], y): 255 if y in lit else 0 for y in rows}, False)
    # The ram: the die's cylinder (its top row's columns) continued upward.
    rx, ry, rw, rh = RAM
    paint("line-ram", {(x, y): rock[326][x][:3] for y in range(ry, ry + rh) for x in range(rx, rx + rw)})
    dx, dy, dw, dh = DRIVE
    paint("line-drive", {(x, y): rock[y][x][:3] for y in range(dy, dy + dh) for x in range(dx, dx + dw)
                         if rock[y][x][3]})
    lx, ly, lw, lh = DRIVE_LAMP
    paint("line-lamp", {(x, ly): LAMP_OFF for x in range(lx, lx + lw)})
    picture("line-lamp-on.png", DRIVE_LAMP, {(x, ly): LAMP_ON + (255,) for x in range(lx, lx + lw)})
    # A ball, as the incline's lowest painted one (its dirt), leaving the stamp.
    bx, by = BALL
    ax, ay = BALL_AT
    # Each its own place along the flat run, BALL_STEP apart (a layer alike
    # in pixels to one below it would not be saved).
    for i in range(BALLS):
        paint(f"line-ball-{i + 1}", {(ax + BALL_STEP * i + x - bx, ay + y - by): dirt[y][x][:3]
                                     for y in range(by, by + 4) for x in range(bx, bx + 4) if dirt[y][x][3]})


def hazard(x, y):
    return HAZ_Y if (x // 2) % 2 == 0 else HAZ_K


def make_lift_art():
    """The cage at the top: its floor (rock, three rows from the hall
    floor's, hazard striped, a blinking light at each end) and its frame
    (posts, roof bar and its lamp)."""
    floor, frame = {}, {}
    x0, x1, f = CAGE, CAGE + 10, FLOOR_TOP
    for x in range(x0, x1):
        floor[(x, f)] = hazard(x, f)
        floor[(x, f + 1)] = STEEL
        floor[(x, f + 2)] = STEEL_D
    blinks = {(x0, f): (255, 170, 40), (x1 - 1, f): (255, 170, 40)}
    floor.update(blinks)
    for y in range(f - 13, f):
        frame[(x0, y)] = STEEL_L
        frame[(x1 - 1, y)] = STEEL_L
    for x in range(x0, x1):
        frame[(x, f - 14)] = STEEL
    lamp = {(x, f - 13): (255, 240, 210) for x in range(x0 + 3, x1 - 3)}
    frame.update(lamp)
    paint("lift-floor", floor)
    glow_mask("lift-floor", {p: 255 for p in blinks})
    paint("lift-frame", frame)
    glow_mask("lift-frame", {p: 255 for p in lamp})


def make_cart_art():
    """The cart at the tunnel's left end: its body and wheels on the rails
    (rows 470-472), a load of dirt over it, a lamp at each end."""
    cart, base, cx = {}, 472, CART_AT
    body = [(1, 0, 16), (0, 1, 18), (0, 2, 18), (0, 3, 18), (1, 4, 16), (1, 5, 16), (2, 6, 14)]
    for o, j, w in body:
        for x in range(cx + o, cx + o + w):
            c = (122, 70, 40) if j in (0, 1) else (92, 58, 36)
            if x in (cx + o, cx + o + w - 1):
                c = (60, 40, 28)
            cart[(x, base - 9 + j)] = c
    for x in range(cx + 2, cx + 16):
        for j in range(2 + (x * 7 % 3)):
            cart[(x, base - 10 - j)] = (150, 96, 40) if j else (176, 116, 52)
    for wx in (cx + 3, cx + 12):
        for dx in range(3):
            for dy in range(3):
                cart[(wx + dx, base - 2 + dy)] = (140, 140, 150) if (dx, dy) == (1, 1) else (40, 40, 44)
    paint("cart", cart)
    paint("cart-lamp", {(cx + 18, base - 6): (255, 230, 170)})
    paint("cart-lamp-b", {(cx - 1, base - 6): (255, 230, 170)})


def make_forklift_art(front):
    """The forklift at rest, its forks 3 px from the stack, a crate of the
    stack's on them; its amber beacon and the mast's work light."""
    body, floor = {}, 404
    rect = lambda x0, y0, x1, y1, c: body.update({(x, y): c for y in range(y0, y1) for x in range(x0, x1)})
    mast = 554
    rect(mast + 2, floor - 12, mast + 16, floor - 3, (220, 170, 30))  # body
    rect(mast + 2, floor - 12, mast + 16, floor - 11, (250, 210, 70))
    rect(mast + 14, floor - 16, mast + 18, floor - 3, (60, 60, 66))  # counterweight
    rect(mast + 2, floor - 23, mast + 15, floor - 22, (40, 40, 44))  # the cab's roof
    for y in range(floor - 26, floor - 2):  # the mast
        body[(mast, y)] = STEEL_D
        body[(mast + 1, y)] = STEEL
    rect(mast - 16, floor - 3, mast, floor - 2, STEEL_D)  # the forks
    for j in range(17):  # a crate of the stack beside it
        for i in range(16):
            p = front[386 + j][517 + i]
            if p[3]:
                body[(mast - 16 + i, floor - 20 + j)] = p[:3]
    for wx in (mast + 4, mast + 13):
        for dx in range(-3, 4):
            for dy in range(-3, 4):
                if dx * dx + dy * dy <= 9:
                    body[(wx + dx, floor - 4 + dy)] = (150, 150, 156) if dx * dx + dy * dy <= 1 else (32, 32, 36)
    frame = {}
    for y in range(floor - 22, floor - 12):  # the cab's posts, the seat
        frame[(mast + 3, y)] = (40, 40, 44)
        frame[(mast + 13, y)] = (40, 40, 44)
    for y in range(floor - 17, floor - 12):
        for x in range(mast + 7, mast + 11):
            frame[(x, y)] = (60, 70, 90)
    paint("forklift", body)
    paint("forklift-cab", frame)
    beacon = {(mast + 8, floor - 24): (255, 160, 30), (mast + 9, floor - 24): (255, 160, 30)}
    paint("forklift-beacon", beacon)
    glow_mask("forklift-beacon", {p: 255 for p in beacon})
    paint("forklift-light", {(mast - 1, floor - 27): (255, 244, 210), (mast - 1, floor - 26): (255, 244, 210)})


def make_generator_art(rock):
    """The generator's lamps and vent, and its wrecked pictures (its
    housing sooted, lamps and vent dark)."""
    lamps = {}
    for x, y, c in GEN_LAMPS:
        lamps[(x, y)] = c
        lamps[(x + 1, y)] = c
    paint("gen-lamps", lamps)
    vx, vy, vw, vh = GEN_VENT
    vent = {(x, y): (200, 80, 30) if (y - vy) % 2 == 0 else (50, 50, 56)
            for y in range(vy, vy + vh) for x in range(vx, vx + vw)}
    for x in range(vx - 1, vx + vw + 1):
        vent[(x, vy - 1)] = (70, 70, 76)
        vent[(x, vy + vh)] = (70, 70, 76)
    paint("gen-vent", vent)
    glow_mask("gen-vent", {p: 180 for p, c in vent.items() if c == (200, 80, 30)})
    soot = lambda c: tuple(round(v * SOOT / 100) for v in c[:3]) + (255,)
    gx, gy, gw, gh = GEN_RECT
    housing = {(x, y): soot(rock[y][x]) for y in range(gy, gy + gh) for x in range(gx, gx + gw)
               if rock[y][x][3] and not inside((1059, 144, 48, 27), x, y)}
    picture("gen-housing-wrecked.png", bbox(housing), housing)
    picture("gen-lamps-wrecked.png", bbox(lamps), {p: soot(c) for p, c in lamps.items()})
    picture("gen-vent-wrecked.png", bbox(vent), {p: soot(c) for p, c in vent.items()})


def make_lab_art(dirt):
    fx, fy, fw, fh = FLASK
    liquid = {(x, y): FLASK_LIQUID for y in range(fy, fy + fh) for x in range(fx, fx + fw) if dirt[y][x][3]}
    paint("lab-liquid", liquid)
    bx, by, bw, _ = BURNER
    paint("lab-burner", {(x, by): BURNER_ART for x in range(bx, bx + bw)})
    return liquid


def make_pane_art(glass, rim):
    """A pane's Broken picture: only a jagged rim of its glass left at its
    frame (one to three pixels, drawn from a fixed sequence), the rim
    colour of After Dark's broken panes."""
    x0, y0, w, h = PANE
    rim_colour = next(rim[y][x][:3] for y in range(151, 173) for x in range(739, 758) if rim[y][x][3])
    jag = [1, 2, 1, 3, 2, 1, 1, 2, 3, 2, 1, 2, 1, 1, 3, 2, 1, 2, 2, 1, 3, 1]
    keep = {}
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            if not glass[y][x][3]:
                continue
            i, j = x - x0, y - y0
            edge = min(j, h - 1 - j, i, w - 1 - i)
            if edge < jag[(i * 3 + j * 5) % len(jag)] - 1:
                keep[(x, y)] = rim_colour + (255,)
    picture("pane-broken.png", bbox([(x, y) for y in range(y0, y0 + h) for x in range(x0, x0 + w) if glass[y][x][3]]), keep)


def make_strip_art(front):
    """The Strip light's Broken picture: the unlit tube, its middle gone."""
    x0, y0 = 778, 80
    rows = []
    for y in range(2):
        row = []
        for x in range(29):
            p = front[y0 + y][x0 + x]
            k = max(p[:3]) / 255
            gone = STRIP_GAP[0] <= x < STRIP_GAP[0] + STRIP_GAP[1]
            row.append((0, 0, 0, 0) if gone else tuple(round(c * k * 0.7) for c in STRIP_OFF) + (255,))
        rows.append(row)
    png(os.path.join(ART, "strip-broken.png"), 29, 2, rows, True)


def make_art():
    os.makedirs(ART, exist_ok=True)
    rock, dirt, glass, rim = layer("rock"), layer("dirt"), layer("glass"), layer("glass-rim")
    _, _, front = decode(os.path.join(AFTER_DARK, "front.png"))
    make_line_art(rock, dirt)
    make_lift_art()
    make_cart_art()
    make_forklift_art(front)
    make_generator_art(rock)
    make_lab_art(dirt)
    make_pane_art(glass, rim)
    make_strip_art(front)


# --- the ground -----------------------------------------------------------
def added(out):
    return next(l.split()[-1] for l in out.splitlines() if l.startswith("added element "))


def add_group(run, pack, gid, colour, intensity, reach, animation):
    args = ["level", "group", "add", pack, gid, "--colour", colour, "--intensity", str(intensity),
            "--reach", str(reach)]
    run(*(args + (["--animation", animation] if animation else [])))


def ground(run, out):
    """The plant works' edits of the ground, while it is alone; gives the
    new elements' ids (locked again by the caller)."""
    el = lambda *a: run("level", "element", *a)
    rock, dirt = layer("rock"), layer("dirt")
    # The Dirt-ball line's pixels: the stripe, the drive's casing and the
    # incline's balls.
    el("erase", out, "rock", "--mask", white("line-stripe.png", stripe_pixels(rock), SCRATCH))
    el("erase", out, "rock", "--rect", ",".join(map(str, DRIVE)))
    el("erase", out, "dirt", "--mask", white("line-balls.png", incline_balls(dirt), SCRATCH))
    # The doorway's threshold under the forklift's travel: its art on the
    # backdrop, its rock out.
    tx, ty, tw = THRESHOLD
    plate = {(x, ty): rock[ty][x][:3] for x in range(tx, tx + tw) if rock[ty][x][3]}
    assert len(plate) == tw, plate
    el("erase", out, "rock", "--mask", white("threshold.png", plate, SCRATCH))
    paint("threshold", plate, SCRATCH)
    el("draw", out, "backdrop", "--mask", os.path.join(SCRATCH, "threshold-mask.png"),
       "--tile", os.path.join(SCRATCH, "threshold-tile.png"))
    # The lab's flask liquid (the copy's now).
    fx, fy, fw, fh = FLASK
    el("erase", out, "dirt", "--rect", f"{fx},{fy},{fw},{fh}")
    # The lift's shaft: the rock out, the wall and guide rails on the
    # backdrop, a level landing.
    sx, sy, sw, sh = SHAFT_CUT
    el("erase", out, "rock", "--rect", f"{sx},{sy},{sw},{sh}")
    shaft = {}
    for y in range(sy, sy + sh):
        for x in range(sx, sx + sw):
            shade = SHAFT_WALL[0] + (8 if (x - sx) % 6 == 0 else 0)
            shaft[(x, y)] = (shade, shade + 2, shade + 6)
        for x, c in ((sx, SHAFT_RAIL_D), (sx + 1, SHAFT_RAIL), (sx + sw - 2, SHAFT_RAIL), (sx + sw - 1, SHAFT_RAIL_D)):
            shaft[(x, y)] = c
    paint("shaft", shaft, SCRATCH)
    el("draw", out, "backdrop", "--mask", os.path.join(SCRATCH, "shaft-mask.png"),
       "--tile", os.path.join(SCRATCH, "shaft-tile.png"))
    px, py, pw, ph = PIT
    paint("shaft-rock", {(x, y): STEEL_D for y in range(py, py + ph) for x in range(px, px + pw)}, SCRATCH)
    el("draw", out, "rock", "--mask", os.path.join(SCRATCH, "shaft-rock-mask.png"),
       "--tile", os.path.join(SCRATCH, "shaft-rock-tile.png"))
    # The mine cart's rails, where the tunnel's floor row is open.
    _, _, mat = decode(os.path.join(out, "material.png"))
    rx, ry, rw = RAILS
    rails = {(x, ry): SLEEPER if x % 4 == 0 else RAIL for x in range(rx, rx + rw) if mat[ry][x][:3] == (0, 0, 0)}
    paint("rails", rails, SCRATCH)
    el("draw", out, "rock", "--mask", os.path.join(SCRATCH, "rails-mask.png"),
       "--tile", os.path.join(SCRATCH, "rails-tile.png"))
    new = []
    # The truck's hazard lights and its cab.
    add_group(run, out, *HAZARD_GROUP)
    # Rock: the lights are the truck's (its rock stays solid).
    hz = added(el("add-layer", out, "--material", "rock", "--name", "Hazards"))
    for r in HAZARDS:
        el("draw", out, hz, "--rect", ",".join(map(str, r)), "--colour", HAZARD_ART)
    el("set", out, hz, "--glow", "100", "--glow-group", HAZARD_GROUP[0])
    new.append(hz)
    add_group(run, out, *CAB_GROUP)
    cx, cy, cw, ch = CAB_BOX
    window = [(x, y) for y in range(cy, cy + ch) for x in range(cx, cx + cw)
              if rock[y][x][3] and max(rock[y][x][:3]) < 90]
    cab = added(el("extract", out, "rock", "--mask", white("cab.png", window, SCRATCH), "--name", "Cab"))
    el("set", out, cab, "--glow", str(CAB_GLOW), "--glow-group", CAB_GROUP[0])
    new.append(cab)
    # The crate at the line's end: its dirt onto a rock layer, wood.
    kx, ky, kw, kh = CRATE
    crate = {(x, y): dirt[y][x][:3] for y in range(ky, ky + kh) for x in range(kx, kx + kw) if dirt[y][x][3]}
    el("erase", out, "dirt", "--mask", white("crate.png", crate, SCRATCH))
    paint("crate", crate, SCRATCH)
    box = added(el("add-layer", out, "--material", "rock", "--name", "Crate"))
    el("draw", out, box, "--mask", os.path.join(SCRATCH, "crate-mask.png"),
       "--tile", os.path.join(SCRATCH, "crate-tile.png"))
    el("set", out, box, "--surface", "wood")
    new.append(box)
    return new


def place(run, out, assets):
    """The copies new to the level, on top of the pieces."""
    run("level", "copy", "add", out, os.path.join(assets, "lift"), "--at", f"{SHAFT[0]},{SHAFT[1]}",
        "--key", "f", "--id", "lift")
    run("level", "copy", "add", out, os.path.join(assets, "mine-cart"), "--at", f"{CART_RECT[0]},{CART_RECT[1]}",
        "--key", "m", "--id", "cart")
    run("level", "copy", "add", out, os.path.join(assets, "forklift"), "--at", f"{FORK_RECT[0]},{FORK_RECT[1]}",
        "--key", "x", "--id", "forklift")
