use crate::biquad::Biquad;
use crate::pitch::{self, MAX_LAG, WINDOW};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::readout::Readout;
use crate::smooth::Smoother;

static PARAMS: [ParamInfo; 2] = [
    ParamInfo::new("Reference", Unit::Hertz, 415.0, 466.0, 440.0),
    ParamInfo::toggle("Mute", false),
];

const REFERENCE: ParamId = 0;
const MUTE: ParamId = 1;

static READOUTS: [&str; 2] = ["Pitch", "Clarity"];

/// The analysis runs near this rate: guitar and bass fundamentals sit
/// well below its Nyquist, and a window of [`WINDOW`] samples is then
/// long enough to hold two periods of the lowest string.
const ANALYSIS_RATE: f64 = 12_000.0;
/// Decimated samples between analyses: about 21 ms at 12 kHz, so the
/// display updates around fifty times a second.
const HOP: usize = 256;
/// A window quieter than this is not analysed.
const SILENCE_RMS: f32 = 1e-4;
/// Butterworth pair for the fourth-order anti-aliasing lowpass.
const AA_Q: [f32; 2] = [0.5412, 1.3066];
const MUTE_SECONDS: f32 = 0.005;

/// A chromatic tuner. Audio passes through untouched (or is muted for
/// silent tuning); the pitch and clarity of what passes are measured
/// and published in the readout. The UI turns the pitch into a note
/// name and a cents offset against the reference.
///
/// The detector works on a decimated copy of the input. Each analysis
/// is spread over the blocks that follow its window, one lag of the
/// NSDF per input frame, so the cost is the same small amount in every
/// block instead of a spike every 20 ms.
#[derive(Debug, Clone)]
pub struct Tuner {
    reference: f32,
    mute: bool,
    gain: Smoother,
    decimation: usize,
    phase: usize,
    anti_alias: [Biquad; 2],
    analysis_rate: f32,
    ring: Vec<f32>,
    write: usize,
    since_hop: usize,
    window: Vec<f32>,
    prefix: Vec<f32>,
    nsdf: Vec<f32>,
    /// Next lag to compute; `MAX_LAG` when no analysis is in progress.
    cursor: usize,
    readout: Readout,
    last: Option<pitch::Pitch>,
}

impl Default for Tuner {
    fn default() -> Self {
        Self {
            reference: PARAMS[REFERENCE].default,
            mute: false,
            gain: Smoother::new(1.0),
            decimation: 4,
            phase: 0,
            anti_alias: [Biquad::identity(); 2],
            analysis_rate: ANALYSIS_RATE as f32,
            ring: vec![0.0; WINDOW],
            write: 0,
            since_hop: 0,
            window: vec![0.0; WINDOW],
            prefix: vec![0.0; WINDOW + 1],
            nsdf: vec![0.0; MAX_LAG],
            cursor: MAX_LAG,
            readout: Readout::new(&READOUTS),
            last: None,
        }
    }
}

impl Tuner {
    /// The last pitch detected, if the input was periodic.
    pub fn pitch(&self) -> Option<pitch::Pitch> {
        self.last
    }

    fn snapshot(&mut self) {
        let (tail, head) = self.ring.split_at(self.write);
        self.window[..head.len()].copy_from_slice(head);
        self.window[head.len()..].copy_from_slice(tail);
        pitch::energy_prefix(&self.window, &mut self.prefix);
        let rms = (self.prefix[WINDOW] / WINDOW as f32).sqrt();
        if rms < SILENCE_RMS {
            self.publish(None);
        } else {
            self.cursor = 0;
        }
    }

    fn publish(&mut self, found: Option<pitch::Pitch>) {
        self.last = found;
        let (hz, clarity) = found.map_or((0.0, 0.0), |p| (p.hz, p.clarity));
        self.readout.set(0, hz);
        self.readout.set(1, clarity);
    }

    /// Advances the analysis in progress by up to `lags` lags.
    fn analyse(&mut self, lags: usize) {
        if self.cursor >= MAX_LAG {
            return;
        }
        let to = (self.cursor + lags).min(MAX_LAG);
        pitch::nsdf_range(&self.window, &self.prefix, &mut self.nsdf, self.cursor, to);
        self.cursor = to;
        if self.cursor >= MAX_LAG {
            let found = pitch::finish(&self.nsdf, self.analysis_rate);
            self.publish(found);
        }
    }
}

