//! Generated fills: a one-bar pattern that plays the groove and then
//! breaks into a fill over its last beat or two.
//!
//! [`generate_fill`] is deterministic for a seed: the same groove, kit
//! pads and seed give the same pattern, so a generated fill can be made
//! again. The result is an ordinary pattern, edited like any other.

use crate::model::{Pattern, PatternId, PatternRow};

/// A small seeded generator (xorshift64*), enough to pick a style and
/// vary velocities without a dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A whole number below `n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// A number from 0 to 1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// How the fill breaks from the groove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Sixteenths on the snare, getting louder.
    SnareRoll,
    /// Down the toms, high to low.
    TomCascade,
    /// Snare and toms in pairs.
    SnareTom,
    /// Kick and snare together on the offbeats.
    Accents,
}

/// Writes a one-bar fill on `groove`, for a bar of `beats` quarter-note
/// beats, using only the `pads` the kit has. The bar starts as the
/// groove's first bar; the last one or two beats (by the seed) become a
/// fill in a style the seed picks, with the kick still on the downbeats.
/// The pattern comes back with id 0, named after the groove plus " fill".
pub fn generate_fill(groove: &Pattern, beats: u32, pads: &[&str], seed: u64) -> Pattern {
    let mut rng = Rng::new(seed);
    let spb = u32::from(groove.steps_per_beat.max(1));
    let length = (spb * beats.max(1)).min(128) as usize;
    let has = |pad: &str| pads.contains(&pad);
    let mut rows: Vec<PatternRow> = Vec::new();
    // The groove's first bar, as far as the kit plays it.
    for row in &groove.rows {
        if !has(&row.pad) {
            continue;
        }
        let steps = (0..length)
            .map(|s| {
                row.steps
                    .get(s % row.steps.len().max(1))
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect();
        rows.push(PatternRow {
            pad: row.pad.clone(),
            steps,
            ..Default::default()
        });
    }
    let fill_beats = if beats >= 2 {
        1 + rng.below(2) as u32
    } else {
        1
    };
    let from = ((beats - fill_beats) * spb) as usize;
    // Clear the fill's span, keeping the kick on each beat.
    for row in &mut rows {
        for (s, v) in row.steps.iter_mut().enumerate().skip(from) {
            let downbeat = s as u32 % spb == 0;
            if !(row.pad == "kick" && downbeat) {
                *v = 0.0;
            }
        }
    }
    let toms: Vec<&str> = ["tom-hi", "tom-mid", "tom-lo"]
        .into_iter()
        .filter(|t| has(t))
        .collect();
    let mut styles = vec![Style::SnareRoll, Style::Accents];
    if !toms.is_empty() {
        styles.push(Style::TomCascade);
        styles.push(Style::SnareTom);
    }
    let style = styles[rng.below(styles.len() as u64) as usize];
    let span = length - from;
    let hit = |rows: &mut Vec<PatternRow>, pad: &str, step: usize, velocity: f32| {
        if !has(pad) || step >= length {
            return;
        }
        let index = match rows.iter().position(|r| r.pad == pad) {
            Some(i) => i,
            None => {
                rows.push(PatternRow {
                    pad: pad.to_string(),
                    steps: vec![0.0; length],
                    ..Default::default()
                });
                rows.len() - 1
            }
        };
        rows[index].steps[step] = velocity.clamp(0.05, 1.0);
    };
    for k in 0..span {
        let step = from + k;
        // Louder toward the bar line, a little uneven, like hands.
        let rise = 0.55 + 0.45 * (k as f32 + 1.0) / span as f32;
        let velocity = rise - 0.08 * rng.unit();
        match style {
            Style::SnareRoll => hit(&mut rows, "snare", step, velocity),
            Style::TomCascade => {
                let tom = toms[(k * toms.len() / span).min(toms.len() - 1)];
                hit(&mut rows, tom, step, velocity);
            }
            Style::SnareTom => {
                let pad = if (k / 2) % 2 == 0 {
                    "snare"
                } else {
                    toms[(k / 4).min(toms.len() - 1)]
                };
                hit(&mut rows, pad, step, velocity);
            }
            Style::Accents => {
                if k % 2 == 1 || k + 1 == span {
                    hit(&mut rows, "snare", step, velocity);
                    hit(&mut rows, "kick", step, velocity * 0.9);
                }
            }
        }
    }
    rows.retain(|r| r.steps.iter().any(|v| *v > 0.0));
    rows.sort_by(|a, b| a.pad.cmp(&b.pad));
    Pattern {
        id: PatternId(0),
        name: format!("{} fill", groove.name),
        steps_per_beat: groove.steps_per_beat,
        length: length as u16,
        swing: groove.swing,
        rows,
        humanize: groove.humanize,
        seed: groove.seed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rock() -> Pattern {
        let mut p = Pattern::new(PatternId(3), "Rock verse");
        let row = |pad: &str, hits: &[usize]| PatternRow {
            pad: pad.into(),
            steps: (0..16)
                .map(|s| if hits.contains(&s) { 1.0 } else { 0.0 })
                .collect(),
            ..Default::default()
        };
        p.rows = vec![
            row("kick", &[0, 8, 10]),
            row("snare", &[4, 12]),
            row("hh-closed", &[0, 2, 4, 6, 8, 10, 12, 14]),
        ];
        p
    }

    const KIT: [&str; 7] = [
        "kick",
        "snare",
        "hh-closed",
        "hh-open",
        "tom-hi",
        "tom-mid",
        "tom-lo",
    ];

    #[test]
    fn a_fill_is_the_groove_then_a_break_and_the_same_for_a_seed() {
        let fill = generate_fill(&rock(), 4, &KIT, 7);
        assert_eq!(fill.name, "Rock verse fill");
        assert_eq!(fill.length, 16);
        fill.validate().unwrap();
        // The first two beats are the groove's.
        let hat = fill.rows.iter().find(|r| r.pad == "hh-closed").unwrap();
        assert_eq!(&hat.steps[..8], &rock().rows[2].steps[..8]);
        // The last beat breaks: no hats there, and something plays.
        assert!(hat.steps[12..].iter().all(|v| *v == 0.0));
        let busy = fill
            .rows
            .iter()
            .filter(|r| r.pad != "kick")
            .any(|r| r.steps[12..].iter().any(|v| *v > 0.0));
        assert!(busy);
        // Deterministic.
        assert_eq!(generate_fill(&rock(), 4, &KIT, 7), fill);
    }

    #[test]
    fn seeds_give_different_fills_and_only_the_kits_pads() {
        let fills: Vec<Pattern> = (0..12)
            .map(|s| generate_fill(&rock(), 4, &KIT, s))
            .collect();
        let distinct: std::collections::HashSet<String> =
            fills.iter().map(|f| format!("{:?}", f.rows)).collect();
        assert!(distinct.len() >= 3, "{} distinct fills", distinct.len());
        // A kit without toms gets snare fills, and no tom rows.
        let small = ["kick", "snare", "hh-closed"];
        for seed in 0..12 {
            let f = generate_fill(&rock(), 4, &small, seed);
            assert!(
                f.rows.iter().all(|r| small.contains(&r.pad.as_str())),
                "{f:?}"
            );
        }
        // Three beats a bar.
        assert_eq!(generate_fill(&rock(), 3, &KIT, 1).length, 12);
    }
}
