#!/usr/bin/env python3
"""Crisp profile (default): clean, dry, authentic mechanical switch samples for Clicky (stdlib only).

Tuned for a clean, dry, un-exaggerated mechanical switch feel:
- Micro-transients (0.3ms - 2.5ms) in the 1.5kHz - 6.5kHz range for true switch crispness
- High-passed above 180Hz to eliminate muffled, boomy "thudding"
- Distinct two-stage actuation (switch leaf/jacket click -> crisp plastic stem bottom-out)
- Tight, dry decays with zero artificial drum-like pitch drops
"""
import math
import os
import random
import struct
import wave

SR = 48_000
VARIANTS = 5
OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds", "crisp")


def biquad(x, kind, freq, q):
    freq = max(20.0, min(SR * 0.48, freq))
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


def impact(rng, length, noise_layers, modes):
    """
    noise_layers = [(kind, freq, q, decay_s, gain), ...]
    modes = [(freq, decay_s, amp), ...]
    """
    n = int(length * SR)
    out = [0.0] * n

    # 1. Instant-attack filtered noise bursts (sharp mechanical click & plastic contact)
    for kind, freq, q, tau, gain in noise_layers:
        inv_tau = 1.0 / (tau * SR)
        raw = [rng.uniform(-1.0, 1.0) * math.exp(-i * inv_tau) for i in range(n)]
        filtered = biquad(raw, kind, freq, q)
        for i in range(n):
            out[i] += filtered[i] * gain

    # 2. Ultra-short, tight keycap/housing body resonances (no pitch-drop thud)
    for f, decay, amp in modes:
        phase = rng.uniform(0, 2 * math.pi)
        w = 2 * math.pi * f / SR
        k = 1.0 / (decay * SR)
        for i in range(n):
            out[i] += amp * math.exp(-i * k) * math.sin(w * i + phase)

    return out


def jitter(rng, value, pct):
    return value * rng.uniform(1 - pct, 1 + pct)


def vary_noise(rng, layers):
    return [
        (kind, jitter(rng, freq, 0.05), jitter(rng, q, 0.08), jitter(rng, tau, 0.10), jitter(rng, gain, 0.08))
        for kind, freq, q, tau, gain in layers
    ]


def vary_modes(rng, modes):
    return [
        (jitter(rng, f, 0.03), jitter(rng, d, 0.10), jitter(rng, a, 0.10))
        for f, d, a in modes
    ]


# Profiles tuned for crisp, clean mechanical switch articulation without boomy low-end.
# Format: (length_s, peak_level, [(onset_s, noise_layers, modes), ...])
PROFILES = {
    # Regular keys: sharp internal switch click + crisp, dry keycap bottom-out 4.5ms later
    "click": (0.045, 0.65, [
        # Stage 1: Crisp switch leaf / click-jacket snap
        (0.000, [
            ("bp", 4800, 2.2, 0.0005, 1.10),
            ("hp", 6500, 1.2, 0.0004, 0.65),
        ], [
            (4400, 0.0009, 0.30),
            (6200, 0.0006, 0.20),
        ]),
        # Stage 2: Clean plastic stem hitting the bottom housing
        (0.0045, [
            ("bp", 2200, 1.4, 0.0016, 1.00),
            ("bp", 3800, 1.8, 0.0010, 0.75),
        ], [
            (1150, 0.0030, 0.28),
            (2450, 0.0018, 0.25),
        ]),
    ]),

    # Spacebar: longer keycap plastic clack + crisp stabilizer tick (solid, not a bass drum)
    "thock": (0.065, 0.78, [
        # Switch click + stabilizer wire contact
        (0.000, [
            ("bp", 3800, 2.0, 0.0006, 0.85),
            ("bp", 5500, 1.8, 0.0005, 0.45),
        ], [
            (3400, 0.0012, 0.22),
        ]),
        # 6.25u keycap strike — lower mid-range body than 1u keys, but still crisp and dry
        (0.0040, [
            ("bp", 1350, 1.1, 0.0026, 1.15),
            ("bp", 2700, 1.5, 0.0015, 0.85),
            ("bp", 4500, 1.8, 0.0008, 0.40),
        ], [
            (540, 0.0055, 0.35),
            (1180, 0.0038, 0.32),
            (2350, 0.0020, 0.20),
        ]),
    ]),

    # Enter: distinct 2.25u stabilized mechanical clack
    "clack": (0.055, 0.75, [
        # Switch click
        (0.000, [
            ("bp", 4400, 2.0, 0.0005, 0.95),
        ], [
            (4100, 0.0010, 0.25),
        ]),
        # Hollow 2.25u keycap bottom-out + stabilizer bar kiss
        (0.0042, [
            ("bp", 1750, 1.2, 0.0022, 1.10),
            ("bp", 3300, 1.5, 0.0013, 0.85),
        ], [
            (820, 0.0045, 0.32),
            (1680, 0.0032, 0.30),
            (3100, 0.0015, 0.22),
        ]),
    ]),

    # Backspace / Modifiers: tight, snappy stabilized click-clack
    "snap": (0.045, 0.68, [
        (0.000, [
            ("bp", 5000, 2.2, 0.0005, 1.00),
        ], [
            (4700, 0.0008, 0.25),
        ]),
        (0.0038, [
            ("bp", 2400, 1.4, 0.0015, 0.95),
            ("bp", 4200, 1.7, 0.0009, 0.70),
        ], [
            (1300, 0.0028, 0.26),
            (2700, 0.0016, 0.22),
        ]),
    ]),

    # Key release: light, crisp plastic tick as the stem hits the top housing
    "topout": (0.035, 0.28, [
        (0.000, [
            ("bp", 3400, 1.6, 0.0009, 0.85),
            ("bp", 5400, 1.8, 0.0005, 0.50),
        ], [
            (1900, 0.0018, 0.20),
            (3700, 0.0010, 0.15),
        ]),
    ]),
}


def render(rng, length, peak, hits):
    n = int(length * SR)
    out = [0.0] * n
    for onset, noise_layers, modes in hits:
        at = int(max(0.0, onset + rng.uniform(-0.0004, 0.0004) if onset else 0.0) * SR)
        hit_audio = impact(rng, length, vary_noise(rng, noise_layers), vary_modes(rng, modes))
        for i, v in enumerate(hit_audio):
            if at + i >= n:
                break
            out[at + i] += v

    # Cut all muddy bass/thud below 180 Hz, keep air up to 16 kHz for crispness
    out = biquad(out, "hp", 180, 0.707)
    out = biquad(out, "lp", 16000, 0.707)

    # Tiny 2-sample fade-in (preserves the sharp initial click transient) and smooth fade-out
    fade_in, fade_out = 2, int(0.004 * SR)
    for i in range(fade_in):
        out[i] *= i / fade_in
    for i in range(fade_out):
        out[n - 1 - i] *= i / fade_out

    scale = peak / (max(abs(v) for v in out) or 1.0)
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
