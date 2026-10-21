# Refines Firefly (issue #13), OLR's double-resolution sample skin, from
# OLX's default skin doubled by `olr-cli skin double`: the body shaded at
# the finer resolution (lit from the top left, a dark rim below, the gun's
# shadow), rings along the body, eyes with a pupil looking where the worm
# aims, the gun's metal shaded, and the tail tip a firefly's lantern that
# glows and lights the worm's surroundings (`skin.ron`'s light), and
# OLX's default skin's origin in `pack.ron`. The commands are in
# `content/reloaded/README.md`:
#
#   python3 -I tools/skins/firefly.py <the doubled skin> content/packs/olx-skin-default

import heapq, math, os, struct, sys, zlib


def decode(path):
    """An 8-bit, non-interlaced PNG's pixels as RGBA tuples (grey as (v, v, v, 255))."""
    b = open(path, "rb").read()
    assert b[:8] == b"\x89PNG\r\n\x1a\n", path
    i, idat, plte, trns = 8, b"", None, None
    while i < len(b):
        n = struct.unpack(">I", b[i : i + 4])[0]
        t, d = b[i + 4 : i + 8], b[i + 8 : i + 8 + n]
        i += 12 + n
        if t == b"IHDR":
            w, h, depth, ct, _, _, il = struct.unpack(">IIBBBBB", d)
        elif t == b"IDAT":
            idat += d
        elif t == b"PLTE":
            plte = d
        elif t == b"tRNS":
            trns = d
    assert depth == 8 and il == 0, (path, depth, il)
    ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct]
    raw, stride, prev, p, px = zlib.decompress(idat), w * ch, bytearray(w * ch), 0, []
    for _ in range(h):
        f, line = raw[p], bytearray(raw[p + 1 : p + 1 + stride])
        p += 1 + stride
        for x in range(stride):
            a = line[x - ch] if x >= ch else 0
            c = prev[x - ch] if x >= ch else 0
            up = prev[x]
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + up) & 255
            elif f == 3:
                line[x] = (line[x] + (a + up) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(up - c), abs(a - c), abs(a + up - 2 * c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else up if pb <= pc else c)) & 255
        row = []
        for x in range(w):
            v = line[x * ch : (x + 1) * ch]
            if ct == 6:
                row.append(tuple(v))
            elif ct == 2:
                row.append(tuple(v) + (255,))
            elif ct in (0, 4):
                row.append((v[0], v[0], v[0], 255))
            else:
                k = v[0]
                row.append(tuple(plte[3 * k : 3 * k + 3]) + (trns[k] if trns and k < len(trns) else 255,))
        px.append(row)
        prev = line
    return w, h, px


def encode(path, w, h, values, grey):
    rows = b"".join(b"\x00" + bytes(v for p in row for v in ((p,) if grey else p)) for row in values)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 0 if grey else 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(png)


FW, FH, AIM, WALK = 40, 36, 7, 3
# OLX's default skin's darkest body shade marks its eye.
OLX_EYE = (57, 90, 148)
# The body's shades, dark to light: OLX's six blues and one more at each end.
RAMP = [
    (0.0, (36, 58, 100)),
    (0.15, (57, 90, 148)),
    (0.3, (74, 123, 198)),
    (0.45, (82, 140, 231)),
    (0.6, (99, 156, 255)),
    (0.72, (115, 165, 255)),
    (0.85, (132, 181, 255)),
    (1.0, (165, 205, 255)),
]
OLX_LEVEL = {c: s for s, c in RAMP}
# The lantern: a radius along the body from the tail's tip, its colours from
# the tip out.
LANTERN = 10.0
LANTERN_CORE, LANTERN_EDGE = (255, 252, 205), (196, 236, 74)
# The light from the top left.
LIGHT = (-0.6, -0.8)


def ramp(s):
    s = min(1.0, max(0.0, s))
    for (s0, c0), (s1, c1) in zip(RAMP, RAMP[1:]):
        if s <= s1:
            k = (s - s0) / (s1 - s0)
            return tuple(round(a + (b - a) * k) for a, b in zip(c0, c1))
    return RAMP[-1][1]


def lerp(c0, c1, k):
    return tuple(round(a + (b - a) * k) for a, b in zip(c0, c1))


def normal(mask, x, y, r=3):
    """Points out of `mask` at (x, y): the sum of the offsets of the
    pixels within `r` that are outside it."""
    nx = ny = 0
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy <= r * r and (x + dx, y + dy) not in mask:
                nx, ny = nx + dx, ny + dy
    n = math.hypot(nx, ny)
    return (nx / n, ny / n) if n else (0.0, 0.0)


def edge_distance(mask, x, y, most=6):
    for d in range(1, most):
        for dy in range(-d, d + 1):
            for dx in range(-d, d + 1):
                if max(abs(dx), abs(dy)) == d and (x + dx, y + dy) not in mask:
                    return d
    return most


def geodesic(mask, start):
    """Distances within `mask` from `start`, 8-connected (diagonal √2)."""
    dist, todo = {start: 0.0}, [(0.0, start)]
    while todo:
        d, p = heapq.heappop(todo)
        if d > dist.get(p, 1e9):
            continue
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)):
            q = (p[0] + dx, p[1] + dy)
            nd = d + (1.4142 if dx and dy else 1.0)
            if q in mask and nd < dist.get(q, 1e9):
                dist[q] = nd
                heapq.heappush(todo, (nd, q))
    return dist


