"""Writes Liero Factory Reloaded's live sounds (issue #97) into
`tools/levels/factory/sounds/`: synthesised for OLR, CC0, the same bytes
on every run (stdlib only: `wave`, `math` and a fixed-seed generator).

    python3 -I tools/sounds/factory_sounds.py

  hum.wav   the crane's motor hum, looped while it moves: a 1.0 s
            seamless loop (every component a whole number of cycles a
            second) of 100 Hz with its 2nd to 5th harmonics, wobbling at
            4 Hz, with a breath of noise, at about -18 dBFS (RMS).
  buzz.wav  issue #98: a faulty fluorescent tube's mains buzz, looped while
            the Strip light is On or Dim: a 1.0 s seamless loop of 100 Hz
            and its 2nd to 12th harmonics, softly clipped (rough), with a
            breath of noise, at about -28 dBFS (RMS), well under the
            weapons.
  tick.wav  issue #98: the tube's starter ticking as it comes On: 40 ms, a
            noise burst decaying in about 4 ms, at about -24 dBFS (RMS).

All are mono, 16-bit, 22 050 Hz.
"""

import math
import os
import wave

RATE = 22050
OUT = "tools/levels/factory/sounds"
# -18 dBFS, the samples' root mean square.
LEVEL = 10 ** (-18 / 20)


class Rng:
    """SplitMix64, seeded: the same numbers on every run and machine."""

    def __init__(self, seed):
        self.state = seed & (2**64 - 1)

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & (2**64 - 1)
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & (2**64 - 1)
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & (2**64 - 1)
        return z ^ (z >> 31)

    def uniform(self):
        """In [-1, 1)."""
        return (self.next() >> 11) / float(1 << 52) - 1.0


def write(name, samples, level=LEVEL):
    """`samples` scaled to `level` (RMS, of full scale) into `name`."""
    rms = math.sqrt(sum(s * s for s in samples) / len(samples))
    gain = level / rms
    frames = bytearray()
    for s in samples:
        v = max(-32767, min(32767, round(s * gain * 32767)))
        frames += v.to_bytes(2, "little", signed=True)
    path = os.path.join(OUT, name)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(frames))
    print(f"wrote {path} ({len(samples)} samples, {len(samples) / RATE:.2f} s)")


def hum():
    """1.0 s: 100 Hz and its harmonics, a 4 Hz wobble, a little noise."""
    rng = Rng(97)
    harmonics = [(1, 1.0), (2, 0.55), (3, 0.30), (4, 0.16), (5, 0.08)]
    out = []
    for i in range(RATE):
        t = i / RATE
        tone = sum(a * math.sin(2 * math.pi * 100 * k * t) for k, a in harmonics)
        wobble = 1.0 + 0.18 * math.sin(2 * math.pi * 4 * t)
        out.append(tone * wobble + 0.02 * rng.uniform())
    return out


def buzz():
    """1.0 s: 100 Hz and its 2nd to 12th harmonics (every one a whole
    number of cycles in the loop), softly clipped, a little noise."""
    rng = Rng(98)
    out = []
    for i in range(RATE):
        t = i / RATE
        tone = sum(math.sin(2 * math.pi * 100 * k * t + k * 0.7) / k ** 0.8 for k in range(1, 13))
        out.append(math.tanh(1.6 * tone) + 0.04 * rng.uniform())
    return out


def tick():
    """40 ms: a noise burst decaying in about 4 ms, its low end taken off
    (each sample less half the one before), so it clicks."""
    rng = Rng(981)
    out, last = [], 0.0
    for i in range(round(RATE * 0.040)):
        n = rng.uniform() * math.exp(-i / (RATE * 0.004))
        out.append(n - 0.5 * last)
        last = n
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    write("hum.wav", hum())
    write("buzz.wav", buzz(), 10 ** (-28 / 20))
    write("tick.wav", tick(), 10 ** (-24 / 20))


main()
