//! Tabs out of Cantina: ASCII tab text, MusicXML and Standard MIDI files.
//!
//! Pure functions from a [`Tab`] to the bytes of each format; the shell
//! writes them where the user chose. The printable PDF is drawn by the UI
//! from the tab view's page layout.
//!
//! - **ASCII** is the forum format: a line per string, highest first,
//!   frets on a grid of columns, techniques written as tab players do
//!   (`5h7`, `7p5`, `7/9`, `7b9r7`, `12~`, `<12>`, `x`, `t12`) and a `PM`
//!   line for palm mutes.
//! - **MusicXML** is one part with a TAB staff: the tuning and capo in the
//!   staff details, every note with its `<string>` and `<fret>`, and the
//!   techniques as MusicXML has them. It is one voice, so a note that
//!   rings past the next one starting is cut where it starts.
//! - **MIDI** is a type 0 file at 960 ticks a quarter, a channel per
//!   string so a bend moves only its own string, with the pitch-bend range
//!   set to four semitones.

use std::fmt::Write as _;

use crate::model::PPQ;
use crate::tab::{Instrument, Tab, TabNote};

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The pitch a natural harmonic over `fret` sounds above the open
/// string, in semitones.
pub fn harmonic_interval(fret: u8) -> u8 {
    match fret {
        12 => 12,
        7 | 19 => 19,
        5 | 24 => 24,
        4 | 9 | 16 => 28,
        3 => 31,
        _ => fret,
    }
}

/// The MIDI note a note sounds: the fretted pitch, a harmonic's own
/// pitch, or for a dead note the pitch it is muted on.
fn sounding(tab: &Tab, note: &TabNote) -> u8 {
    let open = tab.open_string(note.string).unwrap_or(40);
    let above = if note.harmonic {
        harmonic_interval(note.fret)
    } else {
        note.fret
    };
    open.saturating_add(tab.capo).saturating_add(above).min(127)
}

/// The next note on the same string after `index`, in time.
fn next_on_string(tab: &Tab, index: usize) -> Option<&TabNote> {
    let note = &tab.notes[index];
    tab.notes
        .iter()
        .filter(|n| n.string == note.string && n.start > note.start)
        .min_by_key(|n| n.start)
}

/// The previous note on the same string before `index`, in time.
fn previous_on_string(tab: &Tab, index: usize) -> Option<&TabNote> {
    let note = &tab.notes[index];
    tab.notes
        .iter()
        .filter(|n| n.string == note.string && n.start < note.start)
        .max_by_key(|n| n.start)
}

/// The string names, highest string first, the top string of a guitar in
/// lower case as tab writes it.
fn string_names(tab: &Tab) -> Vec<String> {
    let lowest = tab.tuning.iter().copied().min().unwrap_or(0);
    tab.tuning
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &midi)| {
            let name = NAMES[usize::from((midi + tab.capo) % 12)];
            if i == 0 && tab.tuning.len() >= 6 && midi.saturating_sub(lowest) >= 24 {
                name.to_lowercase()
            } else {
                name.to_string()
            }
        })
        .collect()
}

// ----- ASCII -----

/// Widest line of ASCII tab before bars wrap to the next system.
pub const ASCII_WIDTH: usize = 80;

/// How a note is written in ASCII tab, without what joins it to the next.
fn ascii_label(tab: &Tab, index: usize) -> String {
    let note = &tab.notes[index];
    let mut label = String::new();
    if note.hammer_on {
        label.push('h');
    } else if note.pull_off {
        label.push('p');
    }
    if note.tap {
        label.push('t');
    }
    if note.dead {
        label.push('x');
    } else if note.harmonic {
        let _ = write!(label, "<{}>", note.fret);
    } else {
        let _ = write!(label, "{}", note.fret);
    }
    if let Some(bend) = note.bend {
        let to = u32::from(note.fret) + bend.semitones.round().max(1.0) as u32;
        let _ = write!(label, "b{to}");
        if bend.release {
            let _ = write!(label, "r{}", note.fret);
        }
    }
    if note.vibrato {
        label.push('~');
    }
    if note.slide {
        if let Some(next) = next_on_string(tab, index) {
            label.push(if next.fret >= note.fret { '/' } else { '\\' });
        }
    }
    label
}

/// The grid step for ASCII: the coarsest of sixteenths, thirty-seconds
/// and sixteenth triplets that every note starts on, else 80 ticks.
fn ascii_step(tab: &Tab) -> u64 {
    [240, 120, 160, 80]
        .into_iter()
        .find(|step| tab.notes.iter().all(|n| n.start % step == 0))
        .unwrap_or(80)
}

