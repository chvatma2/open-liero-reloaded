# Puts the skies of OLR's Reloaded levels on glowing distant (parallax)
# layers (issue #63), through `olr-cli level element`:
#
#   python3 -I tools/levels/reloaded_skies.py target/debug/olr-cli
#
# For each level: the sky region (`REGION`: Castle Strike's sky glow mask,
# Liero Factory's `sky` layer) is erased from every back element covering
# it (`erase --mask`, the region as a PNG), the old sky element is removed
# when it holds nothing else, and a level-sized image takes its place at
# (0, 0), distant and glowing as the old sky did. With the camera at the
# level's centre (a distant layer's reference placement) the image shows
# the old sky's art at the old sky's pixels, byte for byte, so the baked
# glow and the light stay as they were. Elsewhere (behind the hills, the
# castle and the factory, seen once the camera moves) the sky's own
# gradient goes on: both are radial, so each pixel takes the colours the
# old sky has at its distance from the gradient's centre (fitted to the
# sky), never stretched. Liero Factory's gets stars (`stars`): a fixed
# seed, so a run always makes the same images. Only the layer's middle is
# ever seen in play (`window`); the rest gives the light at the reference
# placement. The generated images are
# written to `target/reloaded-skies/` first. A level already holding a
# distant layer is left alone: running it again changes nothing.

import math, os, subprocess, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reloaded_materials import decode, encode  # noqa: E402

ROOT = "content/reloaded"
OUT = "target/reloaded-skies"

# name: the sky region (a glow mask or a layer), the element holding the
# sky's art, the back elements covering the region, whether the art's
# element goes (it holds only the sky), the layer's depth and glow
# (strength percent, group), whether it is locked, the fill (`bands`: the
# majority colour at each distance, crisp rings; `noise`: a colour drawn
# from those at each distance, a dithered gradient) and the stars' seed.
LEVELS = {
    "olr-level-castlestrike-reloaded": dict(
        region=("glow", "background-2"),
        art="background-2",
        cut=["background", "background-2"],
        remove=False,
        depth=85,
        glow=(100, "sky"),
        locked=False,
        fill="bands",
        stars=None,
    ),
    "olr-level-liero-factory-after-dark": dict(
        region=("layers", "sky"),
        art="sky",
        cut=["backdrop"],
        remove=True,
        depth=90,
        glow=(35, "sky"),
        locked=True,
        fill="noise",
        stars=63,
    ),
}

# The stars' colours, faint to bright (white to pale blue), and how often
# each is drawn, of 100.
STAR_COLOURS = [((0x93, 0xAC, 0xDF), 45), ((0xBC, 0xCF, 0xF5), 30), ((0xE4, 0xEC, 0xFF), 18), ((0xFF, 0xFF, 0xFF), 7)]
# A star's chance per art pixel at the top and at the bottom of the part
# of the layer the game ever shows (`window`; linear between: denser
# towards the top, the top's above and the bottom's below), and a bright
# star's chance of being 2 x 2.
STAR_TOP, STAR_BOTTOM, STAR_BIG = 1 / 140, 1 / 420, 1 / 20
# The game's view, world pixels (`Viewport::GAME`).
VIEW = (427, 192)


class Rng:
    """SplitMix64: the same numbers on every Python."""

    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return z ^ (z >> 31)

    def unit(self):
        """A float in [0, 1)."""
        return (self.next() >> 11) / float(1 << 53)

    def below(self, n):
        return self.next() % n


def region(level, spec):
    """The sky's pixels: a glow mask's non-zero ones, or a layer's covered
    ones."""
    kind, element = spec
    _, _, px = decode(os.path.join(level, "editor", kind, element + ".png"))
    if kind == "glow":
        return {(x, y) for y, row in enumerate(px) for x, c in enumerate(row) if c[0]}
    return {(x, y) for y, row in enumerate(px) for x, c in enumerate(row) if c[3]}


def value(c):
    return c[1] + c[2]


def spread(sky, cx, cy):
    """How much the sky's brightness varies at one distance from (cx, cy):
    the sum of the variances in half-pixel rings, per pixel."""
    rings = {}
    for (x, y), c in sky:
        r = int(2 * math.hypot(x - cx, y - cy))
        n = rings.setdefault(r, [0, 0, 0])
        v = value(c)
        n[0] += 1
        n[1] += v
        n[2] += v * v
    return sum(s2 - s * s / n for n, s, s2 in rings.values()) / len(sky)


