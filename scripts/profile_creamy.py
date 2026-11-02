#!/usr/bin/env python3
"""Creamy profile: warm, deep PBT thock for Clicky (stdlib only).

Upgraded physical model:
- Two-stage attack/decay noise envelopes for realistic plastic-on-plastic transients
- Pitch-dropping resonant modes (simulates mechanical stress on keycap/plate)
- Short-delay comb filtering to mimic internal keyboard case reflections
- Soft tanh saturation for warm, mic-recorded transient compression
"""
import math
import os
import random
import struct
import wave

SR = 48_000
VARIANTS = 5
OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds", "creamy")


def biquad(x, kind, freq, q):
    freq = max(20.0, min(SR * 0.45, freq))
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
    noise_layers = [(kind, freq, q, attack_s, decay_s, gain), ...]
    modes = [(freq, decay_s, amp, drop_ratio), ...]
    """
    n = int(length * SR)
    out = [0.0] * n

    # 1. Multi-layered filtered noise bursts (skin/stem/housing friction & strike)
    for kind, freq, q, att, tau, gain in noise_layers:
        att_samples = max(1, int(att * SR))
        raw = [0.0] * n
        for i in range(n):
            env = (i / att_samples) if i < att_samples else math.exp(-(i - att_samples) / (tau * SR))
            raw[i] = rng.uniform(-1.0, 1.0) * env
        filtered = biquad(raw, kind, freq, q)
        for i in range(n):
            out[i] += filtered[i] * gain

    # 2. Damped resonant modes with impact pitch-drop (plastic & plate body)
    for f, decay, amp, drop in modes:
        phase = rng.uniform(0, 2 * math.pi)
        k = 1.0 / (decay * SR)
        drop_k = 1.0 / (0.0025 * SR)  # 2.5ms pitch settle time
        for i in range(n):
            inst_f = f * (1.0 + drop * math.exp(-i * drop_k))
            phase += 2 * math.pi * inst_f / SR
            out[i] += amp * math.exp(-i * k) * math.sin(phase)

    return out


def jitter(rng, value, pct):
    return value * rng.uniform(1 - pct, 1 + pct)


def vary_noise(rng, layers):
    return [
        (kind, jitter(rng, freq, 0.06), jitter(rng, q, 0.08), att, jitter(rng, tau, 0.12), jitter(rng, gain, 0.10))
        for kind, freq, q, att, tau, gain in layers
    ]


def vary_modes(rng, modes):
    return [
        (jitter(rng, f, 0.04), jitter(rng, d, 0.10), jitter(rng, a, 0.12), drop)
        for f, d, a, drop in modes
    ]


# Profiles tuned to real acoustic measurements of PBT keycaps on a dampened plate.
# Format: (length_s, peak_level, [(onset_s, noise_layers, modes), ...])
PROFILES = {
    # Regular keys: crisp tactile bump + solid, woody POM/PBT bottom-out
    "click": (0.075, 0.65, [
        # Tactile leaf/jacket snap
        (0.000, [
            ("bp", 3400, 1.8, 0.0001, 0.0008, 0.55),
            ("hp", 5200, 0.9, 0.0001, 0.0005, 0.30),
        ], [
            (2800, 0.0018, 0.22, 0.10),
        ]),
        # Stem bottoming out on housing + keycap cavity resonance
        (0.0045, [
            ("lp", 1100, 0.8, 0.0002, 0.0038, 1.35),
            ("bp", 2100, 1.1, 0.0001, 0.0020, 0.95),
            ("bp", 4200, 1.6, 0.0001, 0.0010, 0.45),
        ], [
            (240, 0.011, 0.45, 0.30),
            (680, 0.006, 0.50, 0.18),
            (1450, 0.0035, 0.30, 0.10),
        ]),
    ]),

    # Spacebar: thick 6.25u PBT bar, deep case thump, damped stabilizer hit
    "thock": (0.11, 0.85, [
        # Initial thumb/switch engagement
        (0.000, [
            ("bp", 1600, 1.2, 0.0002, 0.0015, 0.45),
        ], [
            (520, 0.005, 0.25, 0.15),
        ]),
        # Main bottom-out + low-end air displacement
        (0.0035, [
            ("lp", 650, 0.7, 0.0004, 0.0065, 1.70),
            ("bp", 1250, 1.0, 0.0002, 0.0035, 1.05),
            ("bp", 2600, 1.5, 0.0001, 0.0015, 0.35),
        ], [
            (135, 0.018, 0.70, 0.35),
            (340, 0.011, 0.65, 0.22),
            (780, 0.0055, 0.35, 0.12),
        ]),
    ]),

    # Enter: hollow 2.25u keycap cavity ("poppy" mid-range clack + lubed stab landing)
    "clack": (0.095, 0.80, [
        # Primary stem strike
        (0.000, [
            ("lp", 950, 0.8, 0.0002, 0.0045, 1.30),
            ("bp", 1850, 0.9, 0.0001, 0.0028, 1.25),
            ("bp", 3500, 1.4, 0.0001, 0.0012, 0.60),
        ], [
            (210, 0.013, 0.50, 0.28),
            (590, 0.008, 0.65, 0.18),
            (1320, 0.0045, 0.45, 0.12),
        ]),
        # Secondary stabilizer wire/housing settle
        (0.005, [
            ("bp", 2400, 1.4, 0.0001, 0.0015, 0.45),
        ], [
            (920, 0.004, 0.22, 0.10),
        ]),
    ]),

    # Backspace / Modifiers: tight, punchy 1.5u–2u strike (no piercing high-pass hiss)
    "snap": (0.07, 0.70, [
        (0.000, [
            ("lp", 1300, 0.8, 0.0002, 0.0032, 1.15),
            ("bp", 2500, 1.1, 0.0001, 0.0018, 1.00),
            ("bp", 4600, 1.5, 0.0001, 0.0009, 0.50),
        ], [
            (290, 0.009, 0.42, 0.25),
            (840, 0.005, 0.45, 0.15),
            (1950, 0.0028, 0.28, 0.08),
        ]),
    ]),

    # Key release: subtle, muted plastic top-out against upper Polycarbonate/Nylon housing
    "topout": (0.05, 0.30, [
        (0.000, [
            ("lp", 1500, 0.8, 0.0002, 0.0022, 0.75),
            ("bp", 2700, 1.3, 0.0001, 0.0012, 0.55),
        ], [
            (420, 0.005, 0.25, 0.15),
            (1250, 0.003, 0.18, 0.10),
        ]),
    ]),
}


def render(rng, length, peak, hits):
    n = int(length * SR)
    out = [0.0] * n
    for onset, noise_layers, modes in hits:
        at = int(max(0.0, onset + rng.uniform(-0.0006, 0.0006) if onset else 0.0) * SR)
        hit_audio = impact(rng, length, vary_noise(rng, noise_layers), vary_modes(rng, modes))
        for i, v in enumerate(hit_audio):
            if at + i >= n:
                break
            out[at + i] += v

    # Simulate internal keyboard case reflection (0.35 ms acoustic bounce)
    delay = int(0.00035 * SR)
    reflected = list(out)
    for i in range(delay, n):
        reflected[i] -= 0.22 * out[i - delay]
    out = reflected

    # Strip sub-bass rumble and tame harsh digital fizz above 8.5 kHz
    out = biquad(out, "hp", 55, 0.707)
    out = biquad(out, "lp", jitter(rng, 8500, 0.05), 0.707)

    # Normalize and apply subtle analog-style soft clipping (tanh) for punchy transients
    raw_max = max(abs(v) for v in out) or 1.0
    drive = 1.35
    out = [math.tanh((v / raw_max) * drive) for v in out]

    # Envelope fades
    fade_in, fade_out = 8, int(0.006 * SR)
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
