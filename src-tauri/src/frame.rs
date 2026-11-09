//! The binary frame the engine streams to the UI at display rate.
//!
//! One format carries meters, pedal readouts and the analysis stream
//! (spectrum, waveform, level, beat): the header says how much of each
//! the body holds, so a reader built for meters skips what it does not
//! know. The layout is documented in `docs/src/content/docs/architecture/frames.md` and decoded by
//! `ui/src/lib/frame.ts`; the two must agree.
//!
//! All integers and floats are little-endian. The header is 48 bytes, a
//! multiple of four, so the UI can view the body as `Float32Array`s
//! straight on the buffer.

/// The four bytes every frame starts with.
pub const MAGIC: [u8; 4] = *b"CNTF";
pub const VERSION: u16 = 3;
pub const HEADER_LEN: usize = 48;

pub const FLAG_PLAYING: u16 = 1 << 0;
pub const FLAG_RECORDING: u16 = 1 << 1;
pub const FLAG_BEAT: u16 = 1 << 2;
pub const FLAG_ANALYSIS: u16 = 1 << 3;
/// A count-in is running: the beat field holds the beat being counted.
pub const FLAG_COUNT_IN: u16 = 1 << 4;
/// The analysis trailer after the readouts: RMS and source.
pub const ANALYSIS_LEN: usize = 8;

/// Bar, beat and phase from the tempo map, while the transport rolls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    pub bpm: f32,
    pub phase: f32,
    pub bar: u32,
    pub beat: u32,
}

/// What one frame carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame<'a> {
    pub sequence: u32,
    /// Frames the sender skipped so far because the UI had not taken the
    /// previous one.
    pub dropped: u32,
    pub position_seconds: f64,
    pub playing: bool,
    pub recording: bool,
    /// The beat a count-in is on, from 1; zero with no count-in.
    pub count_in: u32,
    pub beat: Option<Beat>,
    /// Peak and RMS pairs, linear: `[peak0, rms0, peak1, rms1, ...]`.
    pub meters: &'a [f32],
    pub spectrum: &'a [f32],
    pub waveform: &'a [f32],
    /// Every pedal readout in the graph, in track and chain order; the
    /// status says which pedal owns how many.
    pub readouts: &'a [f32],
    pub analysis: Option<Analysis>,
}

/// What the spectrum and waveform were taken from, and its level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Analysis {
    /// RMS of the analysis window, linear.
    pub rms: f32,
    /// The meter that measures the same signal: 1 for the master, 2 and
    /// up for a track.
    pub source: u32,
}

