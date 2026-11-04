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
  The plant works (2026-11-04), loops 1.0 s and seamless unless timed:
  thump.wav   the stamp striking the belt: 0.35 s, a 55 Hz body falling
              to 40 Hz under a short noise crack, about -16 dBFS.
  rumble.wav  the belt running: low noise smoothed twice, the rollers'
              10 Hz knock over it, about -26 dBFS.
  whine.wav   the lift's motor: 300 Hz and 450 Hz with a 2 Hz drift and a
              geared 50 Hz growl, about -26 dBFS.
  cart.wav    the mine cart rolling: a rail joint's clack-clack twice a
              second over a low rumble, about -24 dBFS.
  engine.wav  the forklift's engine: a 30 Hz firing pulse train, its
              harmonics, softly clipped, about -24 dBFS.
  beep.wav    the forklift reversing: 1 kHz for 0.5 s, then the engine
              alone for 0.5 s, about -24 dBFS.
  idle.wav    the generator: a 50 Hz drone and its odd harmonics with a
              slow 1 Hz swell, about -26 dBFS.
  bubble.wav  the lab's flask: eight bubbles a second rising in pitch,
              each a 15 ms chirp, about -30 dBFS.
  klaxon.wav  the alarm: 440 Hz then 330 Hz, half a second each, square
              and softly clipped, about -20 dBFS.
  horn.wav    the shift horn: 1.4 s of 196 Hz and 247 Hz sawtooth swelling
              in and out, about -18 dBFS.
  smash.wav   a window pane shattering: 0.6 s, a crack and a shower of
              high noise bursts thinning out, about -20 dBFS.
  pop.wav     a tube or a lamp going: 0.12 s, a pop and a fizz, about
              -22 dBFS.

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


def smoothed(rng, n, k):
    """`n` samples of noise through `k` one-pole low passes (a = 0.05)."""
    out, ys = [], [0.0] * k
    for _ in range(n):
        x = rng.uniform()
        for j in range(k):
            ys[j] += 0.05 * (x - ys[j])
            x = ys[j]
        out.append(x)
    return out


def looped(samples, fade):
    """Crossfades the last `fade` samples into the first, so a noise loop
    has no seam; `fade` shorter."""
    n = len(samples) - fade
    out = samples[:n]
    for i in range(fade):
        a = i / fade
        out[i] = out[i] * a + samples[n + i] * (1 - a)
    return out


def thump():
    """0.35 s: 55 Hz falling to 40 Hz, decaying in 80 ms, under a 5 ms
    noise crack."""
    rng = Rng(2001)
    out, phase = [], 0.0
    for i in range(round(RATE * 0.35)):
        t = i / RATE
        phase += 2 * math.pi * (40 + 15 * math.exp(-t / 0.05)) / RATE
        body = math.sin(phase) * math.exp(-t / 0.08)
        crack = 0.6 * rng.uniform() * math.exp(-t / 0.005)
        out.append(body + crack)
    return out


def rumble():
    """1.0 s: noise smoothed twice, a 10 Hz roller knock over it."""
    rng = Rng(2002)
    fade = RATE // 10
    noise = looped(smoothed(rng, RATE + fade, 2), fade)
    out = []
    for i, n in enumerate(noise):
        t = i / RATE
        knock = max(0.0, math.sin(2 * math.pi * 10 * t)) ** 8
        out.append(n * 6 + 0.3 * knock * math.sin(2 * math.pi * 80 * t))
    return out


def whine():
    """1.0 s: 300 Hz and 450 Hz drifting at 2 Hz, a geared 50 Hz growl."""
    out = []
    for i in range(RATE):
        t = i / RATE
        drift = 1 + 0.25 * math.sin(2 * math.pi * 2 * t)
        tone = math.sin(2 * math.pi * 300 * t + 0.3 * math.sin(2 * math.pi * 2 * t))
        tone += 0.5 * math.sin(2 * math.pi * 450 * t)
        growl = 0.4 * math.tanh(3 * math.sin(2 * math.pi * 50 * t))
        out.append(tone * drift + growl)
    return out


def cart():
    """1.0 s: two clack pairs (at 0 and 0.5 s, 60 ms apart) over a low
    rumble."""
    rng = Rng(2004)
    fade = RATE // 10
    noise = looped(smoothed(rng, RATE + fade, 2), fade)
    out = []
    for i, n in enumerate(noise):
        t = i / RATE
        clack = 0.0
        for at in (0.0, 0.06, 0.5, 0.56):
            d = t - at
            if 0 <= d < 0.05:
                clack += math.sin(2 * math.pi * 900 * d) * math.exp(-d / 0.008)
        out.append(n * 5 + 0.7 * clack)
    return out