/// The tab as plain-text ASCII tab, wrapped at [`ASCII_WIDTH`]
/// characters, for a forum post or a text file.
pub fn ascii(tab: &Tab) -> String {
    let mut out = String::new();
    if !tab.name.is_empty() {
        let _ = writeln!(out, "{}", tab.name);
    }
    let names = string_names(tab);
    let tuning: Vec<String> = tab
        .tuning
        .iter()
        .map(|&m| NAMES[usize::from(m % 12)].to_string())
        .collect();
    let _ = write!(out, "Tuning: {}", tuning.join(" "));
    if tab.capo > 0 {
        let _ = write!(out, ", capo {}", tab.capo);
    }
    let _ = writeln!(
        out,
        ". {} bpm, {}/{}.",
        tab.tempo.bpm.round(),
        tab.tempo.numerator,
        tab.tempo.denominator
    );
    out.push('\n');

    let strings = tab.strings();
    let step = ascii_step(tab);
    let bar = tab.bar_ticks().max(1);
    let bars = tab.end().div_ceil(bar).max(1);
    let columns = (bar / step).max(1) as usize;
    let snap = |tick: u64| ((tick + step / 2) / step) as usize;
    let palm_muted = tab.notes.iter().any(|n| n.palm_mute);
    let let_ring = tab.notes.iter().any(|n| n.let_ring);

    // Each bar as rows of text: one per string, plus the PM and let
    // ring lines over them when any note needs them.
    let mut rendered: Vec<Vec<String>> = Vec::new();
    for b in 0..bars {
        let mut cells: Vec<Vec<String>> = vec![vec![String::new(); columns]; strings];
        let mut pm = vec![false; columns];
        let mut ring = vec![false; columns];
        for (i, note) in tab.notes.iter().enumerate() {
            let col = snap(note.start);
            let first = (b * bar / step) as usize;
            if col < first || col >= first + columns {
                continue;
            }
            let c = col - first;
            let row = usize::from(note.string.max(1)) - 1;
            if row < strings {
                cells[row][c] = ascii_label(tab, i);
            }
            let span = (note.length / step).max(1) as usize;
            for k in c..(c + span).min(columns) {
                pm[k] |= note.palm_mute;
                ring[k] |= note.let_ring;
            }
        }
        let widths: Vec<usize> = (0..columns)
            .map(|c| cells.iter().map(|r| r[c].len()).max().unwrap_or(0).max(1) + 1)
            .collect();
        let mut rows = Vec::new();
        // A mark line: the name where a run starts, dashes while it
        // lasts, written over a blank line so a long name never pushes
        // the columns after it out of line.
        let total: usize = widths.iter().sum::<usize>() + 1;
        let starts: Vec<usize> = widths
            .iter()
            .scan(0, |x, w| {
                let at = *x;
                *x += w;
                Some(at)
            })
            .collect();
        let marks = |on: &[bool], name: &str| {
            let mut line = vec![' '; total];
            let mut written_to = 0;
            for c in 0..columns {
                if !on[c] {
                    continue;
                }
                let at = starts[c].max(written_to);
                let text = if c == 0 || !on[c - 1] { name } else { "-" };
                for (k, ch) in text.chars().enumerate() {
                    if at + k < total {
                        line[at + k] = ch;
                    }
                }
                written_to = at + text.len() + 1;
            }
            line.into_iter().collect::<String>()
        };
        if palm_muted {
            rows.push(marks(&pm, "PM"));
        }
        if let_ring {
            rows.push(marks(&ring, "let ring"));
        }
        for row in &cells {
            let mut line = String::new();
            for (c, w) in widths.iter().enumerate() {
                let _ = write!(line, "-{:-<width$}", row[c], width = w - 1);
            }
            line.push('|');
            rows.push(line);
        }
        rendered.push(rows);
    }

    let name_width = names.iter().map(String::len).max().unwrap_or(1);
    let above = usize::from(palm_muted) + usize::from(let_ring);
    let mut b = 0;
    while b < rendered.len() {
        // As many bars as fit the width, and at least one.
        let mut end = b + 1;
        let mut width = name_width + 1 + rendered[b][above].len();
        while end < rendered.len() && width + rendered[end][above].len() <= ASCII_WIDTH {
            width += rendered[end][above].len();
            end += 1;
        }
        for line in 0..above {
            let _ = write!(out, "{:width$} ", "", width = name_width);
            for bar in &rendered[b..end] {
                out.push_str(&bar[line]);
            }
            out.truncate(out.trim_end().len());
            out.push('\n');
        }
        for (s, name) in names.iter().enumerate() {
            let _ = write!(out, "{name:<name_width$}|");
            for bar in &rendered[b..end] {
                out.push_str(&bar[above + s]);
            }
            out.push('\n');
        }
        out.push('\n');
        b = end;
    }
    out.push_str(
        "h hammer-on, p pull-off, / \\ slide, b bend, r release, ~ vibrato,\n\
         <> harmonic, x dead note, t tap, PM palm mute\n",
    );
    out
}

// ----- MusicXML -----

/// MusicXML divisions per quarter note: one is 40 ticks, so sixteenth
/// triplets (160) and thirty-seconds (120) are whole numbers.
const DIVISIONS: u64 = 24;
const TICKS_PER_DIVISION: u64 = PPQ as u64 / DIVISIONS;

