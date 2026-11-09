//! Chord progressions for a jam's backing, as text: Roman numerals in the
//! jam's key, or chord names.
//!
//! ```text
//! I7 | IV7 | I7 | V7          one chord a bar
//! I IV V IV                   the same, without bar lines
//! | ii7 V7 | Imaj7 - |        two chords a bar; `-` holds the last
//! Am | F | C/E | G            chord names, a slash for the bass
//! ```
//!
//! With `|` anywhere, the chords between two bar lines share the bar
//! equally; without, each chord is a bar. A Roman numeral counts from the
//! tonic as a major scale does, whatever the mode: in A minor the minor
//! third is `bIII`. Upper case is a major chord and lower case a minor
//! one; a suffix says the rest: `7`, `maj7`, `m7`, `6`, `9`, `add9`,
//! `sus2`, `sus4`, `5`, `dim` (`°`), `dim7`, `m7b5` (`ø`), `aug` (`+`).

/// A chord: its root, the note in the bass, and its notes as semitones
/// above the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    /// Pitch class of the root, 0 for C.
    pub root: u8,
    /// Pitch class of the bass note: the root unless slashed.
    pub bass: u8,
    /// Semitones above the root, the root's 0 first, rising.
    pub intervals: Vec<u8>,
}

/// One bar: its chords, sharing it equally. `None` holds the chord
/// before it.
pub type Bar = Vec<Option<Chord>>;

/// The semitones of each Roman numeral's degree in a major scale.
const DEGREES: [(&str, u8); 7] = [
    ("vii", 11),
    ("iii", 4),
    ("vi", 9),
    ("iv", 5),
    ("ii", 2),
    ("v", 7),
    ("i", 0),
];

/// Reads a progression in the key whose tonic is `tonic` (a pitch class).
/// Every problem is reported, by the chord's place.
pub fn parse_progression(text: &str, tonic: u8) -> Result<Vec<Bar>, Vec<String>> {
    let mut problems = Vec::new();
    let barred = text.contains('|');
    let groups: Vec<&str> = if barred {
        text.split('|').filter(|g| !g.trim().is_empty()).collect()
    } else {
        text.split_whitespace().collect()
    };
    let mut bars = Vec::new();
    for group in groups {
        let mut bar = Bar::new();
        for token in group.split_whitespace() {
            if matches!(token, "-" | "%" | "/") {
                bar.push(None);
                continue;
            }
            match parse_chord(token, tonic) {
                Some(chord) => bar.push(Some(chord)),
                None => problems.push(format!("{token:?} is not a chord")),
            }
        }
        if !bar.is_empty() {
            bars.push(bar);
        }
    }
    if bars.is_empty() && problems.is_empty() {
        problems.push("the progression has no chords".into());
    }
    if bars
        .first()
        .and_then(|b| b.first())
        .is_some_and(|c| c.is_none())
    {
        problems.push("the progression starts by holding a chord it has not played".into());
    }
    if problems.is_empty() {
        Ok(bars)
    } else {
        Err(problems)
    }
}

/// One chord: a numeral or a name, a suffix and an optional `/bass`.
pub fn parse_chord(token: &str, tonic: u8) -> Option<Chord> {
    let (body, slash) = match token.split_once('/') {
        Some((b, s)) => (b, Some(s)),
        None => (token, None),
    };
    let (root, minor, suffix) = numeral(body, tonic).or_else(|| name(body))?;
    let intervals = quality(suffix, minor)?;
    let bass = match slash {
        None => root,
        Some(s) => numeral(s, tonic)
            .filter(|(_, _, rest)| rest.is_empty())
            .map(|(pc, _, _)| pc)
            .or_else(|| crate::jam::pitch_class(s))?,
    };
    Some(Chord {
        root,
        bass,
        intervals,
    })
}

