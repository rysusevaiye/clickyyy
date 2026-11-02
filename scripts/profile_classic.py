#!/usr/bin/env python3
"""Classic profile: bright, lively clicky-switch samples for Clicky (stdlib only).

Each sound is modelled as one or more physical impacts: a short burst of
filtered noise (the plastic-on-plastic strike) plus a handful of exponentially
decaying resonant modes (the keycap, plate and case ringing). Several variants
are rendered per category so that consecutive keystrokes never sound identical.
"""
import math
import os
import random
import struct
import wave

SR = 48_000
VARIANTS = 5
OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds", "classic")


def biquad(x, kind, freq, q):
    w = 2 * math.pi * freq / SR
    cos_w, alpha = math.cos(w), math.sin(w) / (2 * q)
    if kind == "lp":
        b0, b1, b2 = (1 - cos_w) / 2, 1 - cos_w, (1 - cos_w) / 2
    elif kind == "hp":
        b0, b1, b2 = (1 + cos_w) / 2, -(1 + cos_w), (1 + cos_w) / 2
    else:  # band-pass, constant peak gain
        b0, b1, b2 = alpha, 0.0, -alpha
    a0, a1, a2 = 1 + alpha, -2 * cos_w, 1 - alpha
    b0, b1, b2, a1, a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0
    y = [0.0] * len(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, v in enumerate(x):
        o = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, o
        y[i] = o
    return y


def impact(rng, length, noise, modes):
    """noise = (filter kind, freq, q, decay seconds, gain); modes = [(freq, decay seconds, amp)]."""
    n = int(length * SR)
    kind, freq, q, tau, gain = noise
    burst = [rng.uniform(-1, 1) * math.exp(-i / (tau * SR)) for i in range(n)]
    out = [v * gain for v in biquad(burst, kind, freq, q)]
    for f, decay, amp in modes:
        phase, w, k = rng.uniform(0, 2 * math.pi), 2 * math.pi * f / SR, 1 / (decay * SR)
        for i in range(n):
            out[i] += amp * math.exp(-i * k) * math.sin(w * i + phase)
    return out


def jitter(rng, value, pct):
    return value * rng.uniform(1 - pct, 1 + pct)


def vary_noise(rng, noise):
    kind, freq, q, tau, gain = noise
    return kind, jitter(rng, freq, 0.08), q, jitter(rng, tau, 0.15), jitter(rng, gain, 0.1)


def vary_modes(rng, modes):
    return [(jitter(rng, f, 0.05), jitter(rng, d, 0.12), jitter(rng, a, 0.15)) for f, d, a in modes]


# Each profile: total length, output peak level, and a list of (onset seconds, noise, modes).
PROFILES = {
    # Regular keys: crisp switch click followed ~4 ms later by the stem bottoming out.
    "click": (0.09, 0.55, [
        (0.000, ("bp", 4200, 1.2, 0.0012, 0.9), [(5200, 0.003, 0.30)]),
        (0.004, ("bp", 1800, 0.8, 0.0020, 1.0), [(1150, 0.012, 0.50), (2300, 0.008, 0.35),
                                                 (3500, 0.005, 0.20), (480, 0.025, 0.25)]),
    ]),
    # Spacebar: deep, rounded bottom-out with a faint stabilizer tick.
    "thock": (0.18, 0.80, [
        (0.000, ("lp", 1400, 0.7, 0.0040, 1.2), [(170, 0.045, 0.90), (310, 0.030, 0.60),
                                                 (620, 0.018, 0.35), (1250, 0.008, 0.15)]),
        (0.006, ("bp", 2600, 2.0, 0.0015, 0.25), [(2900, 0.006, 0.08)]),
    ]),
    # Enter: loud, hollow plastic clack, then the stabilizer wire landing.
    "clack": (0.14, 0.85, [
        (0.000, ("bp", 2200, 0.7, 0.0025, 1.2), [(850, 0.028, 0.70), (1600, 0.020, 0.50),
                                                 (2700, 0.012, 0.30), (400, 0.040, 0.35)]),
        (0.009, ("bp", 3000, 1.5, 0.0015, 0.5), [(1900, 0.010, 0.20)]),
    ]),
    # Backspace and modifiers: a short, bright snap.
    "snap": (0.05, 0.60, [
        (0.000, ("hp", 4500, 0.7, 0.0007, 1.2), [(6400, 0.004, 0.50), (3300, 0.006, 0.35),
                                                 (1500, 0.009, 0.15)]),
    ]),
    # Key release: the stem springing back into the top housing. Quieter and lighter.
    "topout": (0.05, 0.28, [
        (0.000, ("bp", 3000, 1.0, 0.0010, 0.8), [(2400, 0.006, 0.30), (4100, 0.004, 0.20),
                                                 (900, 0.012, 0.15)]),
    ]),
}


def render(rng, length, peak, hits):
    n = int(length * SR)
    out = [0.0] * n
    for onset, noise, modes in hits:
        at = int(max(0.0, onset + rng.uniform(-0.001, 0.001) if onset else 0.0) * SR)
        for i, v in enumerate(impact(rng, length, vary_noise(rng, noise), vary_modes(rng, modes))):
            if at + i >= n:
                break
            out[at + i] += v
    out = biquad(out, "hp", 60, 0.7)  # strip DC / sub rumble
    fade_in, fade_out = 5, int(0.005 * SR)
    for i in range(fade_in):
        out[i] *= i / fade_in
    for i in range(fade_out):
        out[n - 1 - i] *= i / fade_out
    scale = peak / max(abs(v) for v in out)
    return [v * scale for v in out]


def write_wav(path, samples):
    with wave.open(path, "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(SR)
        f.writeframes(b"".join(struct.pack("<h", int(max(-1.0, min(1.0, v)) * 32767)) for v in samples))


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    for name, (length, peak, hits) in PROFILES.items():
        for variant in range(1, VARIANTS + 1):
            rng = random.Random(f"{name}-{variant}")
            path = os.path.join(OUT_DIR, f"{name}_{variant}.wav")
            write_wav(path, render(rng, length, peak, hits))
            print("wrote", os.path.relpath(path))


if __name__ == "__main__":
    main()
