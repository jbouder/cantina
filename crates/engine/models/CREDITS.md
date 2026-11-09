# Models

## `basic-pitch.onnx`

[Basic Pitch](https://github.com/spotify/basic-pitch) by Spotify, the
note transcription model Cantina runs to turn a recorded clip into a tab.
This is `basic_pitch/saved_models/icassp_2022/nmp.onnx` from the
`basic-pitch` 0.4.0 Python package, unchanged (SHA-256
`2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`).

Copyright 2022 Spotify AB, licensed under the Apache License 2.0: see
`basic-pitch.LICENSE` and `basic-pitch.NOTICE` in this folder. The model
is described in Bittner et al., "A Lightweight Instrument-Agnostic Model
for Polyphonic Note Transcription and Multipitch Estimation", ICASSP
2022.

Cantina's `transcribe` module in `crates/engine` runs it with
[tract](https://github.com/sonos/tract) and ports Basic Pitch's note
creation (`note_creation.py`) to Rust; that port is a derivative of the
Apache-2.0 code and is credited in the module's documentation. Why this
model and how it was measured: the
[transcription](https://cantina-docs.vibeproject.workers.dev/architecture/transcription/)
architecture page.
