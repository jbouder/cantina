//! Waveshaping curves: the memoryless functions at the heart of every
//! drive pedal. Each maps an input sample to an output in about -1 to 1;
//! what they do above unity is what gives a pedal its character.

/// A transfer curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// `tanh`: rounds off gently, the classic overdrive.
    Soft,
    /// A straight clip at ±1: buzzy, lots of high harmonics.
    Hard,
    /// Soft on top, harder and lower underneath, like a mismatched
    /// diode pair: adds even harmonics for a thicker sound.
    Asymmetric,
    /// Folds the wave back on itself past ±1 instead of flattening it:
    /// the harmonics keep changing as the note dies.
    Fold,
    /// Silicon fuzz: huge gain into a hard clip, near a square wave.
    Silicon,
    /// Germanium fuzz: softer and lopsided, with a bias that pushes
    /// the wave off centre so it gates and sputters as it decays.
    Germanium,
    /// Octave fuzz: the wave is rectified before clipping, so the
    /// fundamental doubles.
    Octave,
}

impl Curve {
    /// Display name, for a `Unit::Choice`.
    pub const fn name(self) -> &'static str {
        match self {
            Curve::Soft => "Soft",
            Curve::Hard => "Hard",
            Curve::Asymmetric => "Asymmetric",
            Curve::Fold => "Fold",
            Curve::Silicon => "Silicon",
            Curve::Germanium => "Germanium",
            Curve::Octave => "Octave",
        }
    }

    /// Shapes one sample.
    #[inline]
    pub fn shape(self, x: f32) -> f32 {
        match self {
            Curve::Soft => x.tanh(),
            Curve::Hard => x.clamp(-1.0, 1.0),
            Curve::Asymmetric => {
                if x >= 0.0 {
                    x.tanh()
                } else {
                    (1.5 * x).tanh() / 1.5
                }
            }
            Curve::Fold => {
                // Triangle fold of the line y = x into -1..1.
                let t = (x + 1.0).rem_euclid(4.0);
                if t < 2.0 {
                    t - 1.0
                } else {
                    3.0 - t
                }
            }
            Curve::Silicon => (4.0 * x).clamp(-1.0, 1.0) * 0.9 + 0.1 * (4.0 * x).tanh(),
            Curve::Germanium => {
                let biased = 2.5 * x - 0.35;
                let y = if biased >= 0.0 {
                    biased.tanh()
                } else {
                    0.7 * (biased / 0.7).tanh()
                };
                // Centred so silence rests near zero, then scaled to 1.
                (y + 0.3234) / 1.3234
            }
            Curve::Octave => (3.0 * x.abs() - 0.6).clamp(-1.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Curve; 7] = [
        Curve::Soft,
        Curve::Hard,
        Curve::Asymmetric,
        Curve::Fold,
        Curve::Silicon,
        Curve::Germanium,
        Curve::Octave,
    ];

    #[test]
    fn every_curve_is_bounded_and_rests_near_zero() {
        for curve in ALL {
            for i in -400..=400 {
                let x = i as f32 / 40.0;
                let y = curve.shape(x);
                assert!(y.abs() <= 1.0 + 1e-5, "{curve:?}({x}) = {y}");
            }
            // Silence in is near silence out, except for the rectifier,
            // whose offset the pedal's highpass removes.
            if curve != Curve::Octave {
                assert!(
                    curve.shape(0.0).abs() < 0.05,
                    "{curve:?}(0) = {}",
                    curve.shape(0.0)
                );
            }
        }
    }

    #[test]
    fn curves_differ_from_each_other() {
        let probe: Vec<f32> = (-20..=20).map(|i| i as f32 / 8.0).collect();
        for (i, a) in ALL.iter().enumerate() {
            for b in &ALL[i + 1..] {
                let distance: f32 = probe
                    .iter()
                    .map(|x| (a.shape(*x) - b.shape(*x)).abs())
                    .sum();
                assert!(distance > 0.5, "{a:?} and {b:?} are the same curve");
            }
        }
    }

    #[test]
    fn soft_is_linear_at_low_level_and_octave_doubles() {
        assert!((Curve::Soft.shape(0.01) - 0.01).abs() < 1e-5);
        assert_eq!(Curve::Hard.shape(3.0), 1.0);
        assert!((Curve::Octave.shape(0.5) - Curve::Octave.shape(-0.5)).abs() < 1e-6);
        assert!(Curve::Asymmetric.shape(2.0) > -Curve::Asymmetric.shape(-2.0));
    }
}