/// A leading accidental: semitones and what is left.
fn accidental(text: &str) -> (i32, &str) {
    let mut shift = 0;
    let mut rest = text;
    loop {
        if let Some(r) = rest.strip_prefix(['b', '♭']) {
            shift -= 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix(['#', '♯']) {
            shift += 1;
            rest = r;
        } else {
            return (shift, rest);
        }
    }
}

/// A Roman numeral: its root, whether it is lower case, and the suffix.
fn numeral(text: &str, tonic: u8) -> Option<(u8, bool, &str)> {
    let (shift, rest) = accidental(text);
    let lower = rest.to_ascii_lowercase();
    let (numeral, semitones) = DEGREES.iter().find(|(n, _)| lower.starts_with(n))?;
    let written = &rest[..numeral.len()];
    let minor = written.chars().all(|c| c.is_ascii_lowercase());
    if !minor && !written.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let root = (i32::from(tonic) + i32::from(*semitones) + shift).rem_euclid(12) as u8;
    Some((root, minor, &rest[numeral.len()..]))
}

/// A chord name's root: a letter and accidentals; never minor by itself.
fn name(text: &str) -> Option<(u8, bool, &str)> {
    let letter = text.chars().next()?;
    if !letter.is_ascii_uppercase() {
        return None;
    }
    let end = text[1..]
        .char_indices()
        .find(|(_, c)| !matches!(c, '#' | '♯' | 'b' | '♭'))
        .map_or(text.len(), |(i, _)| i + 1);
    // `Bb` is B flat, but in `Cb5` and `Ebm` the `b` is still a flat:
    // names have no suffix starting with `b`.
    let root = crate::jam::pitch_class(&text[..end])?;
    Some((root, false, &text[end..]))
}

/// The intervals a suffix gives, on a major or minor triad.
fn quality(suffix: &str, minor: bool) -> Option<Vec<u8>> {
    let third = if minor { 3 } else { 4 };
    let (minor_named, rest) = match suffix {
        s if s.starts_with("maj") || s.starts_with('M') || s.starts_with('Δ') => (false, s),
        s => match s
            .strip_prefix("min")
            .or_else(|| s.strip_prefix('m'))
            .or_else(|| s.strip_prefix('-'))
        {
            Some(r) => (true, r),
            None => (false, s),
        },
    };
    let third = if minor_named { 3 } else { third };
    let v = match rest {
        "" => vec![0, third, 7],
        "5" => vec![0, 7],
        "6" => vec![0, third, 7, 9],
        "7" => vec![0, third, 7, 10],
        "9" => vec![0, third, 7, 10, 14],
        "add9" => vec![0, third, 7, 14],
        "maj7" | "M7" | "Δ" | "Δ7" => vec![0, third, 7, 11],
        "maj9" | "M9" => vec![0, third, 7, 11, 14],
        "sus2" => vec![0, 2, 7],
        "sus4" | "sus" => vec![0, 5, 7],
        "7sus4" => vec![0, 5, 7, 10],
        "dim" | "°" => vec![0, 3, 6],
        "dim7" | "°7" => vec![0, 3, 6, 9],
        "7b5" if minor_named || minor => vec![0, 3, 6, 10],
        "ø" | "ø7" => vec![0, 3, 6, 10],
        "aug" | "+" => vec![0, 4, 8],
        _ => return None,
    };
    Some(v)
}

/// A progression that ships with Cantina, for a genre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// What it is for: `Blues`, `Rock`…
    pub genre: &'static str,
    /// Its name in the list.
    pub name: &'static str,
    /// The progression, as a jam writes it.
    pub chords: &'static str,
}

