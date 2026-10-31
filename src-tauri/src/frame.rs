//! The binary frame the engine streams to the UI at display rate.
//!
//! One format carries meters now and the analysis stream (spectrum,
//! waveform, beat) in M8: the header says how much of each the body
//! holds, so a reader built for meters skips what it does not know. The
//! layout is documented in `docs/frames.md` and decoded by
//! `ui/src/lib/frame.ts`; the two must agree.
//!
//! All integers and floats are little-endian. The header is 48 bytes, a
//! multiple of four, so the UI can view the body as `Float32Array`s
//! straight on the buffer.

/// The four bytes every frame starts with.
pub const MAGIC: [u8; 4] = *b"CNTF";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 48;

pub const FLAG_PLAYING: u16 = 1 << 0;
pub const FLAG_RECORDING: u16 = 1 << 1;
pub const FLAG_BEAT: u16 = 1 << 2;

/// Bar, beat and phase from the tempo map. Not filled in until M8.
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
    pub beat: Option<Beat>,
    /// Peak and RMS pairs, linear: `[peak0, rms0, peak1, rms1, ...]`.
    pub meters: &'a [f32],
    pub spectrum: &'a [f32],
    pub waveform: &'a [f32],
}

impl Frame<'_> {
    pub fn encoded_len(&self) -> usize {
        HEADER_LEN + 4 * (self.meters.len() + self.spectrum.len() + self.waveform.len())
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
        let beat = self.beat.unwrap_or(Beat {
            bpm: 0.0,
            phase: 0.0,
            bar: 0,
            beat: 0,
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
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&beat.bpm.to_le_bytes());
        out.extend_from_slice(&beat.phase.to_le_bytes());
        out.extend_from_slice(&beat.bar.to_le_bytes());
        out.extend_from_slice(&beat.beat.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        for lane in [self.meters, self.spectrum, self.waveform] {
            for v in lane {
                out.extend_from_slice(&v.to_le_bytes());
            }
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
            beat: Some(Beat {
                bpm: 120.0,
                phase: 0.25,
                bar: 3,
                beat: 1,
            }),
            meters: &[0.5, 0.25, 0.75, 0.1],
            spectrum: &[1.0, 2.0, 3.0],
            waveform: &[-1.0, 1.0],
        };
        let b = frame.encode();
        assert_eq!(b.len(), frame.encoded_len());
        assert_eq!(b.len(), HEADER_LEN + 4 * 9);
        assert_eq!(&b[0..4], b"CNTF");
        assert_eq!(u16_at(&b, 4), 1);
        assert_eq!(u16_at(&b, 6), FLAG_PLAYING | FLAG_BEAT);
        assert_eq!(u32_at(&b, 8), 7);
        assert_eq!(u32_at(&b, 12), 2);
        assert_eq!(f64::from_le_bytes(b[16..24].try_into().unwrap()), 1.5);
        assert_eq!(u16_at(&b, 24), 2, "two meters");
        assert_eq!(u16_at(&b, 26), 3, "spectrum bins");
        assert_eq!(u16_at(&b, 28), 2, "waveform samples");
        assert_eq!(f32_at(&b, 32), 120.0);
        assert_eq!(f32_at(&b, 36), 0.25);
        assert_eq!(u32_at(&b, 40), 3);
        assert_eq!(u32_at(&b, 44), 1);
        assert_eq!(f32_at(&b, 48), 0.5);
        assert_eq!(f32_at(&b, 48 + 12), 0.1);
        assert_eq!(f32_at(&b, 48 + 16), 1.0);
        assert_eq!(f32_at(&b, 48 + 28), -1.0);
    }

    #[test]
    fn meters_only_frame_is_small_and_beatless() {
        let frame = Frame {
            sequence: 0,
            dropped: 0,
            position_seconds: 0.0,
            playing: false,
            recording: true,
            beat: None,
            meters: &[0.0, 0.0, 0.0, 0.0],
            spectrum: &[],
            waveform: &[],
        };
        let b = frame.encode();
        assert_eq!(b.len(), HEADER_LEN + 16);
        assert_eq!(u16_at(&b, 6), FLAG_RECORDING);
        assert_eq!(u16_at(&b, 26), 0);
    }
}
