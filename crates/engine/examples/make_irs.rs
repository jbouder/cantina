//! Writes the starter cabinet impulse responses into `irs/` at the
//! repository root. They are synthesised, not recorded: an impulse
//! through a speaker-shaped filter (a resonant low cut, a steep high
//! roll-off, a mid dip and a presence peak), a few close reflections
//! for the cone and the microphone, and a short dark tail for the box.
//! Deterministic, so running it again changes nothing.
//!
//!     cargo run -p cantina-engine --example make_irs

use std::path::Path;

use cantina_dsp::Biquad;
use cantina_engine::wav;

const RATE: f32 = 48_000.0;
/// Length of each impulse: 100 ms.
const FRAMES: usize = 4800;

struct Design {
    id: &'static str,
    /// Resonant low cut: frequency and Q.
    low_cut: (f32, f32),
    /// High roll-off, two sections.
    high_cut: f32,
    /// A mid dip or bump: frequency, decibels, Q.
    mid: (f32, f32, f32),
    /// A presence peak: frequency, decibels, Q.
    presence: (f32, f32, f32),
    /// Reflections as (milliseconds, gain).
    reflections: [(f32, f32); 3],
    /// The box tail: level and decay in milliseconds.
    tail: (f32, f32),
}

const DESIGNS: [Design; 4] = [
    Design {
        id: "1x12-open-back",
        low_cut: (95.0, 1.1),
        high_cut: 5200.0,
        mid: (500.0, -2.0, 0.8),
        presence: (2800.0, 3.0, 1.5),
        reflections: [(0.35, -0.25), (0.9, 0.18), (1.7, -0.1)],
        tail: (0.05, 12.0),
    },
    Design {
        id: "2x12-closed",
        low_cut: (75.0, 1.4),
        high_cut: 4700.0,
        mid: (400.0, -3.0, 0.8),
        presence: (2400.0, 2.0, 1.5),
        reflections: [(0.4, -0.3), (1.1, 0.2), (2.2, -0.12)],
        tail: (0.07, 16.0),
    },
    Design {
        id: "4x12-modern",
        low_cut: (70.0, 1.5),
        high_cut: 4300.0,
        mid: (550.0, -5.0, 0.7),
        presence: (2200.0, 3.0, 1.2),
        reflections: [(0.45, -0.35), (1.3, 0.22), (2.6, -0.15)],
        tail: (0.09, 20.0),
    },
    Design {
        id: "4x10-bass",
        low_cut: (45.0, 1.2),
        high_cut: 3600.0,
        mid: (800.0, -2.0, 1.0),
        presence: (110.0, 3.0, 1.2),
        reflections: [(0.3, -0.2), (0.8, 0.15), (1.5, -0.08)],
        tail: (0.08, 18.0),
    },
];

fn synthesise(d: &Design) -> Vec<f32> {
    let mut x = vec![0.0f32; FRAMES];
    x[0] = 1.0;
    // The cone and the microphone: a few close copies with sign flips.
    for (ms, gain) in d.reflections {
        let at = (ms * RATE / 1000.0) as usize;
        x[at] += gain;
    }
    // The box: a short tail of dark noise.
    let mut seed = 0x9E37_79B9u32;
    let mut box_lp = Biquad::identity();
    box_lp.set_lowpass(RATE, 250.0, 0.7);
    for (i, s) in x.iter_mut().enumerate() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
        let t_ms = i as f32 / RATE * 1000.0;
        let env = d.tail.0 * (-t_ms / d.tail.1).exp();
        *s += box_lp.tick(noise) * env;
    }
    // The speaker: resonant low cut, steep high roll-off, mid, presence.
    let mut filters = [Biquad::identity(); 5];
    filters[0].set_highpass(RATE, d.low_cut.0, d.low_cut.1);
    filters[1].set_lowpass(RATE, d.high_cut, 0.54);
    filters[2].set_lowpass(RATE, d.high_cut * 1.15, 1.31);
    filters[3].set_peaking(RATE, d.mid.0, d.mid.1, d.mid.2);
    filters[4].set_peaking(RATE, d.presence.0, d.presence.1, d.presence.2);
    for f in &mut filters {
        f.process(&mut x);
    }
    // Fade the last 20 ms so the file ends cleanly.
    let fade = (0.02 * RATE) as usize;
    for i in 0..fade {
        x[FRAMES - fade + i] *= 1.0 - i as f32 / fade as f32;
    }
    let energy: f32 = x.iter().map(|s| s * s).sum();
    let scale = 1.0 / energy.sqrt();
    for s in &mut x {
        *s *= scale;
    }
    x
}

fn main() -> anyhow::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../irs");
    std::fs::create_dir_all(&dir)?;
    for d in &DESIGNS {
        let path = dir.join(format!("{}.wav", d.id));
        wav::write(&path, RATE as f64, &[synthesise(d)])?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
