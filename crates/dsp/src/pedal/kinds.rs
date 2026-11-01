//! The primitives a pedal is built from, as a catalog: what each kind
//! does, which fields it has and what each takes. Validation checks a
//! file against it, and the designer draws its node palette and side
//! panel from it.

use crate::lfo::NOTE_NAMES;

/// A node kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindInfo {
    /// The `kind` a file uses.
    pub kind: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
    /// Takes no audio in: the pedal's input and the oscillator.
    pub source: bool,
    /// Produces a control signal for bindings, not audio for edges.
    pub control: bool,
    /// Sums any number of incoming edges; other kinds take one.
    pub sums: bool,
    pub fields: &'static [FieldInfo],
}

/// One field of a kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldInfo {
    pub name: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
    pub kind: FieldKind,
    /// May follow a signal sample by sample.
    pub modulatable: bool,
    /// Eases toward a parameter change over the node's `smooth` time.
    pub smoothed: bool,
}

/// What a field takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    Number {
        min: f32,
        max: f32,
        default: f32,
        unit: &'static str,
    },
    /// One of `options`, by name in a file and by index when bound.
    Name {
        options: &'static [&'static str],
        default: usize,
    },
    Bool {
        default: bool,
    },
}

const fn number(
    name: &'static str,
    label: &'static str,
    unit: &'static str,
    min: f32,
    max: f32,
    default: f32,
    summary: &'static str,
) -> FieldInfo {
    FieldInfo {
        name,
        label,
        summary,
        kind: FieldKind::Number {
            min,
            max,
            default,
            unit,
        },
        modulatable: false,
        smoothed: false,
    }
}

const fn name(
    name: &'static str,
    label: &'static str,
    options: &'static [&'static str],
    default: usize,
    summary: &'static str,
) -> FieldInfo {
    FieldInfo {
        name,
        label,
        summary,
        kind: FieldKind::Name { options, default },
        modulatable: false,
        smoothed: false,
    }
}

const fn flag(
    name: &'static str,
    label: &'static str,
    default: bool,
    summary: &'static str,
) -> FieldInfo {
    FieldInfo {
        name,
        label,
        summary,
        kind: FieldKind::Bool { default },
        modulatable: false,
        smoothed: false,
    }
}

const fn modulatable(mut field: FieldInfo) -> FieldInfo {
    field.modulatable = true;
    field
}

const fn smoothed(mut field: FieldInfo) -> FieldInfo {
    field.smoothed = true;
    field
}

const fn smooth(default: f32) -> FieldInfo {
    number(
        "smooth",
        "Smoothing",
        "s",
        0.0,
        1.0,
        default,
        "Seconds a parameter change takes to settle; 0 applies it on the next block",
    )
}

/// The filter designs a `biquad` offers, in the order a bound choice
/// indexes them.
pub static BIQUAD_TYPES: [&str; 6] = [
    "lowpass",
    "highpass",
    "peak",
    "lowshelf",
    "highshelf",
    "notch",
];

/// The curves a `waveshaper` offers; see `shaper.rs`.
pub static CURVES: [&str; 7] = [
    "soft",
    "hard",
    "asymmetric",
    "fold",
    "silicon",
    "germanium",
    "octave",
];

/// The shapes an `lfo` offers; see `lfo.rs`.
pub static SHAPES: [&str; 5] = ["sine", "triangle", "square", "saw", "random"];

static GAIN_FIELDS: [FieldInfo; 3] = [
    modulatable(smoothed(number(
        "db",
        "Gain",
        "dB",
        -96.0,
        96.0,
        0.0,
        "Gain in decibels; eased in the linear domain",
    ))),
    modulatable(smoothed(number(
        "linear",
        "Multiplier",
        "",
        0.0,
        16.0,
        1.0,
        "A plain multiplier instead of decibels; set one or the other",
    ))),
    smooth(0.005),
];

