# Liero Factory Reloaded's night skyline (the night shift look): two
# level-sized pictures for parallax layers in front of the night sky, each
# with its glow mask, drawn from fixed seeds so every run writes the same
# bytes. `factory_assets.py` imports `write`; by hand:
#
#   python3 -I tools/levels/factory_skyline.py <out-dir>
#
# - `city.png`, `city-glow.png`: the far city (depth 75): towers with
#   setbacks, masts and water towers, lit floors in runs, darker up and
#   hazier at the foot; the lit windows and the masts' red tips glow.
# - `industry.png`, `industry-glow.png`: the near industry (depth 45):
#   sawtooth sheds, chimneys, a cooling tower, lattice pylons with sagging
#   power lines; only the red light on every top glows.
#
# A picture covers the layer's whole level-sized box; alpha 0 is sky.

import os, random, struct, sys, zlib

W, H = 1200, 480
RED = (255, 50, 36)


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


class Layer:
    def __init__(self):
        self.art = [[(0, 0, 0, 0)] * W for _ in range(H)]
        self.glow = [[0] * W for _ in range(H)]

    def put(self, x, y, c, glow=0):
        if 0 <= x < W and 0 <= y < H:
            self.art[y][x] = (*c, 255)
            if glow:
                self.glow[y][x] = glow

    def solid(self, x, y):
        return 0 <= x < W and 0 <= y < H and self.art[y][x][3] != 0

    def rect(self, x0, y0, x1, y1, c):
        for y in range(y0, y1):
            for x in range(x0, x1):
                self.put(x, y, c)


def mix(a, b, t):
    return tuple(int(a[k] + (b[k] - a[k]) * t) for k in range(3))


def far_city(seed):
    r = random.Random(seed)
    lay = Layer()
    top_c, foot_c = (22, 40, 88), (40, 64, 124)
    base = 400

    def body(y):
        return mix(top_c, foot_c, max(0.0, min(1.0, (y - 250) / (base - 250))))

    warm = ((255, 206, 128), (255, 226, 170), (250, 190, 110))
    cool = ((200, 220, 255), (170, 200, 250))
    x = -4
    while x < W:
        kind = r.random()
        w = r.randint(10, 18) if kind < 0.35 else r.randint(16, 34)
        h = r.randint(70, 150) if kind < 0.35 else r.randint(34, 90)
        top = base - h
        for xx in range(x, x + w):
            for y in range(top, H):
                lay.put(xx, y, body(y))
        crown = r.random()
        mid = x + w // 2
        if kind < 0.35 and crown < 0.5:  # a setback and a mast
            sw = max(4, w // 2)
            sh = r.randint(8, 18)
            for xx in range(mid - sw // 2, mid - sw // 2 + sw):
                for y in range(top - sh, top):
                    lay.put(xx, y, body(y))
            mh = r.randint(8, 20)
            for y in range(top - sh - mh, top - sh):
                lay.put(mid, y, body(y))
            lay.put(mid, top - sh - mh - 1, (255, 60, 40), 255)
        elif crown < 0.75:  # a parapet and roof boxes
            for xx in range(x, x + w):
                lay.put(xx, top - 1, body(top))
            for _ in range(r.randint(1, 2)):
                bx = r.randint(x + 1, x + w - 5)
                lay.rect(bx, top - r.randint(2, 4), bx + r.randint(2, 4), top - 1, body(top))
        elif crown < 0.9 and w >= 14:  # a water tower on legs
            tx = r.randint(x + 2, x + w - 8)
            for y in range(top - 4, top):
                lay.put(tx + 1, y, body(top))
                lay.put(tx + 5, y, body(top))
            lay.rect(tx, top - 10, tx + 7, top - 4, body(top))
            for xx in range(tx + 1, tx + 6):
                lay.put(xx, top - 11, body(top))
            lay.put(tx + 3, top - 12, body(top))
        # Windows: floors every 3 px, columns every 2 or 3, lit in runs.
        pal = r.choice((warm, warm, cool))
        lit_floors = r.uniform(0.15, 0.55)
        step = r.choice((2, 3))
        for wy in range(top + 3, base - 2, 3):
            if r.random() > lit_floors:
                continue
            run = r.random() < 0.4  # a whole floor lit, an office working late
            for wx in range(x + 2, x + w - 2, step):
                if run or r.random() < 0.35:
                    lay.put(wx, wy, r.choice(pal), 255)
        x += w + r.choice((0, 0, 1, 2, 3))
    return lay


def near_industry(seed):
    r = random.Random(seed)
    lay = Layer()
    c = (12, 24, 54)
    base = 380
    x = 0
    while x < W:  # sawtooth sheds along the bottom
        w = r.randint(40, 90)
        h = r.randint(14, 24)
        for xx in range(x, min(W, x + w)):
            tooth = (xx - x) % 10
            for y in range(base - h - (tooth if tooth < 7 else 0) // 2, H):
                lay.put(xx, y, c)
        x += w + r.randint(0, 4)
    tops = []
    for cx in r.sample(range(30, W - 30, 12), 7):  # chimneys
        h, w = r.randint(70, 130), r.randint(4, 7)
        lay.rect(cx, base - h, cx + w, base, c)
        lay.rect(cx - 1, base - h, cx + w + 1, base - h + 2, c)
        for band in range(base - h + 8, base - 10, 14):
            for xx in range(cx, cx + w):
                lay.put(xx, band, (18, 34, 70))
        tops.append((cx + w // 2, base - h - 2))
    tx, th = r.randint(500, 700), 70  # a cooling tower
    for k in range(th):
        t = k / th
        half = int(26 - 10 * (1 - (2 * t - 1) ** 2) + 4 * t)
        for xx in range(tx - half, tx + half + 1):
            lay.put(xx, base - th + k, c)
    pylons = []
    px = 60
    while px < W:  # lattice pylons
        h = r.randint(80, 100)
        for k in range(h):
            half = 1 + (h - k) * 6 // h
            lay.put(px - half, base - k, c)
            lay.put(px + half, base - k, c)
            if k % 5 == 0:
                for xx in range(px - half, px + half + 1):
                    lay.put(xx, base - k, c)
            if k % 10 == 5:
                lay.put(px, base - k, c)
        for arm in (base - h + 8, base - h + 18):
            for xx in range(px - 9, px + 10):
                lay.put(xx, arm, c)
        pylons.append((px, base - h))
        tops.append((px, base - h - 2))
        px += r.randint(150, 230)
    for (ax, ay), (bx, by) in zip(pylons, pylons[1:]):  # their wires
        for wire in (8, 18):
            for side in (-9, 9):
                x0, x1 = ax + side, bx + side
                for xx in range(x0, x1 + 1):
                    t = (xx - x0) / max(1, x1 - x0)
                    y = int(ay + wire + (by - ay) * t + 10 * 4 * t * (1 - t))
                    if not lay.solid(xx, y):
                        lay.put(xx, y, (20, 36, 76))
    for x, y in tops:
        for dx in (0, 1):
            for dy in (0, 1):
                lay.put(x + dx, y + dy, RED, 255)
    return lay


def write(out):
    """The two pictures and their masks in `out`; returns their paths."""
    os.makedirs(out, exist_ok=True)
    paths = {}
    for name, lay in (("city", far_city(23)), ("industry", near_industry(9))):
        art, glow = os.path.join(out, name + ".png"), os.path.join(out, name + "-glow.png")
        png(art, W, H, lay.art, True)
        png(glow, W, H, lay.glow, False)
        paths[name] = (art, glow)
    return paths


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: python3 -I tools/levels/factory_skyline.py <out-dir>")
    write(sys.argv[1])