/// A written value: its length in ticks, its type and whether it is dotted.
type Value = (u64, &'static str, bool);

/// Plain values, longest first.
const PLAIN: [Value; 9] = [
    (3840, "whole", false),
    (2880, "half", true),
    (1920, "half", false),
    (1440, "quarter", true),
    (960, "quarter", false),
    (720, "eighth", true),
    (480, "eighth", false),
    (360, "16th", true),
    (240, "16th", false),
];

/// Values inside a triplet quarter (three eighths in the time of two),
/// at their sounding length.
const TRIPLET: [Value; 3] = [
    (640, "quarter", false),
    (320, "eighth", false),
    (160, "16th", false),
];

/// Thirty-seconds, for a plain group that needs them.
const THIRTY_SECOND: Value = (120, "32nd", false);

/// A length cut into as few `values` as add up to it exactly, longest
/// first; `None` when they cannot.
fn split(ticks: u64, values: &[Value]) -> Option<Vec<Value>> {
    let unit = 40;
    let n = (ticks / unit) as usize;
    let mut best: Vec<Option<(u32, usize)>> = vec![None; n + 1];
    best[0] = Some((0, usize::MAX));
    for len in 1..=n {
        for (v, value) in values.iter().enumerate() {
            let size = (value.0 / unit) as usize;
            if size <= len {
                if let Some((parts, _)) = best[len - size] {
                    if best[len].is_none_or(|b| parts + 1 < b.0) {
                        best[len] = Some((parts + 1, v));
                    }
                }
            }
        }
    }
    best[n]?;
    let mut len = n;
    let mut parts = Vec::new();
    while len > 0 {
        let (_, v) = best[len]?;
        parts.push(values[v]);
        len -= (values[v].0 / unit) as usize;
    }
    parts.sort_by_key(|v| std::cmp::Reverse(v.0));
    Some(parts)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The notes that start together, and until when they are written.
struct Onset {
    start: u64,
    end: u64,
    notes: Vec<usize>,
}

/// One written piece of the voice: a note or rest value inside a group.
struct Piece {
    value: Value,
    /// The onset it belongs to, or none for a rest.
    onset: Option<usize>,
    /// Tied from the piece before / to the piece after.
    tie_stop: bool,
    tie_start: bool,
    /// In a triplet group, and where its bracket starts and stops.
    triplet: bool,
    tuplet_start: bool,
    tuplet_stop: bool,
}

/// The tab as a MusicXML 4.0 score: one part on a TAB staff.
///
/// The voice is written a quarter note at a time. A quarter whose notes
/// fall on a triplet grid is written as one whole triplet (three eighths
/// in the time of two), every other quarter on a grid of thirty-seconds,
/// so there is never an incomplete tuplet. Notes are moved to the
/// nearest line of their quarter's grid, and a note that rings past the
/// next onset is cut there.
pub fn musicxml(tab: &Tab) -> String {
    const GROUP: u64 = PPQ as u64;
    let bar = tab.bar_ticks().max(1);
    let group_of = |t: u64| (t / bar, (t % bar) / GROUP);
    // Which quarters are triplets: a note starts or ends on a triplet
    // line there that is not on the thirty-second grid.
    let mut triplets = std::collections::HashSet::new();
    for note in &tab.notes {
        for t in [note.start, note.end()] {
            let within = (t % bar) % GROUP;
            let full = (t % bar) / GROUP * GROUP + GROUP <= bar;
            if full && within % 120 != 0 && within % 160 == 0 {
                triplets.insert(group_of(t));
            }
        }
    }
    let snap = |t: u64| {
        let step = if triplets.contains(&group_of(t)) {
            160
        } else {
            120
        };
        (t + step / 2) / step * step
    };
    let mut starts: Vec<u64> = tab.notes.iter().map(|n| snap(n.start)).collect();
    starts.sort_unstable();
    starts.dedup();
    let onsets: Vec<Onset> = starts
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let notes: Vec<usize> = (0..tab.notes.len())
                .filter(|&i| snap(tab.notes[i].start) == start)
                .collect();
            let end = notes
                .iter()
                .map(|&i| snap(tab.notes[i].end()))
                .max()
                .unwrap_or(start)
                .max(start + 120);
            let end = starts.get(k + 1).map_or(end, |&next| end.min(next));
            Onset { start, end, notes }
        })
        .collect();
    let last = onsets.iter().map(|o| o.end).max().unwrap_or(bar);
    let bars = last.div_ceil(bar).max(1);

    // What sounds at each moment: the onset, or a rest.
    let sounding_at = |t: u64| onsets.iter().position(|o| o.start <= t && t < o.end);

    let mut measures: Vec<Vec<Piece>> = Vec::new();
    for b in 0..bars {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut g = 0;
        while g * GROUP < bar {
            let from = b * bar + g * GROUP;
            let to = (from + GROUP).min((b + 1) * bar);
            let triplet = triplets.contains(&(b, g));
            // A plain run from the start of a quarter that lasts whole
            // plain quarters is written as one long value.
            if !triplet {
                if let Some(current) = sounding_at(from) {
                    let run_end = onsets[current].end.min((b + 1) * bar);
                    let started_here = onsets[current].start == from;
                    let mut whole = 0;
                    while from + (whole + 1) * GROUP <= run_end
                        && !triplets.contains(&(b, g + whole))
                    {
                        whole += 1;
                    }
                    if whole >= 2 {
                        let length = PLAIN
                            .iter()
                            .map(|v| v.0)
                            .find(|&l| l <= whole * GROUP && l % GROUP == 0)
                            .unwrap_or(GROUP);
                        let value = *PLAIN.iter().find(|v| v.0 == length).unwrap_or(&PLAIN[4]);
                        pieces.push(Piece {
                            value,
                            onset: Some(current),
                            tie_stop: !started_here,
                            tie_start: onsets[current].end > from + length,
                            triplet: false,
                            tuplet_start: false,
                            tuplet_stop: false,
                        });
                        g += length / GROUP;
                        continue;
                    }
                } else {
                    // Rests over whole quarters merge the same way.
                    let mut whole = 0;
                    while from + (whole + 1) * GROUP <= (b + 1) * bar
                        && !triplets.contains(&(b, g + whole))
                        && (0..GROUP)
                            .step_by(40)
                            .all(|k| sounding_at(from + whole * GROUP + k).is_none())
                    {
                        whole += 1;
                    }
                    if whole >= 2 {
                        let length = PLAIN
                            .iter()
                            .map(|v| v.0)
                            .find(|&l| l <= whole * GROUP && l % GROUP == 0)
                            .unwrap_or(GROUP);
                        let value = *PLAIN.iter().find(|v| v.0 == length).unwrap_or(&PLAIN[4]);
                        pieces.push(Piece {
                            value,
                            onset: None,
                            tie_stop: false,
                            tie_start: false,
                            triplet: false,
                            tuplet_start: false,
                            tuplet_stop: false,
                        });
                        g += length / GROUP;
                        continue;
                    }
                }
            }
            // The quarter on its own: the runs of notes and rests in it.
            let step = if triplet { 160 } else { 120 };
            let mut runs: Vec<(u64, u64, Option<usize>)> = Vec::new();
            let mut t = from;
            while t < to {
                let here = sounding_at(t);
                let mut e = t + step;
                while e < to && sounding_at(e) == here && here.is_none_or(|o| onsets[o].start != e)
                {
                    e += step;
                }
                let e = e.min(to);
                runs.push((t, e, here));
                t = e;
            }
            let first_piece = pieces.len();
            for (s, e, onset) in runs {
                let values: Vec<Value> = if triplet {
                    split(e - s, &TRIPLET)
                        .unwrap_or_else(|| vec![TRIPLET[2]; ((e - s) / 160) as usize])
                } else {
                    let mut all = PLAIN.to_vec();
                    all.push(THIRTY_SECOND);
                    split(e - s, &all).unwrap_or_default()
                };
                let count = values.len();
                for (k, value) in values.into_iter().enumerate() {
                    let (tie_stop, tie_start) = match onset {
                        Some(o) => (
                            k > 0 || onsets[o].start < s,
                            k + 1 < count || onsets[o].end > e,
                        ),
                        None => (false, false),
                    };
                    pieces.push(Piece {
                        value,
                        onset,
                        tie_stop,
                        tie_start,
                        triplet,
                        tuplet_start: false,
                        tuplet_stop: false,
                    });
                }
            }
            if triplet && pieces.len() > first_piece {
                pieces[first_piece].tuplet_start = true;
                let end = pieces.len() - 1;
                pieces[end].tuplet_stop = true;
            }
            g += 1;
        }
        measures.push(pieces);
    }

    let (instrument, program) = match tab.instrument {
        Instrument::Guitar => ("Guitar", 26),
        Instrument::Bass => ("Bass", 34),
    };
    let mut x = String::new();
    x.push_str(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n",
        "<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" ",
        "\"http://www.musicxml.org/dtds/partwise.dtd\">\n",
        "<score-partwise version=\"4.0\">\n"
    ));
    let title = if tab.name.is_empty() {
        "Tab"
    } else {
        tab.name.as_str()
    };
    let _ = writeln!(
        x,
        "  <work><work-title>{}</work-title></work>",
        escape(title)
    );
    x.push_str(
        "  <identification><encoding><software>Cantina</software></encoding></identification>\n",
    );
    let _ = writeln!(
        x,
        "  <part-list>\n    <score-part id=\"P1\">\n      <part-name>{instrument}</part-name>\n      \
         <score-instrument id=\"P1-I1\"><instrument-name>{instrument}</instrument-name></score-instrument>\n      \
         <midi-instrument id=\"P1-I1\"><midi-channel>1</midi-channel><midi-program>{program}</midi-program></midi-instrument>\n    \
         </score-part>\n  </part-list>\n  <part id=\"P1\">"
    );
    let mut pm_on = false;
    let mut ring_on = false;
    for (b, pieces) in measures.iter().enumerate() {
        let _ = writeln!(x, "    <measure number=\"{}\">", b + 1);
        if b == 0 {
            write_attributes(&mut x, tab);
        }
        for piece in pieces {
            let Some(o) = piece.onset else {
                write_rest(&mut x, piece);
                continue;
            };
            let onset = &onsets[o];
            if !piece.tie_stop {
                // Palm mute and let ring as words where they start.
                let pm = onset.notes.iter().any(|&i| tab.notes[i].palm_mute);
                let ring = onset.notes.iter().any(|&i| tab.notes[i].let_ring);
                if pm && !pm_on {
                    x.push_str("      <direction placement=\"above\"><direction-type><words>P.M.</words></direction-type></direction>\n");
                }
                if ring && !ring_on {
                    x.push_str("      <direction placement=\"above\"><direction-type><words>let ring</words></direction-type></direction>\n");
                }
                pm_on = pm;
                ring_on = ring;
            }
            for (k, &i) in onset.notes.iter().enumerate() {
                write_note(&mut x, tab, i, piece, k > 0);
            }
        }
        if b + 1 == measures.len() {
            x.push_str(
                "      <barline location=\"right\"><bar-style>light-heavy</bar-style></barline>\n",
            );
        }
        x.push_str("    </measure>\n");
    }
    x.push_str("  </part>\n</score-partwise>\n");
    x
}