def refine(img, tint, glow, olx, a, r):
    ox, oy = a * FW, r * FH
    at = lambda x, y: (ox + x, oy + y)
    opaque = {(x, y) for y in range(FH) for x in range(FW) if img[oy + y][ox + x][3]}
    body = {p for p in opaque if tint[oy + p[1]][ox + p[0]]}
    gun = opaque - body
    # OLX's eye pixel, doubled.
    eyes = [(x, y) for y in range(10) for x in range(20) if olx[r * 18 + y][a * 20 + x][:3] == OLX_EYE]
    ex, ey = 2 * eyes[0][0], 2 * eyes[0][1]
    # The white is 4 x 3 from (ex - 1, ey); where it would stick out of the
    # head it moves back and down, else it narrows to 3 wide.
    fits = lambda x0, y0, w: all((x, y) in body for x in range(x0 - 1, x0 + w - 1) for y in range(y0, y0 + 3))
    spots = [(ex - dx, ey + dy, 4) for dx in range(4) for dy in range(2)]
    spots += [(ex - dx, ey + dy, 3) for dx in range(3) for dy in range(2)]
    ex, ey, ew = next((p for p in spots if fits(*p)), (ex, ey, 3))
    tip = min(body, key=lambda p: (p[0] - p[1], p[0]))
    geo = geodesic(body, tip)
    out = {}
    for (x, y) in body:
        X, Y = at(x, y)
        g = geo.get((x, y), 1e9)
        n = normal(opaque, x, y, 4)
        lit = n[0] * LIGHT[0] + n[1] * LIGHT[1]
        d = edge_distance(opaque, x, y)
        if g <= LANTERN:
            k = g / LANTERN
            c = lerp(LANTERN_CORE, LANTERN_EDGE, k * k)
            if d == 1 and lit < -0.2:
                c = lerp(c, (120, 170, 40), 0.5)
            out[(x, y)] = (c, 0, 255 if k < 0.75 else 192)
            continue
        s = OLX_LEVEL.get(tuple(img[Y][X][:3]), 0.6)
        reach = {1: 1.0, 2: 0.75, 3: 0.45}.get(d, 0.2)
        s = 0.3 * s + 0.7 * (0.58 + 0.42 * lit * reach)
        if d == 1 and lit < 0.1:
            s -= 0.2
        elif d == 1 and lit > 0.6:
            s += 0.12
        # A shine on the head, up and left of the eye.
        if d == 2 and lit > 0.8 and math.hypot(x - ex, y - ey) < 10:
            s += 0.3
        # The gun's shadow on the body below it.
        if (x, y - 1) in gun or (x, y - 2) in gun:
            s -= 0.18
        # The segment edge behind the lantern, then a ring every 7 pixels
        # along the body, the head left plain.
        if g <= LANTERN + 1.6:
            s -= 0.25
        elif math.hypot(x - ex, y - ey) > 9 and (g - LANTERN) % 6.0 < 1.2:
            s -= 0.2
        out[(x, y)] = (ramp(s), 255, 0)
    for (x, y) in gun:
        X, Y = at(x, y)
        v = img[Y][X][0]
        n = normal(gun, x, y, 2)
        lit = n[0] * LIGHT[0] + n[1] * LIGHT[1]
        if edge_distance(gun, x, y) == 1:
            v += 28 * lit if lit > 0 else 48 * lit
        v = max(70, min(240, round(v)))
        out[(x, y)] = ((v, v, min(255, v + 6)), 0, 0)
    # The eye: white, a lid above it, the pupil towards the aim.
    py = ey if a <= 2 else ey + 1
    for x in range(ex - 1, ex + ew - 1):
        if (x, ey - 1) in body and (x, ey - 1) in out and out[(x, ey - 1)][1]:
            out[(x, ey - 1)] = (ramp(0.12), 255, 0)
        for y in range(ey, ey + 3):
            if (x, y) in body:
                pupil = x >= ex + ew - 3 and py <= y <= py + 1
                out[(x, y)] = ((18, 20, 32) if pupil else (236, 240, 232), 0, 0)
    for (x, y), (c, t, gl) in out.items():
        X, Y = at(x, y)
        img[Y][X] = c + (255,)
        tint[Y][X] = t
        glow[Y][X] = gl


src, olx_dir = sys.argv[1], sys.argv[2]
w, h, img = decode(os.path.join(src, "skin.png"))
_, _, tint = decode(os.path.join(src, "tint.png"))
_, _, olx = decode(os.path.join(olx_dir, "skin.png"))
assert (w, h) == (AIM * FW, WALK * FH), (w, h)
img = [list(row) for row in img]
tint = [[p[0] for p in row] for row in tint]
glow = [[0] * w for _ in range(h)]
for r in range(WALK):
    for a in range(AIM):
        refine(img, tint, glow, olx, a, r)
encode(os.path.join(src, "skin.png"), w, h, img, False)
encode(os.path.join(src, "tint.png"), w, h, tint, True)
encode(os.path.join(src, "glow.png"), w, h, glow, True)
# The lantern's light: modest, well within the engine's caps (200 %, 48).
ron = os.path.join(src, "skin.ron")
text = open(ron).read()
assert "light:" not in text and text.endswith(")\n"), text
open(ron, "w").write(text[:-2] + "    light: (\n        intensity: 80,\n        reach: 24,\n    ),\n)\n")
# Made from OLX's default skin: its origin (the manifest, which
# `olr-cli skin double` cleared).
pack = os.path.join(src, "pack.ron")
text, olx_text = open(pack).read(), open(os.path.join(olx_dir, "pack.ron")).read()
origin = olx_text[olx_text.index("    origin:") : olx_text.index("    license:")]
assert "    origin: None,\n" in text, text
open(pack, "w").write(text.replace("    origin: None,\n", origin))