static BIQUAD_FIELDS: [FieldInfo; 5] = [
    name(
        "type",
        "Type",
        &BIQUAD_TYPES,
        0,
        "The design: a cut, a bell, a shelf or a notch",
    ),
    modulatable(smoothed(number(
        "freq",
        "Frequency",
        "Hz",
        20.0,
        20_000.0,
        1000.0,
        "Corner or centre frequency",
    ))),
    smoothed(number(
        "gain",
        "Gain",
        "dB",
        -24.0,
        24.0,
        0.0,
        "Boost or cut for a peak or shelf; the cuts and the notch ignore it",
    )),
    smoothed(number(
        "q",
        "Q",
        "",
        0.1,
        10.0,
        std::f32::consts::FRAC_1_SQRT_2,
        "Width: 0.7071 is a flat Butterworth cut",
    )),
    smooth(0.0),
];

static SHAPER_FIELDS: [FieldInfo; 2] = [
    name(
        "curve",
        "Curve",
        &CURVES,
        0,
        "The transfer curve; what it does past full scale is the character",
    ),
    number(
        "oversample",
        "Oversampling",
        "×",
        1.0,
        8.0,
        4.0,
        "1, 2, 4 or 8 times the rate; anything above 1 adds 22 samples of latency, the same at every factor",
    ),
];

static DELAY_FIELDS: [FieldInfo; 5] = [
    modulatable(smoothed(number(
        "time",
        "Time",
        "ms",
        0.1,
        10_000.0,
        300.0,
        "Delay in milliseconds; a change slides the read head, like tape",
    ))),
    smoothed(number(
        "feedback",
        "Feedback",
        "",
        0.0,
        0.99,
        0.0,
        "How much of the delayed signal goes round again",
    )),
    smoothed(number(
        "mix",
        "Mix",
        "",
        0.0,
        1.0,
        1.0,
        "Dry at 0, the delayed signal alone at 1",
    )),
    number(
        "max",
        "Longest time",
        "ms",
        1.0,
        10_000.0,
        2000.0,
        "The line's length; memory is reserved for this at prepare",
    ),
    smooth(0.02),
];

static LFO_FIELDS: [FieldInfo; 9] = [
    number(
        "rate",
        "Rate",
        "Hz",
        0.01,
        50.0,
        1.0,
        "Cycles per second when not synced",
    ),
    name("shape", "Shape", &SHAPES, 0, "The wave"),
    flag(
        "sync",
        "Sync",
        false,
        "Lock to the transport at the note value instead of the rate",
    ),
    name(
        "note",
        "Note",
        &NOTE_NAMES,
        3,
        "Cycle length when synced, in note values",
    ),
    smoothed(number(
        "depth",
        "Depth",
        "",
        0.0,
        1.0,
        1.0,
        "Scales the output; bindings see 0 at no depth",
    )),
    number(
        "phase",
        "Phase",
        "",
        0.0,
        1.0,
        0.0,
        "Where in the cycle the wave is read, in cycles",
    ),
    number(
        "spread",
        "Stereo spread",
        "",
        0.0,
        1.0,
        0.0,
        "How far ahead the right channel reads, in cycles",
    ),
    flag(
        "unipolar",
        "Unipolar",
        false,
        "Run from 0 to 1 instead of -1 to 1, so a map's ends are the wave's",
    ),
    smooth(0.02),
];

static ALLPASS_FIELDS: [FieldInfo; 5] = [
    number(
        "stages",
        "Stages",
        "",
        1.0,
        16.0,
        4.0,
        "First-order allpasses in a row; each pair makes a notch when mixed with the dry signal",
    ),
    modulatable(smoothed(number(
        "freq",
        "Frequency",
        "Hz",
        20.0,
        20_000.0,
        1000.0,
        "Where the phase shift is centred",
    ))),
    smoothed(number(
        "feedback",
        "Feedback",
        "",
        0.0,
        0.95,
        0.0,
        "The cascade's output fed back to its input, which deepens the notches",
    )),
    smoothed(number(
        "mix",
        "Mix",
        "",
        0.0,
        1.0,
        1.0,
        "Dry at 0, the shifted signal alone at 1; a phaser sits at 0.5",
    )),
    smooth(0.02),
];

