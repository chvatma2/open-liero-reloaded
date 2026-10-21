# Makes `look-test-skin/` (issue #12): a double-resolution skin (art scale
# 4, 40 x 36 frames) from OLX's default skin (`content/packs/olx-skin-default`),
# each art pixel doubled, the body shaded at the finer resolution (a light
# top edge and a dark bottom edge on the tinted part), the eye a cyan visor
# that glows in full with a light, and a half-glowing tip on the gun.
#
#   python3 -I fixtures/look/make-look-test-skin.py content/packs/olx-skin-default fixtures/look/skins/look-test-skin
#
# `make-look-test-skin.sh` runs it and builds the dark level it is shown on.
import os, struct, sys, zlib


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


src, out = sys.argv[1], sys.argv[2]
w, h, img = decode(os.path.join(src, "skin.png"))
_, _, tint = decode(os.path.join(src, "tint.png"))
assert (w, h) == (140, 54), (w, h)
W, H = 2 * w, 2 * h
image = [[(0, 0, 0, 0)] * W for _ in range(H)]
tints = [[0] * W for _ in range(H)]
glow = [[0] * W for _ in range(H)]
for y in range(h):
    for x in range(w):
        p, t = img[y][x], tint[y][x][0]
        if p[3] == 0:
            continue
        bright = sum(p[:3]) > 600 and t == 0
        for dy in (0, 1):
            for dx in (0, 1):
                X, Y = 2 * x + dx, 2 * y + dy
                q = p
                if bright:
                    # The eye: a cyan visor, its inner pixels brightest.
                    q = (150, 250, 255, 255) if (dx, dy) == (0, 1) else (60, 210, 255, 255)
                    glow[Y][X] = 255
                elif t:
                    # Shading at the finer resolution: a light top edge, a dark bottom one.
                    above = y % 18 > 0 and img[y - 1][x][3] > 0
                    below = y % 18 < 17 and img[y + 1][x][3] > 0
                    k = 1.0
                    if dy == 0 and not above:
                        k = 1.25
                    elif dy == 1 and not below:
                        k = 0.7
                    q = tuple(min(255, int(c * k)) for c in p[:3]) + (255,)
                image[Y][X] = q
                tints[Y][X] = 255 if t else 0
# The gun's tip: its outermost grey art pixel in each frame glows at half.
for f in range(21):
    a, r = f % 7, f // 7
    best = None
    for y in range(18):
        for x in range(20):
            p, t = img[r * 18 + y][a * 20 + x], tint[r * 18 + y][a * 20 + x][0]
            if p[3] and not t and sum(p[:3]) <= 600:
                d = abs(x - 9.5) + abs(y - 9)
                if best is None or d > best[0]:
                    best = (d, a * 20 + x, r * 18 + y)
    if best:
        _, x, y = best
        for dy in (0, 1):
            for dx in (0, 1):
                image[2 * y + dy][2 * x + dx] = (255, 200, 120, 255)
                glow[2 * y + dy][2 * x + dx] = 128
os.makedirs(out, exist_ok=True)
encode(os.path.join(out, "skin.png"), W, H, image, False)
encode(os.path.join(out, "tint.png"), W, H, tints, True)
encode(os.path.join(out, "glow.png"), W, H, glow, True)
open(os.path.join(out, "skin.ron"), "w").write(
    "(\n    name: \"Look Test Skin\",\n    art_scale: 4,\n    frame_size: (40, 36),\n    aim_frames: 7,\n"
    "    walk_frames: 3,\n    light: (\n        intensity: 200,\n        reach: 40,\n    ),\n)\n"
)
open(os.path.join(out, "pack.ron"), "w").write(
    "(\n    format_version: 5,\n    id: \"look-test-skin\",\n    name: \"Look Test Skin\",\n    version: \"1\",\n"
    "    kind: Skin,\n    authors: [],\n    origin: None,\n    license: None,\n)\n"
)
