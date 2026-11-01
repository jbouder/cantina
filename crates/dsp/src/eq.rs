//! A four-band parametric equaliser.

use crate::biquad::Biquad;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};

pub const BANDS: usize = 4;
const PER_BAND: usize = 4;

static TYPES: [&str; 6] = [
    "Peak",
    "Low shelf",
    "High shelf",
    "Low cut",
    "High cut",
    "Notch",
];

const fn kind(name: &'static str, default: usize) -> ParamInfo {
    ParamInfo::choice(name, &TYPES, default)
}

const fn freq(hz: f32) -> ParamInfo {
    ParamInfo::new("Freq", Unit::Hertz, 20.0, 20_000.0, hz)
}

const fn gain() -> ParamInfo {
    ParamInfo::new("Gain", Unit::Decibels, -18.0, 18.0, 0.0)
}

const fn q(value: f32) -> ParamInfo {
    ParamInfo::new("Q", Unit::Generic, 0.1, 10.0, value)
}

/// The band parameters in order: for each band its type, frequency,
/// gain and Q. The type's name is the band's label.
static PARAMS: [ParamInfo; BANDS * PER_BAND] = [
    kind("Band 1", 1),
    freq(100.0),
    gain(),
    q(0.7),
    kind("Band 2", 0),
    freq(400.0),
    gain(),
    q(1.0),
    kind("Band 3", 0),
    freq(1600.0),
    gain(),
    q(1.0),
    kind("Band 4", 2),
    freq(5000.0),
    gain(),
    q(0.7),
];

/// Four bands of peak, shelf, cut or notch, each on both channels.
pub struct Equalizer {
    values: [f32; BANDS * PER_BAND],
    filters: [[Biquad; BANDS]; MAX_CHANNELS],
    dirty: bool,
    sample_rate: f32,
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            values: std::array::from_fn(|i| PARAMS[i].default),
            filters: [[Biquad::identity(); BANDS]; MAX_CHANNELS],
            dirty: true,
            sample_rate: 48_000.0,
        }
    }
}

impl Equalizer {
    fn design(&self, band: usize) -> Biquad {
        let at = band * PER_BAND;
        let kind = PARAMS[at].index(self.values[at]);
        let hz = self.values[at + 1];
        let gain = self.values[at + 2];
        let q = self.values[at + 3];
        let mut f = Biquad::identity();
        let sr = self.sample_rate;
        match kind {
            0 => f.set_peaking(sr, hz, gain, q),
            1 => f.set_lowshelf(sr, hz, gain, q),
            2 => f.set_highshelf(sr, hz, gain, q),
            3 => f.set_highpass(sr, hz, q),
            4 => f.set_lowpass(sr, hz, q),
            _ => f.set_notch(sr, hz, q),
        }
        f
    }

    /// Recomputes coefficients, keeping each filter's state so a knob
    /// move does not click.
    fn update(&mut self) {
        for band in 0..BANDS {
            let designed = self.design(band);
            for ch in 0..MAX_CHANNELS {
                self.filters[ch][band].set_coefficients_from(&designed);
            }
        }
        self.dirty = false;
    }
}

