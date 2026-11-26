"""The promo's sound: a soft synth bed, the camera's air, the clicks and
claudibeat's own chimes, timed to scene.html. Then muxed into the MP4s.

    python -I docs/promo/audio.py en ru

Everything is synthesised here; no samples, nothing downloaded.
"""
import subprocess
import sys
import wave
from pathlib import Path

import imageio_ffmpeg
import numpy as np

HERE = Path(__file__).resolve().parent
SR = 48_000
DUR = 20.6
N = int(SR * DUR)
rng = np.random.default_rng(7)


def note(name):
    names = {"C": -9, "C#": -8, "D": -7, "D#": -6, "E": -5, "F": -4, "F#": -3, "G": -2, "G#": -1, "A": 0, "A#": 1, "B": 2}
    pitch, octave = name[:-1], int(name[-1])
    return 440.0 * 2 ** ((names[pitch] + 12 * (octave - 4)) / 12)


def env(n, attack, release):
    e = np.ones(n)
    a, r = int(attack * SR), int(release * SR)
    if a:
        e[:a] = np.sin(np.linspace(0, np.pi / 2, a)) ** 2
    if r:
        e[-r:] *= np.cos(np.linspace(0, np.pi / 2, r)) ** 2
    return e


def add(buf, at, sig, pan=0.0):
    i = int(at * SR)
    j = min(N, i + len(sig))
    if j <= i:
        return
    left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    buf[0, i:j] += sig[: j - i] * left
    buf[1, i:j] += sig[: j - i] * right


def lowpass(x, cutoff):
    # one-pole, run forward and back for no phase smear
    k = np.exp(-2 * np.pi * cutoff / SR)
    def run(v):
        out = np.empty_like(v)
        acc = 0.0
        for i, s in enumerate(v):
            acc = (1 - k) * s + k * acc
            out[i] = acc
        return out
    return run(run(x)[::-1])[::-1]


# ---- the bed: four chords, long and warm -------------------------------------
CHORDS = [
    (0.0, 5.9, ["D3", "A3", "C#4", "E4", "F#4"]),       # Dmaj9
    (5.8, 11.45, ["B2", "F#3", "A3", "D4", "E4"]),      # Bm11
    (11.35, 17.7, ["G2", "D3", "F#3", "B3", "C#4"]),    # Gmaj7#11
    (17.6, DUR, ["D3", "A3", "E4", "F#4", "A4"]),       # Dadd9, home
]


def pad(buf):
    for start, end, notes in CHORDS:
        n = int((end - start + 1.0) * SR)
        t = np.arange(n) / SR
        e = env(n, 1.1, 1.4)
        for k, nm in enumerate(notes):
            f = note(nm)
            for side, cents in ((-1, -4), (1, 4)):
                ff = f * 2 ** (cents / 1200)
                ph = rng.uniform(0, 2 * np.pi)
                v = (np.sin(2 * np.pi * ff * t + ph) + .28 * np.sin(4 * np.pi * ff * t + ph) + .08 * np.sin(6 * np.pi * ff * t))
                v *= 1 + .06 * np.sin(2 * np.pi * (.18 + .03 * k) * t)  # slow breath
                add(buf, start, v * e * .028, pan=side * .55)


# ---- a quiet pluck pattern while the product is on screen ---------------------
def pluck(f, length=.55):
    n = int(length * SR)
    t = np.arange(n) / SR
    v = np.sin(2 * np.pi * f * t) + .35 * np.sin(4 * np.pi * f * t) * np.exp(-t * 14)
    return v * np.minimum(1, t / .004) * np.exp(-t * 7.5)


def arpeggio(buf):
    beat = 60 / 104 / 2  # eighths at 104 bpm
    t = 5.8
    i = 0
    while t < 17.55:
        notes = next(n for s, e, n in CHORDS if s <= t < e)
        f = note(notes[[1, 2, 3, 4, 3, 2][i % 6]]) * 2
        level = .05 * min(1, (t - 5.8) / 1.5) * (1 - max(0, (t - 16.8) / .75))
        add(buf, t, pluck(f) * level, pan=(-.4 if i % 2 else .4))
        t += beat
        i += 1


# ---- air: a breath of filtered noise on every camera move ---------------------
def whoosh(length, peak_at):
    n = int(length * SR)
    noise = rng.standard_normal(n)
    v = lowpass(noise, 1400) * 3
    t = np.arange(n) / SR
    shape = np.where(t < peak_at, (t / peak_at) ** 2, np.exp(-(t - peak_at) * 7))
    return v * shape


def air(buf):
    for at, length, peak in ((2.35, 1.1, .7), (4.75, 1.1, .75), (11.3, .9, .55), (13.65, .9, .55), (17.5, 1.0, .5)):
        add(buf, at, whoosh(length, peak) * .05, pan=0)


# ---- the clicks -----------------------------------------------------------------
def click(up=False):
    n = int(.03 * SR)
    t = np.arange(n) / SR
    burst = rng.standard_normal(n) * np.exp(-t * 900)
    tick = np.sin(2 * np.pi * (2600 if up else 2100) * t) * np.exp(-t * 600)
    return (burst * .5 + tick) * (.55 if up else .8)


def clicks(buf):
    for at, up in ((11.2, False), (11.36, True), (15.0, False), (16.55, True)):
        add(buf, at, click(up) * .35, pan=-.2)


# ---- claudibeat's own chime, the way src/sound.rs makes it ----------------------
def tone(hz, ms):
    n = SR * ms // 1000
    t = np.arange(n) / SR
    return np.sin(2 * np.pi * hz * t) * np.minimum(1, t / .005) * np.exp(-t * 18)


def chimes(buf):
    done = tone(880, 140)
    waiting = np.concatenate([tone(660, 90), tone(880, 90)])
    add(buf, 6.3, done * .5, pan=-.25)
    add(buf, 8.75, waiting * .5, pan=-.25)


# ---- the end card: one bell -------------------------------------------------------
def bell(buf):
    n = int(2.6 * SR)
    t = np.arange(n) / SR
    v = sum(a * np.sin(2 * np.pi * note("D5") * r * t) * np.exp(-t * d) for r, a, d in ((1, 1, 1.6), (2.0, .35, 2.8), (3.01, .18, 4.0), (4.2, .08, 6.0)))
    add(buf, 17.95, v * np.minimum(1, t / .003) * .09, pan=0)
    add(buf, 18.6, (np.sin(2 * np.pi * note("A5") * t) * np.exp(-t * 2.2) * np.minimum(1, t / .003)) * .04, pan=.3)


def main():
    buf = np.zeros((2, N))
    pad(buf)
    arpeggio(buf)
    air(buf)
    clicks(buf)
    chimes(buf)
    bell(buf)
    buf *= env(N, .4, .9)
    peak = np.abs(buf).max()
    buf *= 10 ** (-1.5 / 20) / peak
    out = HERE / "out"
    out.mkdir(exist_ok=True)
    wav = out / "promo.wav"
    pcm = (buf.T * 32767).astype("<i2")
    with wave.open(str(wav), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    print(wav)
    ff = imageio_ffmpeg.get_ffmpeg_exe()
    for lang in sys.argv[1:]:
        src = out / f"claudibeat-{lang}.mp4"
        dst = out / f"claudibeat-{lang}-sound.mp4"
        subprocess.run([ff, "-y", "-loglevel", "error", "-i", str(src), "-i", str(wav), "-map", "0:v", "-map", "1:a",
                        "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart", str(dst)], check=True)
        print(dst, round(dst.stat().st_size / 1e6, 2), "MB")


if __name__ == "__main__":
    main()