/// The first measure's attributes: the TAB staff with its tuning and
/// capo, the time signature, and the tempo.
fn write_attributes(x: &mut String, tab: &Tab) {
    let _ = writeln!(
        x,
        "      <attributes>\n        <divisions>{DIVISIONS}</divisions>\n        \
         <key><fifths>0</fifths></key>\n        \
         <time><beats>{}</beats><beat-type>{}</beat-type></time>\n        \
         <clef><sign>TAB</sign><line>5</line></clef>\n        \
         <staff-details>\n          <staff-lines>{}</staff-lines>",
        tab.tempo.numerator,
        tab.tempo.denominator,
        tab.strings()
    );
    for (line, &midi) in tab.tuning.iter().enumerate() {
        let name = NAMES[usize::from(midi % 12)];
        let _ = writeln!(
            x,
            "          <staff-tuning line=\"{}\"><tuning-step>{}</tuning-step>{}<tuning-octave>{}</tuning-octave></staff-tuning>",
            line + 1,
            &name[..1],
            if name.len() > 1 { "<tuning-alter>1</tuning-alter>" } else { "" },
            i32::from(midi) / 12 - 1
        );
    }
    if tab.capo > 0 {
        let _ = writeln!(x, "          <capo>{}</capo>", tab.capo);
    }
    x.push_str("        </staff-details>\n      </attributes>\n");
    let _ = writeln!(
        x,
        "      <direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit>\
         <per-minute>{}</per-minute></metronome></direction-type><sound tempo=\"{}\"/></direction>",
        tab.tempo.bpm.round(),
        tab.tempo.bpm
    );
}