impl Processor for Tuner {
    fn name(&self) -> &str {
        "Tuner"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            REFERENCE => self.reference,
            MUTE if self.mute => 1.0,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        match id {
            REFERENCE => self.reference = PARAMS[REFERENCE].clamp(value),
            MUTE => {
                self.mute = PARAMS[MUTE].index(value) == 1;
                self.gain.set_target(if self.mute { 0.0 } else { 1.0 });
            }
            _ => {}
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.decimation = ((sample_rate / ANALYSIS_RATE).round() as usize).max(1);
        self.analysis_rate = (sample_rate / self.decimation as f64) as f32;
        let cutoff = self.analysis_rate * 0.4;
        for (filter, q) in self.anti_alias.iter_mut().zip(AA_Q) {
            filter.set_lowpass(sample_rate as f32, cutoff, q);
        }
        self.gain.set_time(MUTE_SECONDS, sample_rate as f32);
        self.gain.snap(self.gain.target());
        self.reset();
    }

    fn reset(&mut self) {
        for f in &mut self.anti_alias {
            f.reset();
        }
        self.ring.fill(0.0);
        self.write = 0;
        self.phase = 0;
        self.since_hop = 0;
        self.cursor = MAX_LAG;
        self.publish(None);
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let channels = buffer.channels().min(MAX_CHANNELS);
        if channels == 0 {
            return;
        }
        let scale = 1.0 / channels as f32;
        for frame in 0..frames {
            let mut x = 0.0;
            for ch in 0..channels {
                x += buffer.channel(ch)[frame];
            }
            let x = x * scale;
            let y = self.anti_alias[0].tick(x);
            let y = self.anti_alias[1].tick(y);
            self.phase += 1;
            if self.phase >= self.decimation {
                self.phase = 0;
                self.ring[self.write] = y;
                self.write = (self.write + 1) % WINDOW;
                self.since_hop += 1;
                if self.since_hop >= HOP && self.cursor >= MAX_LAG {
                    self.since_hop = 0;
                    self.snapshot();
                }
            }
            let gain = self.gain.advance();
            if gain != 1.0 {
                for ch in 0..channels {
                    buffer.channel_mut(ch)[frame] *= gain;
                }
            }
        }
        self.analyse(frames);
    }

    fn readout(&self) -> Option<Readout> {
        Some(self.readout.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::cents;
    use crate::Renderer;
    use std::f32::consts::TAU;

    fn tone(hz: f32, seconds: f32) -> Vec<f32> {
        let sr = 48_000.0;
        (0..(seconds * sr) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                // Six harmonics fading at different rates: a plucked string.
                (1..=6)
                    .map(|n| {
                        let n = n as f32;
                        (TAU * hz * n * t).sin() / n * (-t * n * 0.5).exp()
                    })
                    .sum::<f32>()
                    * 0.3
            })
            .collect()
    }

    #[test]
    fn reads_guitar_and_bass_notes_within_a_cent() {
        for hz in [41.2, 61.7, 98.0, 110.0, 196.0, 329.6, 523.3, 1318.5] {
            let mut tuner = Tuner::default();
            let input = tone(hz, 0.5);
            let out = Renderer::default().render(&mut tuner, std::slice::from_ref(&input));
            assert_eq!(out[0], input, "the tuner changed the audio");
            let pitch = tuner
                .pitch()
                .unwrap_or_else(|| panic!("no pitch at {hz} Hz"));
            let off = cents(pitch.hz, hz);
            assert!(
                off.abs() < 1.0,
                "{hz} Hz read as {} Hz, {off} cents",
                pitch.hz
            );
            let readout = tuner.readout().unwrap();
            assert_eq!(readout.get(0), pitch.hz);
            assert!(readout.get(1) > 0.8);
        }
    }

    #[test]
    fn works_at_other_sample_rates_and_block_sizes() {
        for (rate, block) in [(44_100.0, 128), (96_000.0, 32), (48_000.0, 1000)] {
            let mut tuner = Tuner::default();
            let input: Vec<f32> = (0..(rate as usize / 2))
                .map(|i| 0.3 * (TAU * 220.0 * i as f32 / rate as f32).sin())
                .collect();
            Renderer::new(rate, block).render(&mut tuner, &[input]);
            let pitch = tuner.pitch().expect("a pitch");
            let off = cents(pitch.hz, 220.0);
            assert!(off.abs() < 1.0, "{rate} Hz / {block}: {off} cents");
        }
    }

    #[test]
    fn silence_reads_nothing_and_mute_silences() {
        let mut tuner = Tuner::default();
        let out = Renderer::default().render(&mut tuner, &[vec![0.0; 24_000]]);
        assert!(out[0].iter().all(|s| *s == 0.0));
        assert!(tuner.pitch().is_none());
        assert_eq!(tuner.readout().unwrap().get(0), 0.0);

        let mut muted = Tuner::default();
        muted.set_param(MUTE, 1.0);
        let out = Renderer::default().render(&mut muted, &[tone(110.0, 0.5)]);
        assert!(out[0].iter().all(|s| *s == 0.0), "mute leaked");
        // Still tuning while muted.
        assert!(muted.pitch().is_some());
    }

    #[test]
    fn analysis_cost_is_spread_over_blocks() {
        // One lag per input frame: a 64-frame block never computes more
        // than 64 lags, whatever the window and hop.
        let mut tuner = Tuner::default();
        tuner.prepare(48_000.0, 64);
        let mut lanes = vec![tone(110.0, 0.2)];
        let mut biggest_step = 0;
        let mut before = tuner.cursor;
        Renderer::default().run(&mut lanes, |buffer| {
            tuner.process(buffer);
            if tuner.cursor >= before {
                biggest_step = biggest_step.max(tuner.cursor - before);
            }
            before = tuner.cursor;
        });
        assert!(biggest_step <= 64, "{biggest_step} lags in one block");
    }
}
