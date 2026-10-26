# Gives OLR's Reloaded levels their surface materials (issue #46): each
# element a default, then a rule that paints its pixels by collision kind
# and art colour, through `olr-cli level material add`, `level element set
# --surface` and `level element surface --rule`. The rule reads each
# element's art (`editor/layers/<id>.png`, art scale 1), sorts every colour
# it holds into a material (`factory` and `castle` below) and hands
# `olr-cli` one exact-colour rule (`<kind>:RRGGBB=<material>`) per colour
# whose material is not the element's default. It clears each map first,
# so a run always makes the same files; the levels' material, front and
# back never change (only their look and surface maps do). The commands
# are in `content/reloaded/README.md`:
#
#   python3 -I tools/levels/reloaded_materials.py target/debug/olr-cli
#
# `--preview DIR` writes each element's materials as a false-colour PNG
# (`<level>.<element>.png`, the colours in `PREVIEW`) instead of editing.

import os, struct, subprocess, sys, zlib

ROOT = "content/reloaded"


def decode(path):
    """An 8-bit, non-interlaced PNG's pixels as RGBA tuples, rows of them."""
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
                row.append((v[0], v[0], v[0], 255 if ct == 0 else v[1]))
            else:
                k = v[0]
                row.append(tuple(plte[3 * k : 3 * k + 3]) + (trns[k] if trns and k < len(trns) else 255,))
        px.append(row)
        prev = line
    return w, h, px


def encode(path, w, h, rows):
    data = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(png)


def hsv(r, g, b):
    """Hue in degrees (0 for a grey), saturation 0 to 1, value 0 to 255."""
    hi, lo = max(r, g, b), min(r, g, b)
    d = hi - lo
    if d == 0:
        return 0.0, 0.0, hi
    if hi == r:
        h = 60.0 * (((g - b) / d) % 6)
    elif hi == g:
        h = 60.0 * ((b - r) / d + 2)
    else:
        h = 60.0 * ((r - g) / d + 4)
    return h, d / hi, hi


def factory(kind, r, g, b):
    """Liero Factory After Dark: a concrete factory on a stone cave. The art
    tells them apart by its greys: the cave's rock is pure grey (r = g =
    b), the building's floors and walls a grey tinted a step warm. Steel
    is grey tinted blue, or white; the machines, the truck and the pipes
    are painted red, orange, teal or blue; wood is brown or olive (desks,
    crates, pallets); earth is the saturated orange-brown of the tunnels."""
    h, s, v = hsv(r, g, b)
    spread = max(r, g, b) - min(r, g, b)
    if kind == "dirt":
        if spread <= 16:
            return "metal"  # the silver crest, steel bits
        if 15 <= h < 40 and 0.75 <= s < 0.97 and v <= 170:
            return "dirt"  # the tunnels' earth, the roof garden's soil (the pure browns are the rack's wood)
        if 38 <= h < 65 and s >= 0.6 and v > 170:
            return "metal"  # the gold heap
        return "wood"  # crates, pallets, plants, the sofa
    if kind == "background":
        if 195 <= h <= 225 and s >= 0.7:
            return None  # the sky (under the sky layer)
        if spread == 0 and v < 0x30:
            return "stone"  # the cave's dark back
        if spread <= 12:
            return "concrete"  # the halls' walls
        if (h < 45 or h >= 330) and s >= 0.5 and v >= 0x60:
            return "metal"  # the red pipe and the truck's box
        return "concrete"
    # Rock.
    if spread <= 12:
        if v >= 0xc8:
            return "metal"  # the truck's cab, white fittings
        if spread == 0:
            return "stone"  # the cave
        return "metal" if b > r else "concrete"
    if 15 <= h < 45 and v < 0xc8 and s >= 0.3:
        return "wood"  # desks, crates
    if 45 <= h < 160 and s < 0.6 or 75 <= h < 160:
        return "wood"  # olive and green crates
    return "metal"  # red, orange, teal and blue machinery


def castle(kind, r, g, b):
    """Castle Strike Reloaded: sandstone walls and grey cave rock (stone),
    red roof tiles (brick), wooden floors, beams, furniture and barrels,
    and grey tankards among the dirt's goods (metal); the sky, the water
    and the hills hold none."""
    h, s, v = hsv(r, g, b)
    spread = max(r, g, b) - min(r, g, b)
    if kind == "background" and 75 <= h < 260 and s >= 0.5:
        return None  # the hills, the sky, the water, and their edges
    if spread <= 6:
        return "metal" if kind == "dirt" else "stone"  # tankards; grey rock
    if (h < 12 or h >= 340) and s >= 0.6:
        return "brick" if kind == "rock" else "wood"  # roof tiles; food
    if 12 <= h < 40 and s >= 0.5:
        return "wood"  # floors, beams, furniture, barrels, the hall's ceiling
    if 35 <= h <= 65 and s < 0.6:
        return "wood" if kind == "dirt" else "stone"  # sacks; sandstone
    if spread <= 16:
        return "metal" if kind == "dirt" else "stone"
    return "wood" if kind == "dirt" else "stone"