/// A piece's written length in divisions: a triplet value sounds two
/// thirds of its plain length, which is what its ticks already are.
fn divisions(piece: &Piece) -> u64 {
    piece.value.0 / TICKS_PER_DIVISION
}

/// The type, dot, triplet ratio and bracket of a piece, after its
/// duration and ties.
fn write_value(x: &mut String, piece: &Piece) {
    let _ = write!(x, "<voice>1</voice><type>{}</type>", piece.value.1);
    if piece.value.2 {
        x.push_str("<dot/>");
    }
    if piece.triplet {
        x.push_str("<time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>");
    }
}

fn write_tuplet(x: &mut String, piece: &Piece) {
    if piece.tuplet_start {
        x.push_str("<tuplet type=\"start\" bracket=\"yes\"/>");
    }
    if piece.tuplet_stop {
        x.push_str("<tuplet type=\"stop\"/>");
    }
}

fn write_rest(x: &mut String, piece: &Piece) {
    let _ = write!(
        x,
        "      <note><rest/><duration>{}</duration>",
        divisions(piece)
    );
    write_value(x, piece);
    if piece.tuplet_start || piece.tuplet_stop {
        x.push_str("<notations>");
        write_tuplet(x, piece);
        x.push_str("</notations>");
    }
    x.push_str("</note>\n");
}

fn write_note(x: &mut String, tab: &Tab, index: usize, piece: &Piece, chord: bool) {
    let note = &tab.notes[index];
    let midi = sounding(tab, note);
    let name = NAMES[usize::from(midi % 12)];
    // The note's techniques go on its first piece.
    let first_part = !piece.tie_stop;
    x.push_str("      <note>");
    if chord {
        x.push_str("<chord/>");
    }
    let _ = write!(
        x,
        "<pitch><step>{}</step>{}<octave>{}</octave></pitch><duration>{}</duration>",
        &name[..1],
        if name.len() > 1 {
            "<alter>1</alter>"
        } else {
            ""
        },
        i32::from(midi) / 12 - 1,
        divisions(piece)
    );
    if piece.tie_stop {
        x.push_str("<tie type=\"stop\"/>");
    }
    if piece.tie_start {
        x.push_str("<tie type=\"start\"/>");
    }
    write_value(x, piece);
    if note.dead {
        x.push_str("<notehead>x</notehead>");
    }
    x.push_str("<notations>");
    if !chord {
        write_tuplet(x, piece);
    }
    if piece.tie_stop {
        x.push_str("<tied type=\"stop\"/>");
    }
    if piece.tie_start {
        x.push_str("<tied type=\"start\"/>");
    }
    if first_part {
        let previous = previous_on_string(tab, index);
        let next = next_on_string(tab, index);
        if note.slide && next.is_some() {
            x.push_str("<slide type=\"start\" line-type=\"solid\" number=\"1\"/>");
        }
        if previous.is_some_and(|p| p.slide) {
            x.push_str("<slide type=\"stop\" line-type=\"solid\" number=\"1\"/>");
        }
        // Hammer-ons and pull-offs are slurred too, as MusicXML asks,
        // which is how most notation apps show them.
        if note.hammer_on || note.pull_off {
            x.push_str("<slur type=\"stop\" number=\"1\"/>");
        }
        if next.is_some_and(|n| n.hammer_on || n.pull_off) {
            x.push_str("<slur type=\"start\" number=\"1\"/>");
        }
        if note.vibrato {
            x.push_str("<ornaments><wavy-line type=\"start\" number=\"1\"/><wavy-line type=\"stop\" number=\"1\"/></ornaments>");
        }
    }
    x.push_str("<technical>");
    if first_part {
        let next = next_on_string(tab, index);
        if note.hammer_on {
            x.push_str("<hammer-on type=\"stop\" number=\"1\"/>");
        }
        if note.pull_off {
            x.push_str("<pull-off type=\"stop\" number=\"1\"/>");
        }
        if let Some(next) = next {
            if next.hammer_on {
                x.push_str("<hammer-on type=\"start\" number=\"1\">H</hammer-on>");
            } else if next.pull_off {
                x.push_str("<pull-off type=\"start\" number=\"1\">P</pull-off>");
            }
        }
        if note.harmonic {
            x.push_str("<harmonic><natural/></harmonic>");
        }
        if note.tap {
            x.push_str("<tap/>");
        }
        if let Some(bend) = note.bend {
            let _ = write!(
                x,
                "<bend><bend-alter>{}</bend-alter></bend>",
                bend.semitones
            );
            if bend.release {
                let _ = write!(
                    x,
                    "<bend><bend-alter>{}</bend-alter><release/></bend>",
                    -bend.semitones
                );
            }
        }
    }
    let _ = writeln!(
        x,
        "<string>{}</string><fret>{}</fret></technical></notations></note>",
        note.string, note.fret
    );
}

