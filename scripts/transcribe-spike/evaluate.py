"""Scores transcription approaches on the reference takes (issue #170).

Reads target/takes/<id>.wav and <id>.truth.json, written by

    cargo run --release -p cantina-dsp --example make_takes

and prints, per approach and take, note F1 on onset and pitch (50 ms,
50 cents), F1 with offsets too, the mean onset error of matched notes
and the real-time factor. Run it in a throwaway environment:

    uv venv && uv pip install basic-pitch onnxruntime librosa
    .venv/bin/python scripts/transcribe-spike/evaluate.py

With --tract it also writes target/takes/windows.f32 for the tract probe
and compares the probe's output with ONNX Runtime's.
"""

import json
import pathlib
import sys
import time

import librosa
import mir_eval
import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[2]
TAKES = ROOT / "target" / "takes"


def hz(midi):
    return 440.0 * 2 ** ((np.asarray(midi, dtype=float) - 69) / 12)


def truth(id):
    doc = json.loads((TAKES / f"{id}.truth.json").read_text())
    notes = doc["notes"]
    intervals = np.array([[n["onset"], n["offset"]] for n in notes])
    return doc["instrument"], intervals, hz([n["pitch"] for n in notes])


def score(ref_iv, ref_hz, est_iv, est_hz):
    if len(est_iv) == 0:
        return dict(f1=0.0, f1_off=0.0, p=0.0, r=0.0, onset_ms=float("nan"))
    p, r, f, _ = mir_eval.transcription.precision_recall_f1_overlap(
        ref_iv, ref_hz, est_iv, est_hz, offset_ratio=None
    )
    _, _, f_off, _ = mir_eval.transcription.precision_recall_f1_overlap(
        ref_iv, ref_hz, est_iv, est_hz
    )
    pairs = mir_eval.transcription.match_notes(
        ref_iv, ref_hz, est_iv, est_hz, offset_ratio=None
    )
    err = [abs(ref_iv[i, 0] - est_iv[j, 0]) * 1000 for i, j in pairs]
    return dict(f1=f, f1_off=f_off, p=p, r=r, onset_ms=float(np.mean(err)) if err else float("nan"))


def basic_pitch(path, instrument, tuned):
    from basic_pitch import ICASSP_2022_MODEL_PATH
    from basic_pitch.inference import Model, predict

    model = basic_pitch.model = getattr(basic_pitch, "model", None) or Model(
        pathlib.Path(ICASSP_2022_MODEL_PATH).with_suffix(".onnx")
    )
    kwargs = {}
    if tuned:
        lo, hi = (30.0, 400.0) if instrument == "bass" else (75.0, 1400.0)
        kwargs = dict(minimum_frequency=lo, maximum_frequency=hi, minimum_note_length=58.0)
    _, _, events = predict(str(path), model, **kwargs)
    if not events:
        return np.zeros((0, 2)), np.zeros(0)
    iv = np.array([[e[0], e[1]] for e in events])
    return iv, hz([e[2] for e in events])


def pyin(path, instrument):
    sr = 22050
    hop = 256
    y, _ = librosa.load(str(path), sr=sr, mono=True)
    lo, hi = (30.0, 400.0) if instrument == "bass" else (70.0, 1400.0)
    f0, voiced, _ = librosa.pyin(y, fmin=lo, fmax=hi, sr=sr, hop_length=hop, frame_length=4096)
    onsets = librosa.onset.onset_detect(y=y, sr=sr, hop_length=hop, backtrack=False, units="frames")
    bounds = list(onsets) + [len(f0)]
    iv, pitches = [], []
    for a, b in zip(bounds[:-1], bounds[1:]):
        seg = np.where(voiced[a:b])[0]
        if len(seg) < 3:
            continue
        midi = np.median(librosa.hz_to_midi(f0[a:b][seg]))
        end = a + seg[-1] + 1
        iv.append([a * hop / sr, end * hop / sr])
        pitches.append(round(midi))
    return np.array(iv).reshape(-1, 2), hz(pitches)


