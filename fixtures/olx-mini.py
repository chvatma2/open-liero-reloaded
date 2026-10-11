#!/usr/bin/env python3
"""Writes fixtures/olx-mini/: a tiny OpenLieroX game directory for the import
tests (rl-content, rl-ui, rl-game), so they need no OLX download.

- `Tiny Mod/`: a gamescript v7 mod, one weapon of pixel projectiles; its sound
  `boom.wav` is missing (an import warning, as OLX drops it).
- `Broken/`: a `script.lgs` that is not one (an import error).
- `levels/Tiny Level.lxl`: a 64 x 48 image level (dirt, rock, a cave);
  `levels/Bad.lxl` is not a level (an error); `levels/old.lev` is a Liero
  level (listed, not imported).
- `data/themes/Dirt/`: a theme of 4 x 4 images.
- `skins/default.png`: a 672 x 36 skin sheet.
- `data/frontend/colours.cfg`: one of the core pack's files only (no core pack).

The byte layouts follow rl-import-lgs's and rl-import-lxl's test builders.
Run from the repository root: python3 fixtures/olx-mini.py
"""
import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "olx-mini")


def put(path, data):
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "wb") as f:
        f.write(data)


def png(w, h, rgba):
    raw = b"".join(b"\x00" + bytes(rgba) * w for _ in range(h))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


class Lgs:
    def __init__(self):
        self.b = bytearray()

    def i(self, v):
        self.b += struct.pack("<i", v)
        return self

    def f(self, v):
        self.b += struct.pack("<f", v)
        return self

    def s(self, v):
        self.b += bytes([len(v)]) + v.encode()
        return self

    @staticmethod
    def header(name, weapons):
        g = Lgs()
        g.b += b"Liero Game Script".ljust(20, b"\x00")
        g.b += struct.pack("<I", 7)
        g.b += (name.encode() + b"\x00").ljust(64, b"\xaa")
        return g.i(weapons)

    def projectile_weapon(self, name, sound):
        self.s(name).i(0).i(4).i(5).f(5.0).f(30.0).f(250.0)
        self.i(100).f(0.1).f(2.0).i(1).i(0)
        return self.i(1).s(sound) if sound else self.i(0)

    def pixel_projectile(self, damage, child):
        self.i(0).f(0.0).f(0.0).i(0).i(0).f(1.0)
        self.i(1).i(255).i(0).i(300)
        self.i(1).i(damage).i(1 if child else 0).i(0).i(0)
        self.i(2).i(7).i(0)
        for _ in range(8):
            self.i(0)
        if child:
            self.i(1).i(90).i(3).f(0.5).i(40).f(0.25)
            child(self)
        return self

    def footer(self):
        return self.i(300).i(27).f(4.5).f(150.0).f(8.0).f(4.0).f(100.0).f(-65.0).f(0.0).f(0.4)


def lxl(name, w, h):
    def fixed(s, n):
        b = bytearray(b"\xcc" * n)
        b[: len(s)] = s.encode()
        b[len(s)] = 0
        return bytes(b)

    back, front, flags = bytearray(), bytearray(), bytearray()
    for y in range(h):
        for x in range(w):
            back += bytes([40, 20, 10])
            if x < 4 or x >= w - 4 or y >= h - 4:
                front += bytes([110, 110, 110])
                flags.append(0x04)  # rock
            elif 16 <= x < 48 and 12 <= y < 36:
                front += bytes([0, 0, 0])
                flags.append(0x01)  # empty
            else:
                front += bytes([150, 90, 40])
                flags.append(0x02)  # dirt
    raw = bytes(back + front + flags) + b"\x00"
    z = zlib.compress(raw, 9)
    out = fixed("LieroX Level", 32) + struct.pack("<i", 0) + fixed(name, 64)
    out += struct.pack("<iii", w, h, 1) + fixed("dirt", 32) + struct.pack("<i", 0)
    return out + struct.pack("<II", len(z), len(raw)) + z


def main():
    mod = Lgs.header("Tiny Mod", 1).projectile_weapon("Pellet", "boom.wav")
    mod.pixel_projectile(12, lambda b: b.pixel_projectile(3, None)).footer()
    put("Tiny Mod/script.lgs", bytes(mod.b))
    put("Broken/script.lgs", b"not a Liero Game Script")
    put("levels/Tiny Level.lxl", lxl("Tiny Level", 64, 48))
    put("levels/Bad.lxl", b"not a level")
    put("levels/old.lev", b"a Liero level")
    theme = "data/themes/Dirt"
    put(f"{theme}/theme.txt", b"[General]\nNumStones = 0\nNumMisc = 0\n")
    for f in ["Hole1", "Hole2", "Hole3", "Hole4", "Hole5", "FrontTile", "BackTile"]:
        put(f"{theme}/{f}.png", png(4, 4, (0, 0, 0, 255)))
    put("skins/default.png", png(672, 36, (200, 200, 200, 255)))
    put("data/frontend/colours.cfg", b"[Colours]\nRopeColour1 = #A05000\n")


if __name__ == "__main__":
    main()