// ----- MIDI -----

/// The pitch-bend range the file sets, in semitones: room for a bend of
/// two whole steps.
const BEND_RANGE: f32 = 4.0;

fn vlq(out: &mut Vec<u8>, mut value: u64) {
    let mut bytes = [0u8; 10];
    let mut n = 0;
    loop {
        bytes[n] = (value & 0x7f) as u8;
        value >>= 7;
        n += 1;
        if value == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(bytes[i] | if i > 0 { 0x80 } else { 0 });
    }
}

/// The tab as a Standard MIDI File, type 0, 960 ticks a quarter: a
/// channel per string (string 1 on channel 1), the tab's tempo and time
/// signature, and bends as pitch bend on the note's own channel.
pub fn midi(tab: &Tab) -> Vec<u8> {
    // (tick, order, bytes): at one tick, controllers, then note-offs,
    // then bends, then note-ons.
    let mut events: Vec<(u64, u8, Vec<u8>)> = Vec::new();
    let tempo = (60_000_000.0 / tab.tempo.bpm).round() as u32;
    events.push((
        0,
        0,
        vec![
            0xff,
            0x51,
            3,
            (tempo >> 16) as u8,
            (tempo >> 8) as u8,
            tempo as u8,
        ],
    ));
    let denominator = u8::try_from(tab.tempo.denominator.max(1).trailing_zeros()).unwrap_or(2);
    events.push((
        0,
        0,
        vec![0xff, 0x58, 4, tab.tempo.numerator, denominator, 24, 8],
    ));
    let name = if tab.name.is_empty() {
        "Tab"
    } else {
        tab.name.as_str()
    };
    let mut meta = vec![0xff, 0x03];
    vlq(&mut meta, name.len() as u64);
    meta.extend_from_slice(name.as_bytes());
    events.push((0, 0, meta));
    let program = match tab.instrument {
        Instrument::Guitar => 25,
        Instrument::Bass => 33,
    };
    for string in 1..=tab.strings() as u8 {
        let ch = string - 1;
        events.push((0, 0, vec![0xc0 | ch, program]));
        // Pitch-bend sensitivity through RPN 0.
        events.push((0, 0, vec![0xb0 | ch, 101, 0]));
        events.push((0, 0, vec![0xb0 | ch, 100, 0]));
        events.push((0, 0, vec![0xb0 | ch, 6, BEND_RANGE as u8]));
        events.push((0, 0, vec![0xb0 | ch, 38, 0]));
    }
    let bend_value = |semitones: f32| -> u16 {
        (8192.0 + semitones / BEND_RANGE * 8191.0)
            .round()
            .clamp(0.0, 16383.0) as u16
    };
    let bend_event = |ch: u8, value: u16| vec![0xe0 | ch, (value & 0x7f) as u8, (value >> 7) as u8];
    for note in &tab.notes {
        let ch = note.string.saturating_sub(1).min(15);
        let pitch = sounding(tab, note);
        let velocity = if note.dead {
            40
        } else {
            (note.velocity * 127.0).round().clamp(1.0, 127.0) as u8
        };
        let length = if note.dead {
            note.length.min(u64::from(PPQ) / 8)
        } else {
            note.length
        }
        .max(1);
        let end = note.start + length;
        events.push((note.start, 3, vec![0x90 | ch, pitch, velocity]));
        events.push((end, 1, vec![0x80 | ch, pitch, 0]));
        if let Some(bend) = note.bend {
            // Up over the first eighth of a beat-and-a-bit, in steps.
            let rise = (length / 4).clamp(1, u64::from(PPQ) / 4);
            for k in 1..=4u64 {
                let amount = bend.semitones * k as f32 / 4.0;
                events.push((
                    note.start + rise * k / 4,
                    2,
                    bend_event(ch, bend_value(amount)),
                ));
            }
            if bend.release {
                let back = note.start + length * 6 / 10;
                for k in 1..=4u64 {
                    let amount = bend.semitones * (4 - k) as f32 / 4.0;
                    events.push((back + rise * k / 4, 2, bend_event(ch, bend_value(amount))));
                }
            }
            events.push((end, 1, bend_event(ch, 8192)));
        }
    }
    events.sort_by_key(|e| (e.0, e.1));

    let mut track = Vec::new();
    let mut last = 0;
    for (tick, _, bytes) in &events {
        vlq(&mut track, tick - last);
        track.extend_from_slice(bytes);
        last = *tick;
    }
    track.extend_from_slice(&[0, 0xff, 0x2f, 0]);

    let mut file = Vec::with_capacity(track.len() + 22);
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6u32.to_be_bytes());
    file.extend_from_slice(&0u16.to_be_bytes());
    file.extend_from_slice(&1u16.to_be_bytes());
    file.extend_from_slice(&(PPQ as u16).to_be_bytes());
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab::Bend;

    /// A riff with every technique: a palm-muted chug, a power chord, a
    /// hammer and pull, a slide, a bend and release, a harmonic, dead
    /// notes, vibrato and a tap.
    pub(crate) fn riff() -> Tab {
        let mut tab = Tab::new("Riff", Instrument::Guitar, 6).unwrap();
        let n = TabNote::new;
        tab.notes = vec![
            TabNote {
                palm_mute: true,
                ..n(6, 0, 0, 240)
            },
            TabNote {
                palm_mute: true,
                ..n(6, 0, 240, 240)
            },
            n(6, 3, 480, 480),
            n(5, 5, 480, 480),
            n(3, 5, 960, 240),
            TabNote {
                hammer_on: true,
                ..n(3, 7, 1200, 240)
            },
            TabNote {
                pull_off: true,
                ..n(3, 5, 1440, 240)
            },
            TabNote {
                slide: true,
                ..n(4, 7, 1680, 240)
            },
            n(4, 9, 1920, 480),
            TabNote {
                bend: Some(Bend {
                    semitones: 2.0,
                    release: true,
                }),
                ..n(2, 8, 2400, 480)
            },
            TabNote {
                harmonic: true,
                let_ring: true,
                ..n(1, 12, 2880, 960)
            },
            TabNote {
                dead: true,
                ..n(3, 0, 3840, 240)
            },
            TabNote {
                vibrato: true,
                ..n(3, 7, 4080, 960)
            },
            TabNote {
                tap: true,
                ..n(1, 15, 5040, 320)
            },
        ];
        tab.validate().unwrap();
        tab
    }

    #[test]
    fn ascii_tab_writes_every_technique_on_its_string() {
        let text = ascii(&riff());
        assert!(
            text.starts_with("Riff\nTuning: E A D G B E. 120 bpm, 4/4.\n"),
            "{text}"
        );
        let lines: Vec<&str> = text.lines().collect();
        let line = |name: &str| *lines.iter().find(|l| l.starts_with(name)).unwrap();
        assert!(line("E|").contains("0-0-3"), "{}", line("E|"));
        assert!(line("A|").contains("5"));
        assert!(line("G|").contains("5-h7-p5"), "{}", line("G|"));
        assert!(line("D|").contains("7/-9"), "{}", line("D|"));
        assert!(line("B|").contains("8b10r8"), "{}", line("B|"));
        assert!(line("e|").contains("<12>"));
        assert!(text.contains("-t15-"));
        assert!(text.contains("-x-7~-"));
        assert!(text.contains("PM"));
        assert!(text.contains("let ring"));
        // Plain text: ASCII only, and every line within the width.
        assert!(text.is_ascii());
        assert!(text.lines().all(|l| l.len() <= ASCII_WIDTH), "{text}");
        // Strings of a system line up.
        let system: Vec<usize> = lines
            .iter()
            .filter(|l| l.contains('|'))
            .map(|l| l.len())
            .collect();
        assert!(system.windows(2).take(5).all(|w| w[0] == w[1]), "{text}");
    }

    #[test]
    fn long_tabs_wrap_into_systems() {
        let mut tab = riff();
        let more: Vec<TabNote> = (0..32)
            .map(|k| TabNote::new(1, (k % 12) as u8, 7680 + k * 240, 240))
            .collect();
        tab.notes.extend(more);
        let text = ascii(&tab);
        assert!(text.lines().all(|l| l.len() <= ASCII_WIDTH));
        assert!(
            text.matches("\ne|").count() + usize::from(text.starts_with("e|")) >= 2,
            "{text}"
        );
    }

    #[test]
    fn musicxml_has_a_tab_staff_strings_frets_and_techniques() {
        let tab = riff();
        let xml = musicxml(&tab);
        assert!(xml.contains("<sign>TAB</sign>"));
        assert!(xml.contains("<staff-lines>6</staff-lines>"));
        assert!(xml.contains(
            "<staff-tuning line=\"1\"><tuning-step>E</tuning-step><tuning-octave>2</tuning-octave>"
        ));
        assert!(xml.contains(
            "<staff-tuning line=\"6\"><tuning-step>E</tuning-step><tuning-octave>4</tuning-octave>"
        ));
        // Every note's string and fret, once (none of these cross a bar).
        for note in &tab.notes {
            let tag = format!("<string>{}</string><fret>{}</fret>", note.string, note.fret);
            assert!(xml.contains(&tag), "{tag}");
        }
        for tag in [
            "<hammer-on type=\"start\"",
            "<pull-off type=\"start\"",
            "<slide type=\"start\"",
            "<bend><bend-alter>2</bend-alter></bend>",
            "<release/>",
            "<harmonic><natural/></harmonic>",
            "<notehead>x</notehead>",
            "<wavy-line type=\"start\"",
            "<tap/>",
            "<words>P.M.</words>",
            "<words>let ring</words>",
        ] {
            assert!(xml.contains(tag), "{tag}");
        }
        // Each measure's durations add up to a bar: 4 quarters of 24.
        for measure in xml.split("<measure").skip(1) {
            let total: u64 = measure
                .split("<note>")
                .skip(1)
                .filter(|n| !n.contains("<chord/>"))
                .map(|n| {
                    let d = n.split("<duration>").nth(1).unwrap();
                    d[..d.find('<').unwrap()].parse::<u64>().unwrap()
                })
                .sum();
            assert_eq!(total, 96, "{measure}");
        }
    }

    #[test]
    fn a_note_over_the_bar_line_is_tied() {
        let mut tab = Tab::new("Tie", Instrument::Bass, 4).unwrap();
        tab.notes = vec![TabNote::new(4, 3, 2880, 1920)];
        let xml = musicxml(&tab);
        assert_eq!(xml.matches("<tie type=\"start\"/>").count(), 1);
        assert_eq!(xml.matches("<tie type=\"stop\"/>").count(), 1);
        assert!(xml.contains("<part-name>Bass</part-name>"));
    }

    #[test]
    fn midi_is_a_type_0_file_with_a_channel_per_string() {
        let tab = riff();
        let bytes = midi(&tab);
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(u16::from_be_bytes([bytes[8], bytes[9]]), 0);
        assert_eq!(u16::from_be_bytes([bytes[12], bytes[13]]), 960);
        assert_eq!(&bytes[14..18], b"MTrk");
        let length = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as usize;
        assert_eq!(bytes.len(), 22 + length);
        assert!(bytes.ends_with(&[0xff, 0x2f, 0]));
        // The low E's chug on channel 1+5, E2 = 40.
        assert!(bytes.windows(3).any(|w| w == [0x95, 40, 127]));
        // The bend on the B string (channel 2) reaches a whole step:
        // 8192 + 2/4 * 8191 = 12288.
        let value = 12288u16;
        assert!(bytes
            .windows(3)
            .any(|w| w == [0xe1, (value & 0x7f) as u8, (value >> 7) as u8]));
        // The harmonic over the 12th of the high E sounds E5.
        assert!(bytes.windows(2).any(|w| w == [0x90, 76]));
    }
}