impl Processor for Equalizer {
    fn name(&self) -> &str {
        "EQ"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        self.values.get(id).copied().unwrap_or(0.0)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if let Some(info) = PARAMS.get(id) {
            let clamped = info.clamp(value);
            if clamped != self.values[id] {
                self.values[id] = clamped;
                self.dirty = true;
            }
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        for f in self.filters.iter_mut().flatten() {
            f.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        if self.dirty {
            self.update();
        }
        let channels = buffer.channels().min(MAX_CHANNELS);
        for ch in 0..channels {
            let lane = buffer.channel_mut(ch);
            for band in 0..BANDS {
                self.filters[ch][band].process(lane);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    fn level_at(eq: &mut Equalizer, hz: f32) -> f32 {
        let input: Vec<f32> = (0..24_000)
            .map(|i| 0.5 * (TAU * hz * i as f32 / 48_000.0).sin())
            .collect();
        let out = Renderer::default().render(eq, &[input]);
        let tail = &out[0][12_000..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms / (0.5 / 2f32.sqrt())).log10()
    }

    #[test]
    fn has_four_bands_with_type_frequency_gain_and_q() {
        let eq = Equalizer::default();
        assert_eq!(eq.params().len(), 16);
        for b in 0..BANDS {
            let names: Vec<&str> = eq.params()[b * 4..b * 4 + 4]
                .iter()
                .map(|p| p.name.as_ref())
                .collect();
            assert_eq!(
                names,
                [format!("Band {}", b + 1).as_str(), "Freq", "Gain", "Q"]
            );
            assert!(matches!(eq.params()[b * 4].unit, Unit::Choice(_)));
        }
    }

    #[test]
    fn flat_by_default() {
        let mut eq = Equalizer::default();
        for hz in [50.0, 400.0, 3000.0, 12_000.0] {
            assert!(level_at(&mut eq, hz).abs() < 0.05, "{hz} Hz");
        }
    }

    #[test]
    fn each_type_acts_where_it_says() {
        // Band 2 peak +9 dB at 1 kHz.
        let mut eq = Equalizer::default();
        eq.set_param(5, 1000.0);
        eq.set_param(6, 9.0);
        assert!((level_at(&mut eq, 1000.0) - 9.0).abs() < 0.3);
        assert!(level_at(&mut eq, 100.0).abs() < 0.3);

        // Band 1 low shelf -12 dB at 100 Hz.
        let mut eq = Equalizer::default();
        eq.set_param(2, -12.0);
        assert!((level_at(&mut eq, 30.0) + 12.0).abs() < 0.5);
        assert!(level_at(&mut eq, 2000.0).abs() < 0.3);

        // Band 4 high shelf +6 dB at 5 kHz.
        let mut eq = Equalizer::default();
        eq.set_param(14, 6.0);
        assert!((level_at(&mut eq, 15_000.0) - 6.0).abs() < 0.5);
        assert!(level_at(&mut eq, 300.0).abs() < 0.3);

        // Band 1 as a low cut at 200 Hz.
        let mut eq = Equalizer::default();
        eq.set_param(0, 3.0);
        eq.set_param(1, 200.0);
        assert!(level_at(&mut eq, 40.0) < -20.0);
        assert!(level_at(&mut eq, 2000.0).abs() < 0.3);

        // Band 3 as a high cut at 2 kHz.
        let mut eq = Equalizer::default();
        eq.set_param(8, 4.0);
        eq.set_param(9, 2000.0);
        assert!(level_at(&mut eq, 12_000.0) < -25.0);
        assert!(level_at(&mut eq, 200.0).abs() < 0.3);

        // Band 2 as a notch at 1 kHz.
        let mut eq = Equalizer::default();
        eq.set_param(4, 5.0);
        eq.set_param(5, 1000.0);
        eq.set_param(7, 5.0);
        assert!(level_at(&mut eq, 1000.0) < -30.0);
        assert!(level_at(&mut eq, 500.0).abs() < 1.0);
    }

    #[test]
    fn a_knob_move_does_not_click() {
        let mut eq = Equalizer::default();
        eq.prepare(48_000.0, 64);
        let mut buffer: Vec<f32> = (0..24_000)
            .map(|i| 0.3 * (TAU * 300.0 * i as f32 / 48_000.0).sin())
            .collect();
        for (i, block) in buffer.chunks_mut(64).enumerate() {
            // Sweep band 2's gain up across the render.
            eq.set_param(5, 300.0);
            eq.set_param(6, (i as f32 / 20.0).min(12.0));
            let mut lanes = [block];
            let mut b = Buffer::new(&mut lanes);
            eq.process(&mut b);
        }
        let max_step = buffer
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.1, "{max_step}");
    }
}
