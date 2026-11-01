#!/usr/bin/env python3
"""Writes the live fixture's two sounds (issue #92) into the directory
given: synthesised, 16-bit mono WAV at 22050 Hz, the same bytes every run.

  hum.wav   a press's motor hum to loop: 105 Hz with its 2nd and 3rd
            harmonics, 50 whole periods (about 0.48 s), so the loop has
            no seam;
  tick.wav  a lamp's tick: 25 ms of a 2.4 kHz tone dying away fast.

make-live-test.sh runs it; nothing else reads it.
"""
import math
import struct
import sys

RATE = 22050


def wav(path, samples):
    data = b"".join(struct.pack("<h", max(-32767, min(32767, round(s * 32767)))) for s in samples)
    fmt = struct.pack("<HHIIHH", 1, 1, RATE, RATE * 2, 2, 16)
    body = b"WAVE" + b"fmt " + struct.pack("<I", len(fmt)) + fmt + b"data" + struct.pack("<I", len(data)) + data
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", len(body)) + body)


def hum():
    period = 210  # 22050 / 105
    out = []
    for i in range(period * 50):
        t = 2 * math.pi * (i % period) / period
        out.append(0.30 * math.sin(t) + 0.12 * math.sin(2 * t) + 0.06 * math.sin(3 * t))
    return out


def tick():
    n = RATE * 25 // 1000
    return [0.6 * math.exp(-i / (RATE * 0.004)) * math.sin(2 * math.pi * 2400 * i / RATE) for i in range(n)]


def main():
    out = sys.argv[1]
    wav(f"{out}/hum.wav", hum())
    wav(f"{out}/tick.wav", tick())


if __name__ == "__main__":
    main()
