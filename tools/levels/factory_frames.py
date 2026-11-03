# Frames of Liero Factory Reloaded for a look check (the night shift,
# 2026-11-03): idle bots, seed 1, tick 350, the camera fixed on each of
# `VIEWS`, written as `<out>/<x>-<y>.png`:
#
#   python3 -I tools/levels/factory_frames.py target/release/olr-cli <out-dir>

import os, subprocess, sys

VIEWS = [(213, 430), (640, 430), (380, 330), (900, 320), (900, 175), (1000, 96), (213, 200), (213, 330)]


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: python3 -I tools/levels/factory_frames.py <olr-cli> <out-dir>")
    cli, out = os.path.abspath(sys.argv[1]), sys.argv[2]
    os.makedirs(out, exist_ok=True)
    for x, y in VIEWS:
        path = os.path.join(out, f"{x}-{y}.png")
        subprocess.run([cli, "render-match", "--mod", "olr-mod-liero-v1-reloaded", "--level",
                        "olr-level-liero-factory-reloaded", "--bots", "2", "--personality", "idle", "--seed", "1",
                        "--ticks", "350", "--at", f"{x},{y}", "--scale", "2", "--out", path], check=True)
        print(f"wrote {path}")


if __name__ == "__main__":
    main()
