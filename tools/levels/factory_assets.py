# Builds Liero Factory Reloaded from assets (issue #96): Liero Factory
# After Dark's art as a ground, a night sky and copies of built-in assets
# at the pieces' old places, through `olr-cli` commands only:
#
#   python3 -I tools/levels/factory_assets.py target/debug/olr-cli
#
# 1. The new built-in assets (`NEW_ASSETS`: the Crane, the Console, the
#    Gauge) are made by their blocks in `content/assets/README.md`, each run
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
# Every copy is static. The bake is After Dark's (`olr-cli`'s
# `tests/reloaded.rs`). Scratch files go to `target/factory-assets/`.
# Running it again makes the same bytes.

import os, shlex, shutil, subprocess, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from reloaded_materials import decode  # noqa: E402

AFTER_DARK = "content/reloaded/olr-level-liero-factory-after-dark"
OUT = "content/reloaded/olr-level-liero-factory-reloaded"
ID = "olr-level-liero-factory-reloaded"
NAME = "Liero Factory Reloaded"
NOTES = (
    "Liero Factory After Dark rebuilt from assets (issue #96): the ground is the level's own "
    "asset, the night sky a copy of its own at depth 90, and the strip lights, the ceiling lamp, "
    "the control room's screen and desk, the lit sign, a roof beacon, the crane, the console and "
    "the gauge are copies of the built-in assets where After Dark has them. It plays and looks "
    "as After Dark; issues #97, #98 and #99 bring it alive."
)
ASSETS = "content/assets"
README = os.path.join(ASSETS, "README.md")
SCRATCH = "target/factory-assets"
NEW_ASSETS = ["crane", "console", "gauge"]

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