# Each level: its rule, the materials it copies in (their order is the
# maps' values) and each element's default (None: none).
LEVELS = {
    "olr-level-liero-factory-after-dark": (
        factory,
        ["stone", "concrete", "metal", "wood", "dirt", "glass"],
        {
            "backdrop": "concrete",
            "dirt": "dirt",
            "rock": "stone",
            "lamps": "glass",
            "glass": "glass",
            "glass-rim": "glass",
            "screens": "glass",
            "ctrl-scr": "glass",
            "rock-scr": "glass",
            "rock-pcs": "glass",
            "pcs-right": "glass",
            "pcs-left": "glass",
            "headlights": "glass",
            "displays": "glass",
            "rock-disp": "glass",
            "beacons": "metal",
            "rock-bcn": "metal",
        },
        # The glowing layers are one material whole: no rule.
        {"backdrop", "dirt", "rock"},
    ),
    "olr-level-castlestrike-reloaded": (
        castle,
        ["stone", "brick", "wood", "metal"],
        {
            # Shows only at the torches, where background-2 is cut away.
            "background": "stone",
            # The sky and the hills: none; the rule paints the rest.
            "background-2": None,
            "dirt": "wood",
            "rock": "stone",
        },
        {"background", "background-2", "dirt", "rock"},
    ),
}

PREVIEW = {
    None: (0, 0, 0),
    "stone": (128, 128, 128),
    "concrete": (200, 184, 150),
    "metal": (64, 192, 255),
    "wood": (138, 90, 42),
    "dirt": (208, 112, 32),
    "glass": (64, 96, 255),
    "brick": (192, 48, 32),
}


def elements(level):
    """Each element's collision kind (`rock`, `dirt`, `background`) and
    whether it is locked, from the composition (canonical: one field a
    line). A distant layer (the skies, issue #63) is left out: it has one
    material whole and no painted map."""
    out, element, locked, distant = {}, None, False, set()
    for line in open(os.path.join(level, "editor", "composition.ron")):
        line = line.strip()
        if line.startswith("id:"):
            element, locked = line.split('"')[1], False
        elif line.startswith("locked:"):
            locked = "true" in line
        elif line.startswith("material:") and element is not None and element not in out:
            out[element] = (line.split(":")[1].strip().rstrip(",").lower(), locked)
        elif line.startswith("distant:") and element is not None:
            distant.add(element)
    return {e: k for e, k in out.items() if e not in distant}


def rules(level, element, kind, rule, default):
    """The `--rule`s of an element: one per colour whose material is not
    its default, sorted; and its pixels' count by material. A painted 0
    is the element's default, so a colour the rule gives none keeps the
    default: only an element without one leaves such pixels none (the
    sky under the factory's `sky` layer and Castle Strike's
    `background-2` never shows)."""
    w, h, px = decode(os.path.join(level, "editor", "layers", element + ".png"))
    colours, counts = {}, {}
    for row in px:
        for c in row:
            if c[3] == 0:
                continue
            if c not in colours:
                colours[c] = rule(kind, *c[:3])
            m = colours[c] if colours[c] is not None or default is None else default
            counts[m] = counts.get(m, 0) + 1
    out = [
        "%s:%02x%02x%02x=%s" % (kind, c[0], c[1], c[2], m)
        for c, m in sorted(colours.items())
        if m is not None and m != default and c[3] == 255
    ]
    return out, counts, (w, h, px, colours)


def main():
    args = sys.argv[1:]
    preview = None
    if "--preview" in args:
        i = args.index("--preview")
        preview = args[i + 1]
        del args[i : i + 2]
        os.makedirs(preview, exist_ok=True)
    if len(args) != 1:
        sys.exit("usage: reloaded_materials.py <olr-cli> [--preview DIR]")
    olr = args[0]
    run = lambda *a: subprocess.run([olr, *a], check=True, stdout=subprocess.DEVNULL)
    for name, (rule, materials, defaults, ruled) in LEVELS.items():
        level = os.path.join(ROOT, name)
        kind = elements(level)
        assert sorted(kind) == sorted(defaults), (name, sorted(kind))
        if preview is None:
            run("level", "material", "add", level, *materials)
        for element, (k, locked) in kind.items():
            default = defaults[element]
            rs = []
            if element in ruled:
                rs, counts, (w, h, px, colours) = rules(level, element, k, rule, default)
                print(name, element, default or "none", len(rs), "rules", dict(sorted(counts.items(), key=str)))
                if preview is not None:
                    pick = lambda c: colours[c] if colours[c] is not None or default is None else default
                    rows = [[PREVIEW[pick(c)] if c[3] else (255, 0, 255) for c in row] for row in px]
                    encode(os.path.join(preview, "%s.%s.png" % (name, element)), w, h, rows)
            if preview is not None:
                continue
            # A locked element (the factory's glowing layers) takes no edit.
            if locked:
                run("level", "element", "set", level, element, "--locked", "false")
            run("level", "element", "set", level, element, "--surface", default or "none")
            run("level", "element", "surface", level, element, "--clear")
            # In batches: a command line stays short.
            for i in range(0, len(rs), 400):
                batch = [a for r in rs[i : i + 400] for a in ("--rule", r)]
                run("level", "element", "surface", level, element, *batch)
            if locked:
                run("level", "element", "set", level, element, "--locked", "true")


if __name__ == "__main__":
    main()
