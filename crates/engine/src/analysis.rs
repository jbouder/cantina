//! What the stage draws from: a spectrum, a waveform and a level from
//! the most recent audio, and the beat position from the tempo map.
//!
//! The pump runs an [`Analyzer`] once per frame on what a [`Scope`]
//! captured, off the audio thread. The spectrum is log-spaced, since that
//! is how pitch is heard and how every scene lays bars out; each bin is
//! the magnitude of its band as a sine's amplitude, so a full-scale sine
//! reads 1 in the bin it falls in.
//!
//! [`Scope`]: crate::scope::Scope

use std::f32::consts::PI;

use cantina_dsp::fft::{Complex, Fft};

use crate::scope::ScopeHandle;
use crate::tempo::TempoMap;

/// Samples in one analysis window: about 43 ms at 48 kHz.
pub const FFT_SIZE: usize = 2048;
/// Log-spaced spectrum bins in a frame.
pub const SPECTRUM_BINS: usize = 128;
/// Recent samples a frame carries, the same span as the window.
pub const WAVEFORM_SAMPLES: usize = FFT_SIZE;
/// The spectrum's lowest and highest band edges, in Hz. The top is
/// lowered to Nyquist at rates that cannot reach it.
pub const SPECTRUM_LOW_HZ: f32 = 20.0;
/// See [`SPECTRUM_LOW_HZ`].
pub const SPECTRUM_HIGH_HZ: f32 = 20_000.0;

/// One log band: the FFT bins it spans, or, for a band narrower than a
/// bin, the fractional bin at its centre to interpolate at.
#[derive(Debug, Clone, Copy)]
enum Band {
    Bins(usize, usize),
    At(f32),
}

/// Turns captured audio into a frame's analysis sections. Everything is
/// allocated up front; [`Analyzer::analyse`] allocates nothing.
pub struct Analyzer {
    fft: Fft,
    window: Vec<f32>,
    samples: Vec<f32>,
    bins: Vec<Complex>,
    /// Magnitude of every FFT bin up to Nyquist, as an amplitude.
    magnitudes: Vec<f32>,
    bands: Vec<Band>,
    /// The latest spectrum: [`SPECTRUM_BINS`] log-spaced bands from
    /// [`SPECTRUM_LOW_HZ`] up, as amplitudes.
    pub spectrum: Vec<f32>,
    /// The latest [`WAVEFORM_SAMPLES`] samples, oldest first.
    pub waveform: Vec<f32>,
    /// RMS of the window, linear.
    pub rms: f32,
}

impl Analyzer {
    /// An analyzer for audio at `sample_rate`. Allocates.
    pub fn new(sample_rate: f64) -> Self {
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
        let high = SPECTRUM_HIGH_HZ.min(sample_rate as f32 / 2.0);
        let ratio = (high / SPECTRUM_LOW_HZ).powf(1.0 / SPECTRUM_BINS as f32);
        let top = FFT_SIZE / 2;
        let bands = (0..SPECTRUM_BINS)
            .map(|b| {
                let lo = SPECTRUM_LOW_HZ * ratio.powi(b as i32);
                let hi = lo * ratio;
                let first = (lo / bin_hz).ceil() as usize;
                let last = ((hi / bin_hz).ceil() as usize).min(top + 1);
                if first < last {
                    Band::Bins(first, last)
                } else {
                    Band::At((lo * hi).sqrt() / bin_hz)
                }
            })
            .collect();
        Self {
            fft: Fft::new(FFT_SIZE),
            window,
            samples: vec![0.0; FFT_SIZE],
            bins: vec![Complex::ZERO; FFT_SIZE],
            magnitudes: vec![0.0; top + 1],
            bands,
            spectrum: vec![0.0; SPECTRUM_BINS],
            waveform: vec![0.0; WAVEFORM_SAMPLES],
            rms: 0.0,
        }
    }

    /// Analyses the most recent window the scope holds.
    pub fn analyse(&mut self, scope: &ScopeHandle) {
        scope.latest(&mut self.samples);
        self.analyse_samples();
    }

    /// Analyses what is in `samples` now.
    fn analyse_samples(&mut self) {
        let mut sum = 0.0f64;
        for (bin, (&s, &w)) in self
            .bins
            .iter_mut()
            .zip(self.samples.iter().zip(&self.window))
        {
            sum += (s as f64) * (s as f64);
            *bin = Complex { re: s * w, im: 0.0 };
        }
        self.rms = (sum / FFT_SIZE as f64).sqrt() as f32;
        let start = FFT_SIZE - WAVEFORM_SAMPLES;
        self.waveform.copy_from_slice(&self.samples[start..]);
        self.fft.forward(&mut self.bins);
        // A Hann window sums to N/2 and a real sine splits its energy
        // between two bins, so 4/N turns a bin into the sine's amplitude.
        let scale = 4.0 / FFT_SIZE as f32;
        for (m, c) in self.magnitudes.iter_mut().zip(&self.bins) {
            *m = (c.re * c.re + c.im * c.im).sqrt() * scale;
        }
        for (out, band) in self.spectrum.iter_mut().zip(&self.bands) {
            *out = match *band {
                // The band's loudest bin, so a tone between two bins
                // still reads at its level.
                Band::Bins(first, last) => self.magnitudes[first..last]
                    .iter()
                    .fold(0.0f32, |a, &b| a.max(b)),
                Band::At(at) => {
                    let i = (at as usize).min(self.magnitudes.len() - 2);
                    let t = at - i as f32;
                    self.magnitudes[i] * (1.0 - t) + self.magnitudes[i + 1] * t
                }
            };
        }
    }