def engine_tone(i):
    """The forklift engine at sample `i`: 30 Hz pulses and harmonics."""
    t = i / RATE
    tone = sum(math.sin(2 * math.pi * 30 * k * t + k) / k for k in range(1, 9))
    return math.tanh(1.5 * tone)


def engine():
    """1.0 s: a 30 Hz firing pulse train, softly clipped."""
    return [engine_tone(i) for i in range(RATE)]


def beep():
    """1.0 s: 1 kHz for 0.5 s (8 ms edges) over the engine, then the
    engine alone."""
    out = []
    for i in range(RATE):
        t = i / RATE
        edge = min(1.0, t / 0.008, max(0.0, 0.5 - t) / 0.008) if t < 0.5 else 0.0
        out.append(0.4 * engine_tone(i) + 1.2 * edge * math.sin(2 * math.pi * 1000 * t))
    return out


def idle():
    """1.0 s: 50 Hz and its odd harmonics, swelling at 1 Hz."""
    out = []
    for i in range(RATE):
        t = i / RATE
        tone = sum(math.sin(2 * math.pi * 50 * k * t) / k for k in (1, 3, 5, 7, 9))
        out.append(tone * (1 + 0.15 * math.sin(2 * math.pi * t)))
    return out


def bubble():
    """1.0 s: eight bubbles, each a 15 ms chirp rising from a pitch of
    its own (the generator's), decaying."""
    rng = Rng(2008)
    out = [0.0] * RATE
    for b in range(8):
        start = round(RATE * (b / 8 + 0.03 * rng.uniform()))
        f0 = 500 + 300 * (rng.uniform() + 1)
        phase = 0.0
        for j in range(round(RATE * 0.015)):
            d = j / RATE
            phase += 2 * math.pi * f0 * (1 + 40 * d) / RATE
            out[(start + j) % RATE] += math.sin(phase) * math.exp(-d / 0.006)
    return out


def klaxon():
    """1.0 s: 440 Hz then 330 Hz (whole cycles in each half), square,
    softly clipped."""
    out = []
    for i in range(RATE):
        t = i / RATE
        f = 440 if t < 0.5 else 330
        out.append(math.tanh(4 * math.sin(2 * math.pi * f * t)))
    return out


def horn():
    """1.4 s: 196 Hz and 247 Hz sawtooths, in over 0.15 s and out over
    0.4 s."""
    n = round(RATE * 1.4)
    out = []
    for i in range(n):
        t = i / RATE
        env = min(1.0, t / 0.15, (1.4 - t) / 0.4)
        saw = sum(math.sin(2 * math.pi * f * k * t) / k for f in (196, 247) for k in range(1, 7))
        out.append(env * math.tanh(0.8 * saw))
    return out


def smash():
    """0.6 s: a 10 ms crack, then high noise bursts (shards) thinning."""
    rng = Rng(2011)
    n = round(RATE * 0.6)
    out, last = [], 0.0
    for i in range(n):
        t = i / RATE
        x = rng.uniform()
        hp = x - last
        last = x
        crack = math.exp(-t / 0.01)
        shards = 0.5 * math.exp(-t / 0.2) * (1 if (rng.next() % 1000) < 120 * math.exp(-t / 0.25) + 5 else 0.15)
        out.append(hp * (crack + shards))
    return out


def pop():
    """0.12 s: a 2 ms pop, a fizz decaying in 40 ms."""
    rng = Rng(2012)
    out = []
    for i in range(round(RATE * 0.12)):
        t = i / RATE
        out.append(math.exp(-t / 0.002) * math.sin(2 * math.pi * 200 * t) + 0.5 * rng.uniform() * math.exp(-t / 0.04))
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    write("hum.wav", hum())
    write("buzz.wav", buzz(), 10 ** (-28 / 20))
    write("tick.wav", tick(), 10 ** (-24 / 20))
    db = lambda d: 10 ** (d / 20)
    write("thump.wav", thump(), db(-16))
    write("rumble.wav", rumble(), db(-26))
    write("whine.wav", whine(), db(-26))
    write("cart.wav", cart(), db(-24))
    write("engine.wav", engine(), db(-24))
    write("beep.wav", beep(), db(-24))
    write("idle.wav", idle(), db(-26))
    write("bubble.wav", bubble(), db(-30))
    write("klaxon.wav", klaxon(), db(-20))
    write("horn.wav", horn(), db(-18))
    write("smash.wav", smash(), db(-20))
    write("pop.wav", pop(), db(-22))


main()