/// The stock progressions, by genre.
pub const PRESETS: &[Preset] = &[
    Preset {
        genre: "Blues",
        name: "12-bar blues",
        chords: "I7 | I7 | I7 | I7 | IV7 | IV7 | I7 | I7 | V7 | IV7 | I7 | V7",
    },
    Preset {
        genre: "Blues",
        name: "Quick-change blues",
        chords: "I7 | IV7 | I7 | I7 | IV7 | IV7 | I7 | I7 | V7 | IV7 | I7 | V7",
    },
    Preset {
        genre: "Blues",
        name: "Minor blues",
        chords: "i7 | i7 | i7 | i7 | iv7 | iv7 | i7 | i7 | bVI7 | V7 | i7 | V7",
    },
    Preset {
        genre: "Rock",
        name: "I–bVII–IV",
        chords: "I | bVII | IV | I",
    },
    Preset {
        genre: "Rock",
        name: "I–IV–V",
        chords: "I | IV | V | IV",
    },
    Preset {
        genre: "Pop",
        name: "I–V–vi–IV",
        chords: "I | V | vi | IV",
    },
    Preset {
        genre: "Pop",
        name: "vi–IV–I–V",
        chords: "vi | IV | I | V",
    },
    Preset {
        genre: "Punk",
        name: "Power chords",
        chords: "I5 | IV5 | V5 | IV5",
    },
    Preset {
        genre: "Metal",
        name: "Aeolian",
        chords: "i5 | bVI5 | bVII5 | i5",
    },
    Preset {
        genre: "Metal",
        name: "Phrygian",
        chords: "i5 | i5 | bII5 | i5",
    },
    Preset {
        genre: "Grunge",
        name: "i–bIII–bVII–IV",
        chords: "i | bIII | bVII | IV",
    },
    Preset {
        genre: "Funk",
        name: "Dorian vamp",
        chords: "i7 | i7 | IV7 | IV7",
    },
    Preset {
        genre: "Funk",
        name: "One-chord groove",
        chords: "I9 | I9 | I9 | IV9",
    },
    Preset {
        genre: "Jazz",
        name: "ii–V–I",
        chords: "ii7 | V7 | Imaj7 | Imaj7",
    },
    Preset {
        genre: "Jazz",
        name: "Turnaround",
        chords: "Imaj7 | vi7 | ii7 | V7",
    },
    Preset {
        genre: "Reggae",
        name: "I–IV",
        chords: "I | IV | I | V",
    },
    Preset {
        genre: "Hip-hop",
        name: "Minor loop",
        chords: "i7 | bVImaj7 | iv7 | V7",
    },
    Preset {
        genre: "Country",
        name: "I–IV–I–V",
        chords: "I | IV | I | V",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    const A: u8 = 9;

    fn chord(token: &str) -> (u8, u8, Vec<u8>) {
        let c = parse_chord(token, A).unwrap_or_else(|| panic!("{token}"));
        (c.root, c.bass, c.intervals)
    }

    #[test]
    fn numerals_count_from_the_tonic() {
        assert_eq!(chord("I"), (9, 9, vec![0, 4, 7]));
        assert_eq!(chord("IV7"), (2, 2, vec![0, 4, 7, 10]));
        assert_eq!(chord("vi"), (6, 6, vec![0, 3, 7]));
        assert_eq!(chord("ii7"), (11, 11, vec![0, 3, 7, 10]));
        assert_eq!(chord("bVII"), (7, 7, vec![0, 4, 7]));
        assert_eq!(chord("bIII5"), (0, 0, vec![0, 7]));
        assert_eq!(chord("Imaj7"), (9, 9, vec![0, 4, 7, 11]));
        assert_eq!(chord("vii°"), (8, 8, vec![0, 3, 6]));
        assert_eq!(chord("iiø"), (11, 11, vec![0, 3, 6, 10]));
        assert_eq!(chord("V/vii"), (4, 8, vec![0, 4, 7]));
        assert!(parse_chord("Iv", A).is_none());
    }

    #[test]
    fn names_ignore_the_key() {
        assert_eq!(chord("C"), (0, 0, vec![0, 4, 7]));
        assert_eq!(chord("Am"), (9, 9, vec![0, 3, 7]));
        assert_eq!(chord("F#m7"), (6, 6, vec![0, 3, 7, 10]));
        assert_eq!(chord("Bb"), (10, 10, vec![0, 4, 7]));
        assert_eq!(chord("Ebmaj7"), (3, 3, vec![0, 4, 7, 11]));
        assert_eq!(chord("Dsus4"), (2, 2, vec![0, 5, 7]));
        assert_eq!(chord("E5"), (4, 4, vec![0, 7]));
        assert_eq!(chord("C/E"), (0, 4, vec![0, 4, 7]));
        assert_eq!(chord("G7"), (7, 7, vec![0, 4, 7, 10]));
        assert!(parse_chord("H", A).is_none());
        assert!(parse_chord("Cxyz", A).is_none());
    }

    #[test]
    fn bars_split_by_lines_or_by_chord() {
        let bars = parse_progression("I IV V IV", A).unwrap();
        assert_eq!(bars.len(), 4);
        assert!(bars.iter().all(|b| b.len() == 1));
        let bars = parse_progression("| ii7 V7 | Imaj7 - |", A).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].len(), 2);
        assert_eq!(bars[1][1], None);
    }

    #[test]
    fn problems_name_the_chord() {
        let p = parse_progression("I | Q7 | V | Zz", A).unwrap_err();
        assert_eq!(p, vec!["\"Q7\" is not a chord", "\"Zz\" is not a chord"]);
        assert!(parse_progression(" | ", A).unwrap_err()[0].contains("no chords"));
        assert!(parse_progression("- I", A).unwrap_err()[0].contains("starts by holding"));
    }

    #[test]
    fn every_preset_reads() {
        for p in PRESETS {
            for tonic in 0..12 {
                assert!(parse_progression(p.chords, tonic).is_ok(), "{}", p.name);
            }
        }
    }
}