impl Frame<'_> {
    pub fn encoded_len(&self) -> usize {
        HEADER_LEN
            + 4 * (self.meters.len()
                + self.spectrum.len()
                + self.waveform.len()
                + self.readouts.len())
            + if self.analysis.is_some() {
                ANALYSIS_LEN
            } else {
                0
            }
    }

    /// Writes the frame into `out`, reusing its allocation.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.encoded_len());
        let mut flags = 0u16;
        if self.playing {
            flags |= FLAG_PLAYING;
        }
        if self.recording {
            flags |= FLAG_RECORDING;
        }
        if self.beat.is_some() {
            flags |= FLAG_BEAT;
        }
        if self.analysis.is_some() {
            flags |= FLAG_ANALYSIS;
        }
        if self.count_in > 0 {
            flags |= FLAG_COUNT_IN;
        }
        // The timeline holds through a count-in, so there is no beat
        // for its count to displace.
        let beat = self.beat.unwrap_or(Beat {
            bpm: 0.0,
            phase: 0.0,
            bar: 0,
            beat: self.count_in,
        });
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out.extend_from_slice(&self.dropped.to_le_bytes());
        out.extend_from_slice(&self.position_seconds.to_le_bytes());
        out.extend_from_slice(&((self.meters.len() / 2) as u16).to_le_bytes());
        out.extend_from_slice(&(self.spectrum.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.waveform.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.readouts.len() as u16).to_le_bytes());
        out.extend_from_slice(&beat.bpm.to_le_bytes());
        out.extend_from_slice(&beat.phase.to_le_bytes());
        out.extend_from_slice(&beat.bar.to_le_bytes());
        out.extend_from_slice(&beat.beat.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        for lane in [self.meters, self.spectrum, self.waveform, self.readouts] {
            for v in lane {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        if let Some(analysis) = self.analysis {
            out.extend_from_slice(&analysis.rms.to_le_bytes());
            out.extend_from_slice(&analysis.source.to_le_bytes());
        }
    }

    #[cfg(test)]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn f32_at(b: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn header_matches_the_documented_layout() {
        let frame = Frame {
            sequence: 7,
            dropped: 2,
            position_seconds: 1.5,
            playing: true,
            recording: false,
            count_in: 0,
            beat: Some(Beat {
                bpm: 120.0,
                phase: 0.25,
                bar: 3,
                beat: 1,
            }),
            meters: &[0.5, 0.25, 0.75, 0.1],
            spectrum: &[1.0, 2.0, 3.0],
            waveform: &[-1.0, 1.0],
            readouts: &[440.0, 0.9],
            analysis: Some(Analysis {
                rms: 0.5,
                source: 1,
            }),
        };
        let b = frame.encode();
        assert_eq!(b.len(), frame.encoded_len());
        assert_eq!(b.len(), HEADER_LEN + 4 * 11 + ANALYSIS_LEN);
        assert_eq!(&b[0..4], b"CNTF");
        assert_eq!(u16_at(&b, 4), 3);
        assert_eq!(u16_at(&b, 6), FLAG_PLAYING | FLAG_BEAT | FLAG_ANALYSIS);
        assert_eq!(u32_at(&b, 8), 7);
        assert_eq!(u32_at(&b, 12), 2);
        assert_eq!(f64::from_le_bytes(b[16..24].try_into().unwrap()), 1.5);
        assert_eq!(u16_at(&b, 24), 2, "two meters");
        assert_eq!(u16_at(&b, 26), 3, "spectrum bins");
        assert_eq!(u16_at(&b, 28), 2, "waveform samples");
        assert_eq!(u16_at(&b, 30), 2, "readouts");
        assert_eq!(f32_at(&b, 32), 120.0);
        assert_eq!(f32_at(&b, 36), 0.25);
        assert_eq!(u32_at(&b, 40), 3);
        assert_eq!(u32_at(&b, 44), 1);
        assert_eq!(f32_at(&b, 48), 0.5);
        assert_eq!(f32_at(&b, 48 + 12), 0.1);
        assert_eq!(f32_at(&b, 48 + 16), 1.0);
        assert_eq!(f32_at(&b, 48 + 28), -1.0);
        assert_eq!(f32_at(&b, 48 + 36), 440.0);
        assert_eq!(f32_at(&b, 48 + 40), 0.9);
        assert_eq!(f32_at(&b, 48 + 44), 0.5);
        assert_eq!(u32_at(&b, 48 + 48), 1);
    }

    #[test]
    fn a_count_in_travels_in_the_beat_field() {
        let frame = Frame {
            sequence: 0,
            dropped: 0,
            position_seconds: 0.0,
            playing: false,
            recording: true,
            count_in: 3,
            beat: None,
            meters: &[],
            spectrum: &[],
            waveform: &[],
            readouts: &[],
            analysis: None,
        };
        let b = frame.encode();
        assert_eq!(u16_at(&b, 6), FLAG_RECORDING | FLAG_COUNT_IN);
        assert_eq!(u32_at(&b, 44), 3);
    }

    #[test]
    fn meters_only_frame_is_small_and_beatless() {
        let frame = Frame {
            sequence: 0,
            dropped: 0,
            position_seconds: 0.0,
            playing: false,
            recording: true,
            count_in: 0,
            beat: None,
            meters: &[0.0, 0.0, 0.0, 0.0],
            spectrum: &[],
            waveform: &[],
            readouts: &[],
            analysis: None,
        };
        let b = frame.encode();
        assert_eq!(b.len(), HEADER_LEN + 16);
        assert_eq!(u16_at(&b, 6), FLAG_RECORDING);
        assert_eq!(u16_at(&b, 26), 0);
        assert_eq!(u16_at(&b, 30), 0);
    }

    /// The frame `ui/src/lib/frame.fixture.bin` holds. The UI's tests
    /// decode the same file, so the two sides cannot drift apart.
    fn fixture() -> Vec<u8> {
        let spectrum: Vec<f32> = (0..8).map(|i| i as f32 / 8.0).collect();
        let waveform: Vec<f32> = (0..6).map(|i| (i as f32 - 3.0) / 4.0).collect();
        Frame {
            sequence: 0x0102_0304,
            dropped: 5,
            position_seconds: 12.25,
            playing: true,
            recording: true,
            count_in: 0,
            beat: Some(Beat {
                bpm: 96.5,
                phase: 0.75,
                bar: 17,
                beat: 2,
            }),
            meters: &[0.5, 0.25, 1.0, 0.5, 0.125, 0.0625],
            spectrum: &spectrum,
            waveform: &waveform,
            readouts: &[440.0, 0.875, -6.5],
            analysis: Some(Analysis {
                rms: 0.375,
                source: 2,
            }),
        }
        .encode()
    }

    #[test]
    fn the_ui_fixture_matches_the_encoder() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/lib/frame.fixture.bin"
        );
        if std::env::var_os("CANTINA_WRITE_FIXTURES").is_some() {
            std::fs::write(path, fixture()).unwrap();
        }
        let file = std::fs::read(path).expect("run with CANTINA_WRITE_FIXTURES=1 to write it");
        assert_eq!(
            file,
            fixture(),
            "the frame layout changed: rerun with CANTINA_WRITE_FIXTURES=1 and update frame.ts"
        );
    }

    /// Writes `packages/scenes/test/recording.bin` when asked: a second
    /// of frames from the real analyser, 60 of them, over a kick on every
    /// beat at 120 BPM, a hat between and a chord, with the beat fields
    /// set. The scenes' harness draws every scene from it. Each frame is
    /// a little-endian u32 length and then the frame.
    #[test]
    fn record_the_scene_harness_frames() {
        use cantina_dsp::Buffer;
        use cantina_engine::graph::{BlockContext, Node};
        use cantina_engine::{beat_at, Analyzer, Scope, TempoMap};
        if std::env::var_os("CANTINA_WRITE_FIXTURES").is_none() {
            return;
        }
        const RATE: f64 = 48_000.0;
        const BLOCK: usize = 800;
        let tempo = TempoMap::default();
        let (mut scope, handle) = Scope::new();
        let mut analyzer = Analyzer::new(RATE);
        let mut noise = 0x1234_5678u32;
        let mut out = Vec::new();
        let mut bytes = Vec::new();
        // Lead in by a window so the first frame is full.
        let lead = 2048;
        for frame in 0..(60 + lead / BLOCK + 1) {
            let mut lane = vec![0.0f32; BLOCK];
            for (i, v) in lane.iter_mut().enumerate() {
                let n = (frame * BLOCK + i) as f64 - lead as f64;
                let t = n.max(0.0) / RATE;
                let into_beat = t % 0.5;
                let into_half = t % 0.25;
                let kick = (-into_beat * 18.0).exp()
                    * (2.0
                        * std::f64::consts::PI
                        * (50.0 + 60.0 * (-into_beat * 30.0).exp())
                        * into_beat)
                        .sin();
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (noise >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0;
                let hat = if into_beat >= 0.25 {
                    (-into_half * 60.0).exp() * white
                } else {
                    0.0
                };
                let chord: f64 = [196.0, 247.0, 294.0]
                    .iter()
                    .map(|hz| (2.0 * std::f64::consts::PI * hz * t).sin())
                    .sum();
                *v = if n < 0.0 {
                    0.0
                } else {
                    (0.7 * kick + 0.15 * hat + 0.08 * chord) as f32
                };
            }
            let mut right = lane.clone();
            let mut lanes: [&mut [f32]; 2] = [&mut lane, &mut right];
            let mut buffer = Buffer::new(&mut lanes);
            let ctx = BlockContext {
                sample_rate: RATE,
                frames: BLOCK,
                span: BLOCK as u64,
                rate: 1.0,
                playing: true,
                position: 0,
                loop_range: None,
                count_in: None,
                tempo: &tempo,
            };
            scope.process(&ctx, &mut buffer);
            let written = handle.written();
            if (written as usize) < lead + BLOCK {
                continue;
            }
            analyzer.analyse(&handle);
            let heard = written - lead as u64;
            let b = beat_at(&tempo, heard, RATE);
            Frame {
                sequence: out.len() as u32,
                dropped: 0,
                position_seconds: heard as f64 / RATE,
                playing: true,
                recording: false,
                count_in: 0,
                beat: Some(Beat {
                    bpm: b.bpm,
                    phase: b.phase,
                    bar: b.bar,
                    beat: b.beat,
                }),
                meters: &[analyzer.rms, analyzer.rms, analyzer.rms, analyzer.rms],
                spectrum: &analyzer.spectrum,
                waveform: &analyzer.waveform,
                readouts: &[],
                analysis: Some(Analysis {
                    rms: analyzer.rms,
                    source: 1,
                }),
            }
            .encode_into(&mut bytes);
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&bytes);
            if out.len() > 60 * (bytes.len() + 4) - 1 {
                break;
            }
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../packages/scenes/test/recording.bin"
        );
        std::fs::write(path, out).unwrap();
    }
}