APPROACHES = {
    "basic-pitch (defaults)": lambda p, i: basic_pitch(p, i, False),
    "basic-pitch (range set)": lambda p, i: basic_pitch(p, i, True),
    "pyin + onsets": pyin,
}


def main():
    ids = sorted(p.name[: -len(".truth.json")] for p in TAKES.glob("*.truth.json"))
    rows = []
    for name, run in APPROACHES.items():
        run(TAKES / f"{ids[0]}.wav", "guitar")  # warm up: model load, JIT
        for id in ids:
            instrument, ref_iv, ref_hz = truth(id)
            path = TAKES / f"{id}.wav"
            seconds = librosa.get_duration(path=str(path))
            t = time.perf_counter()
            est_iv, est_hz = run(path, instrument)
            elapsed = time.perf_counter() - t
            s = score(ref_iv, ref_hz, est_iv, est_hz)
            rows.append((name, id, len(ref_iv), len(est_iv), s, seconds / elapsed))
    print(f"| Approach | Take | Notes | Found | F1 | F1 with offsets | Precision | Recall | Onset error | Speed |")
    print("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for name, id, n, found, s, rtf in rows:
        print(
            f"| {name} | {id} | {n} | {found} | {s['f1']:.2f} | {s['f1_off']:.2f} | "
            f"{s['p']:.2f} | {s['r']:.2f} | {s['onset_ms']:.0f} ms | {rtf:.0f}× |"
        )
    print()
    for name in APPROACHES:
        mine = [r for r in rows if r[0] == name]
        f1 = np.mean([r[4]["f1"] for r in mine])
        f1o = np.mean([r[4]["f1_off"] for r in mine])
        on = np.nanmean([r[4]["onset_ms"] for r in mine])
        rtf = np.mean([r[5] for r in mine])
        print(f"{name}: mean F1 {f1:.2f}, with offsets {f1o:.2f}, onset error {on:.0f} ms, {rtf:.0f}× real time")

    if "--tract" in sys.argv:
        compare_tract(ids)


def compare_tract(ids):
    """Writes windows for the tract probe, runs ONNX Runtime on the same
    windows and compares the outputs written by the probe."""
    import subprocess

    import onnxruntime as ort
    from basic_pitch import ICASSP_2022_MODEL_PATH

    onnx = str(pathlib.Path(ICASSP_2022_MODEL_PATH).with_suffix(".onnx"))
    window = 43844
    y = np.concatenate([librosa.load(str(TAKES / f"{i}.wav"), sr=22050)[0] for i in ids])
    n = len(y) // window
    windows = y[: n * window].astype("<f4")
    (TAKES / "windows.f32").write_bytes(windows.tobytes())

    sess = ort.InferenceSession(onnx)
    names = [o.name for o in sess.get_outputs()]
    t = time.perf_counter()
    ours = []
    for w in windows.reshape(n, 1, window, 1):
        out = dict(zip(names, sess.run(None, {sess.get_inputs()[0].name: w})))
        # The probe writes outputs in the order tract returns them, which is
        # the graph's declared order.
        ours.extend(out[k].ravel() for k in names)
    ort_ms = (time.perf_counter() - t) * 1000 / n
    ours = np.concatenate(ours)

    probe = ROOT / "scripts" / "transcribe-spike" / "tract-probe"
    out = TAKES / "tract.f32"
    subprocess.run(
        ["cargo", "run", "-q", "--release", "--", onnx, str(TAKES / "windows.f32"), str(out)],
        cwd=probe,
        check=True,
    )
    theirs = np.frombuffer(out.read_bytes(), dtype="<f4")
    print(f"\nONNX Runtime: {ort_ms:.1f} ms per window of {window / 22050:.2f} s")
    print(f"tract vs ONNX Runtime over {n} windows: max abs diff {np.max(np.abs(theirs - ours)):.2e}")


if __name__ == "__main__":
    main()
