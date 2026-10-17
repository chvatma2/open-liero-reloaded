# A 16 x 16 RGBA PNG: a disc of radius 7 with a colour gradient, transparent
# outside it and a half-transparent ring at its edge.
import struct, sys, zlib
w = h = 16
rows = b""
for y in range(h):
    row = b"\x00"
    for x in range(w):
        d2 = (x - 7.5) ** 2 + (y - 7.5) ** 2
        a = 255 if d2 <= 36 else 100 if d2 <= 56 else 0
        row += bytes([x * 16, y * 16, 255 - x * 8, a])
    rows += row
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