/// The round trip the PR checks by hand: MuseScore imports the MusicXML
/// with no warnings and writes it back with the strings, frets and
/// techniques intact. Needs MuseScore 4 in /Applications:
///
/// ```text
/// cargo test -p cantina-project musescore -- --ignored --nocapture
/// ```
#[cfg(test)]
mod musescore {
    use super::*;

    const MSCORE: &str = "/Applications/MuseScore 4.app/Contents/MacOS/mscore";

    /// `(string, fret)` of every note that is not a tie's continuation,
    /// in order.
    fn places(xml: &str) -> Vec<(u8, u8)> {
        xml.split("<note")
            .skip(1)
            .filter(|n| !n.contains("<tie type=\"stop\"/>") && n.contains("<string>"))
            .map(|n| {
                let get = |tag: &str| -> u8 {
                    let open = format!("<{tag}>");
                    let at = n.find(&open).unwrap() + open.len();
                    n[at..at + n[at..].find('<').unwrap()]
                        .trim()
                        .parse()
                        .unwrap()
                };
                (get("string"), get("fret"))
            })
            .collect()
    }

    #[test]
    #[ignore = "needs MuseScore 4"]
    fn round_trips_through_musescore() {
        let mut tab = tests::riff();
        // A beat of eighth-note triplets and a bar of sixteenth triplets.
        for k in 0..3u64 {
            tab.notes
                .push(TabNote::new(2, 5 + k as u8, 6720 + k * 320, 320));
        }
        for k in 0..6u64 {
            tab.notes
                .push(TabNote::new(1, 3 + k as u8, 7680 + k * 160, 160));
        }
        let dir = std::env::temp_dir().join("cantina-musescore");
        std::fs::create_dir_all(&dir).unwrap();
        let ours = dir.join("riff.musicxml");
        let back = dir.join("riff-back.musicxml");
        let _ = std::fs::remove_file(&back);
        let xml = musicxml(&tab);
        std::fs::write(&ours, &xml).unwrap();
        // No -f: MuseScore refuses a file it has warnings about.
        let status = std::process::Command::new(MSCORE)
            .arg("-o")
            .arg(&back)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "MuseScore refused it: {:?}",
            status.status
        );
        let theirs = std::fs::read_to_string(&back).unwrap();
        assert_eq!(places(&theirs), places(&xml), "strings and frets");
        // What MuseScore 4.5 keeps, as the tabs guide says.
        for tag in [
            "<slur type=\"start\"",
            "<slide",
            "<harmonic",
            "<notehead>x</notehead>",
            "P.M.",
            "let ring",
            "<wavy-line",
            "<staff-lines>6</staff-lines>",
            "<staff-tuning",
            "<tuplet type=\"start\"",
        ] {
            assert!(theirs.contains(tag), "MuseScore lost {tag}");
        }
        // And what it does not read from MusicXML at all.
        for tag in ["<bend", "<tap", "<hammer-on", "<pull-off"] {
            println!(
                "{tag:<12} {}",
                if theirs.contains(tag) { "kept" } else { "lost" }
            );
        }
    }
}
