"""Holds the app's transcription equal to Basic Pitch's Python (#172).

For each reference take, feeds the same 22 050 Hz audio to Basic Pitch's
`predict` (ONNX Runtime and its Python note creation) and to the app's
`transcribe` example (tract and the Rust port), and compares the notes:
how many each found, how many match (same pitch, onset and offset within
one frame), and the largest onset difference among the matches.

    .venv/bin/python scripts/transcribe-spike/parity.py
"""

import json
import pathlib
import subprocess
import tempfile

import librosa
import numpy as np
import soundfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
TAKES = ROOT / "target" / "takes"
EXAMPLE = ROOT / "target" / "release" / "examples" / "transcribe"
FRAME = 256 / 22050


def python_notes(audio):
    from basic_pitch import ICASSP_2022_MODEL_PATH
    from basic_pitch.inference import Model, predict

    model = Model(pathlib.Path(ICASSP_2022_MODEL_PATH).with_suffix(".onnx"))
    with tempfile.NamedTemporaryFile(suffix=".wav") as f:
        soundfile.write(f.name, audio, 22050, subtype="FLOAT")
        _, _, events = predict(f.name, model)
    return [(s, e, p, a) for s, e, p, a, _ in events]


def rust_notes(audio):
    with tempfile.NamedTemporaryFile(suffix=".f32") as f:
        f.write(audio.astype("<f4").tobytes())
        f.flush()
        out = subprocess.run([str(EXAMPLE), f.name], capture_output=True, text=True, check=True)
    return [(n["start"], n["end"], n["pitch"], n["amplitude"]) for n in json.loads(out.stdout)]


def main():
    worst = 0
    for wav in sorted(TAKES.glob("*.wav")):
        audio, _ = librosa.load(str(wav), sr=22050, mono=True)
        py, rs = python_notes(audio), rust_notes(audio)
        used = set()
        matched, err = 0, 0.0
        for s, e, p, _ in py:
            for j, (s2, e2, p2, _) in enumerate(rs):
                if j not in used and p == p2 and abs(s - s2) <= FRAME + 1e-6 and abs(e - e2) <= FRAME + 1e-6:
                    used.add(j)
                    matched += 1
                    err = max(err, abs(s - s2))
                    break
        worst = max(worst, len(py) - matched, len(rs) - matched)
        print(f"{wav.stem}: python {len(py)}, rust {len(rs)}, matched {matched}, max onset diff {err * 1000:.1f} ms")
    print("PARITY OK" if worst == 0 else f"PARITY: up to {worst} notes differ in a take")


if __name__ == "__main__":
    main()