def centre(sky, w, h):
    """The gradient's centre: the point whose rings vary least, searched
    in steps of 16, 4 and 1 pixels over every seventh sky pixel."""
    sample = sky[::7]
    best = min(((spread(sample, x, y), x, y) for x in range(0, w, 16) for y in range(0, h + h // 2, 16)))
    for step in (4, 1):
        _, bx, by = best
        near = [(x, y) for x in range(bx - 4 * step, bx + 4 * step + 1, step) for y in range(by - 4 * step, by + 4 * step + 1, step)]
        best = min(best, min((spread(sample, x, y), x, y) for x, y in near))
    return best[1], best[2]


def rings(sky, cx, cy, fill):
    """Each half-pixel ring's colours: `bands` its commonest (ties by the
    colour), `noise` those within 4 of its median brightness (a stray
    colour left out), sorted."""
    by = {}
    for (x, y), c in sky:
        by.setdefault(int(2 * math.hypot(x - cx, y - cy)), []).append(c)
    out = {}
    for r, cs in by.items():
        if fill == "bands":
            count = {}
            for c in cs:
                count[c] = count.get(c, 0) + 1
            out[r] = [max(sorted(count), key=lambda c: count[c])]
        else:
            vs = sorted(value(c) for c in cs)
            mid = vs[len(vs) // 2]
            out[r] = sorted(c for c in cs if abs(value(c) - mid) <= 4)
    return out


def sky_image(level, cfg):
    """The distant layer's image (rows of RGB), its region and the
    gradient's centre."""
    w, h, art = decode(os.path.join(level, "editor", "layers", cfg["art"] + ".png"))
    keep = region(level, cfg["region"])
    sky = sorted(((x, y), art[y][x][:3]) for x, y in keep)
    cx, cy = centre(sky, w, h)
    colours = rings(sky, cx, cy, cfg["fill"])
    known = sorted(colours)
    rng = Rng(cfg["stars"] or 0)
    nearest = {}
    rows = []
    for y in range(h):
        row = []
        for x in range(w):
            if (x, y) in keep:
                row.append(art[y][x][:3])
                continue
            r = int(2 * math.hypot(x - cx, y - cy))
            if r not in colours:
                if r not in nearest:
                    nearest[r] = min(known, key=lambda k: (abs(k - r), k))
                r = nearest[r]
            cs = colours[r]
            row.append(cs[rng.below(len(cs))] if len(cs) > 1 else cs[0])
        rows.append(row)
    return w, h, rows, keep, (cx, cy)


def window(w, h, depth):
    """The part of a level-sized layer at (0, 0) the game ever shows: the
    camera keeps the view inside the level, and the layer moves by `depth`
    percent of the camera's movement from the level's centre, so it shows
    `view + (1 - depth)(size - view)` around its centre (x0, y0, x1, y1)."""
    out = []
    for size, view in ((w, VIEW[0]), (h, VIEW[1])):
        lo = depth * (size - view) // 200
        out.append((lo, size - lo))
    return out[0][0], out[1][0], out[0][1], out[1][1]


def stars(rows, w, h, seed, area):
    """Scatters stars over the image: single pixels, a few 2 x 2, never
    touching another; returns how many fell inside `area`."""
    rng = Rng(seed * 0x2545F4914F6CDD1D + 1)
    x0, y0, x1, y1 = area
    taken = set()
    inside = 0
    for y in range(h):
        t = min(max((y - y0) / (y1 - 1 - y0), 0.0), 1.0)
        chance = STAR_TOP + (STAR_BOTTOM - STAR_TOP) * t
        for x in range(w):
            if rng.unit() >= chance:
                continue
            pick = rng.below(100)
            for colour, weight in STAR_COLOURS:
                if pick < weight:
                    break
                pick -= weight
            big = colour in (STAR_COLOURS[2][0], STAR_COLOURS[3][0]) and rng.unit() < STAR_BIG
            side = 2 if big else 1
            cells = [(x + i, y + j) for j in range(side) for i in range(side)]
            if any(px >= w or py >= h for px, py in cells):
                continue
            if any((px + i, py + j) in taken for px, py in cells for j in (-1, 0, 1) for i in (-1, 0, 1)):
                continue
            for px, py in cells:
                rows[py][px] = colour
                taken.add((px, py))
            inside += x0 <= x < x1 and y0 <= y < y1
    return inside


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: reloaded_skies.py <olr-cli>")
    olr = sys.argv[1]
    run = lambda *a: subprocess.run([olr, *a], check=True, stdout=subprocess.PIPE, text=True).stdout
    for name, cfg in LEVELS.items():
        level = os.path.join(ROOT, name)
        if "distant:" in open(os.path.join(level, "editor", "composition.ron")).read():
            print(name, "already has a distant layer: left alone")
            continue
        w, h, rows, keep, (cx, cy) = sky_image(level, cfg)
        line = "%s: %d sky pixels, the gradient's centre (%d, %d)" % (name, len(keep), cx, cy)
        x0, y0, x1, y1 = window(w, h, cfg["depth"])
        line += ", shown (%d, %d) to (%d, %d)" % (x0, y0, x1, y1)
        if cfg["stars"] is not None:
            n = stars(rows, w, h, cfg["stars"], (x0, y0, x1, y1))
            line += ", %d stars there (1 per %d pixels)" % (n, (x1 - x0) * (y1 - y0) // max(n, 1))
        print(line)
        out = os.path.join(OUT, name)
        os.makedirs(out, exist_ok=True)
        image = os.path.join(out, "sky.png")
        cut = os.path.join(out, "cut.png")
        encode(image, w, h, rows)
        white, black = (255, 255, 255), (0, 0, 0)
        encode(cut, w, h, [[white if (x, y) in keep else black for x in range(w)] for y in range(h)])
        for element in cfg["cut"]:
            run("level", "element", "erase", level, element, "--mask", cut)
        if cfg["remove"]:
            run("level", "element", "set", level, cfg["art"], "--locked", "false")
            run("level", "element", "remove", level, cfg["art"])
        size = "%dx%d" % (w, h)
        added = run("level", "element", "add-image", level, image, "--at", "0,0", "--size", size, "--material", "background", "--name", "Sky")
        sky = added.split()[-1]
        strength, group = cfg["glow"]
        flags = ["--distant", str(cfg["depth"]), "--glow", str(strength), "--glow-group", group]
        if cfg["locked"]:
            flags += ["--locked", "true"]
        run("level", "element", "set", level, sky, *flags)


if __name__ == "__main__":
    main()