static KINDS: [KindInfo; 10] = [
    KindInfo {
        kind: "input",
        label: "Input",
        summary: "The signal coming into the pedal. Exactly one.",
        source: true,
        control: false,
        sums: false,
        fields: &[],
    },
    KindInfo {
        kind: "output",
        label: "Output",
        summary: "The signal leaving the pedal. Exactly one.",
        source: false,
        control: false,
        sums: false,
        fields: &[],
    },
    KindInfo {
        kind: "gain",
        label: "Gain",
        summary: "Scales the signal, in decibels or by a multiplier, smoothly.",
        source: false,
        control: false,
        sums: false,
        fields: &GAIN_FIELDS,
    },
    KindInfo {
        kind: "biquad",
        label: "Filter",
        summary: "A second-order filter: lowpass, highpass, peak, shelf or notch.",
        source: false,
        control: false,
        sums: false,
        fields: &BIQUAD_FIELDS,
    },
    KindInfo {
        kind: "waveshaper",
        label: "Waveshaper",
        summary: "Clips or folds the signal through a curve, oversampled so the harmonics do not alias.",
        source: false,
        control: false,
        sums: false,
        fields: &SHAPER_FIELDS,
    },
    KindInfo {
        kind: "delay",
        label: "Delay",
        summary: "A delay line with feedback and a dry/wet mix; bind the time to an lfo for chorus and flanging.",
        source: false,
        control: false,
        sums: false,
        fields: &DELAY_FIELDS,
    },
    KindInfo {
        kind: "lfo",
        label: "LFO",
        summary: "A low-frequency oscillator, free or synced to the tempo. Drives other nodes' fields; it carries no audio.",
        source: true,
        control: true,
        sums: false,
        fields: &LFO_FIELDS,
    },
    KindInfo {
        kind: "allpass",
        label: "Allpass",
        summary: "A cascade of allpass stages with feedback: flat in level, it shifts phase, and mixed with the dry signal makes the notches of a phaser.",
        source: false,
        control: false,
        sums: false,
        fields: &ALLPASS_FIELDS,
    },
    KindInfo {
        kind: "mix",
        label: "Mix",
        summary: "Sums everything connected to it. Put gains in front to weight them.",
        source: false,
        control: false,
        sums: true,
        fields: &[],
    },
    KindInfo {
        kind: "split",
        label: "Split",
        summary: "Passes the signal through, as a junction to fan it out from.",
        source: false,
        control: false,
        sums: false,
        fields: &[],
    },
];

/// Every kind, in palette order.
pub fn kinds() -> &'static [KindInfo] {
    &KINDS
}

/// The kind called `name`, if any.
pub fn kind(name: &str) -> Option<&'static KindInfo> {
    KINDS.iter().find(|k| k.kind == name)
}

impl KindInfo {
    pub fn field(&self, name: &str) -> Option<&'static FieldInfo> {
        self.fields.iter().find(|f| f.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_unique_and_fields_within_a_kind_are_too() {
        for (i, k) in KINDS.iter().enumerate() {
            assert!(
                !KINDS[i + 1..].iter().any(|o| o.kind == k.kind),
                "{}",
                k.kind
            );
            for (j, f) in k.fields.iter().enumerate() {
                assert!(
                    !k.fields[j + 1..].iter().any(|o| o.name == f.name),
                    "{}.{}",
                    k.kind,
                    f.name
                );
                if let FieldKind::Number {
                    min, max, default, ..
                } = f.kind
                {
                    assert!(
                        min < max && default >= min && default <= max,
                        "{}.{}",
                        k.kind,
                        f.name
                    );
                }
                if let FieldKind::Name { options, default } = f.kind {
                    assert!(default < options.len(), "{}.{}", k.kind, f.name);
                }
            }
        }
        assert!(kind("gain").is_some());
        assert!(kind("Gain").is_none());
        assert_eq!(
            kind("lfo").unwrap().field("note").map(|f| f.name),
            Some("note")
        );
    }

    #[test]
    fn only_modulatable_fields_are_numbers() {
        for k in KINDS {
            for f in k.fields {
                if f.modulatable {
                    assert!(
                        matches!(f.kind, FieldKind::Number { .. }),
                        "{}.{}",
                        k.kind,
                        f.name
                    );
                }
            }
        }
    }
}