    /// Back to silence, for a source that stopped.
    pub fn clear(&mut self) {
        self.spectrum.fill(0.0);
        self.waveform.fill(0.0);
        self.rms = 0.0;
    }
}

/// Where the beat is at a timeline sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatPosition {
    /// Tempo at that sample.
    pub bpm: f32,
    /// 0 on the beat, rising to just under 1 before the next.
    pub phase: f32,
    /// Zero-based, as the tempo map counts.
    pub bar: u32,
    /// Zero-based beat within the bar.
    pub beat: u32,
}

/// The bar, beat and phase at `sample`, to the sample rather than the
/// tick.
pub fn beat_at(tempo: &TempoMap, sample: u64, sample_rate: f64) -> BeatPosition {
    let ticks = tempo.samples_to_ticks(sample, sample_rate).max(0.0);
    let whole = ticks.floor() as u64;
    let position = tempo.ticks_to_position(whole);
    let per_beat = tempo.signature_at(position.bar).ticks_per_beat() as f64;
    let into = position.tick as f64 + (ticks - whole as f64);
    BeatPosition {
        bpm: tempo.tempo_at(whole) as f32,
        phase: ((into / per_beat) as f32).clamp(0.0, 1.0 - f32::EPSILON),
        bar: position.bar,
        beat: position.beat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::{TimeSignature, PPQ};

    const RATE: f64 = 48_000.0;

    fn analyse(samples: impl Fn(usize) -> f32) -> Analyzer {
        let mut a = Analyzer::new(RATE);
        for (i, s) in a.samples.iter_mut().enumerate() {
            *s = samples(i);
        }
        a.analyse_samples();
        a
    }

    /// The band a frequency falls in.
    fn band_of(hz: f32) -> usize {
        let high = SPECTRUM_HIGH_HZ.min(RATE as f32 / 2.0);
        let ratio = (high / SPECTRUM_LOW_HZ).powf(1.0 / SPECTRUM_BINS as f32);
        ((hz / SPECTRUM_LOW_HZ).ln() / ratio.ln()).floor() as usize
    }

    #[test]
    fn a_sine_reads_its_amplitude_in_its_band() {
        // On a bin centre, so no leakage softens the peak.
        let hz = 43.0 * RATE as f32 / FFT_SIZE as f32;
        let a = analyse(|i| 0.5 * (2.0 * PI * hz * i as f32 / RATE as f32).sin());
        let band = band_of(hz);
        assert!(
            (a.spectrum[band] - 0.5).abs() < 0.01,
            "{}",
            a.spectrum[band]
        );
        let elsewhere = a
            .spectrum
            .iter()
            .enumerate()
            .filter(|(b, _)| b.abs_diff(band) > 3)
            .map(|(_, v)| *v)
            .fold(0.0f32, f32::max);
        assert!(elsewhere < 0.01, "leakage {elsewhere}");
        assert!((a.rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {}", a.rms);
        assert_eq!(a.waveform.len(), WAVEFORM_SAMPLES);
        assert_eq!(a.waveform[FFT_SIZE - 1], a.samples[FFT_SIZE - 1]);
    }

    #[test]
    fn low_bands_narrower_than_a_bin_interpolate() {
        // 30 Hz sits below the first FFT bin wide enough for its band.
        let a = analyse(|i| (2.0 * PI * 30.0 * i as f32 / RATE as f32).sin());
        let band = band_of(30.0);
        assert!(a.spectrum[band] > 0.3, "{}", a.spectrum[band]);
        assert!(a.spectrum.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn silence_is_zero_everywhere() {
        let mut a = analyse(|_| 0.0);
        assert!(a.spectrum.iter().all(|&v| v == 0.0));
        assert_eq!(a.rms, 0.0);
        a.clear();
        assert!(a.waveform.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn beat_position_follows_the_map_to_the_sample() {
        let tempo = TempoMap::new(120.0, TimeSignature::COMMON);
        // At 120 BPM a beat is half a second.
        let beat = |seconds: f64| beat_at(&tempo, (seconds * RATE) as u64, RATE);
        assert_eq!(
            beat(0.0),
            BeatPosition {
                bpm: 120.0,
                phase: 0.0,
                bar: 0,
                beat: 0
            }
        );
        let b = beat(0.25);
        assert_eq!((b.bar, b.beat), (0, 0));
        assert!((b.phase - 0.5).abs() < 1e-4, "{}", b.phase);
        let b = beat(2.0 + 1.5 + 0.125);
        assert_eq!((b.bar, b.beat), (1, 3));
        assert!((b.phase - 0.25).abs() < 1e-4, "{}", b.phase);

        // In 6/8 a beat is an eighth note, a quarter of a 120 BPM second.
        let tempo = TempoMap::new(120.0, TimeSignature::new(6, 8));
        let b = beat_at(&tempo, (0.25 * 7.0 * RATE) as u64, RATE);
        assert_eq!((b.bar, b.beat), (1, 1));
        assert!(b.phase < 1e-4);
        assert_eq!(TimeSignature::new(6, 8).ticks_per_beat(), PPQ as u64 / 2);
    }
}
