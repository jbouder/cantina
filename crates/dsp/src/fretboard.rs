//! Fitting notes to the fretboard: a string and a fret for every note,
//! so a list of pitches becomes a tab a player can play.
//!
//! The same E can be played in five places, so pitch alone does not make
//! a tab. [`fit`] chooses among the places by what a hand does: it keeps
//! each chord within a stretch, moves the hand as little as it can from
//! one chord or note to the next, prefers open strings in low positions
//! as players do, prefers lower positions slightly to higher, avoids
//! cutting off a string that is still ringing, and plays a repeated note
//! in the same place. It is a shortest path (Viterbi) over the chord
//! groups, each group's candidates being every way to put its notes on
//! different strings without leaving a silent string between two that
//! sound.
//!
//! Before fitting, a transcription is [`clean`]ed of the mistakes the
//! transcription spike (#170) found in it:
//!
//! - [`limit_range`]: a note below the lowest open string moves up an
//!   octave, one above the highest fret down. Distortion's difference
//!   tones put driven chords an octave low.
//! - [`suppress_harmonics`]: a quieter note an octave, a twelfth or two
//!   octaves above a note it sounds with is the string's harmonic, unless
//!   other notes make it a chord.
//! - [`merge_bends`]: a note one or two semitones above the one before it,
//!   arriving as that note's pitch bends up to it, is the bend, not a new
//!   note.
//!
//! [`techniques`] then reads hammer-ons, pull-offs, palm mutes and bends
//! from the timing and loudness of the fitted notes. These are guesses
//! from the audio; slides, vibrato and harmonics are left to the player.
//!
//! [`refit`] places a selection again, in a region of the neck, leaving
//! the other notes where they are.
//!
//! Everything here is pure: notes in, notes or places out. Times are in
//! seconds, so it runs on a transcription before it is quantised and on
//! a tab's notes converted at its tempo alike. Strings are numbered as
//! tab is read, 1 for the highest-pitched.

/// A fretboard: the open strings, the capo and how many frets there are.
#[derive(Debug, Clone, PartialEq)]
pub struct Fretboard {
    /// The open strings from the lowest to the highest, as MIDI notes.
    pub tuning: Vec<u8>,
    /// The fret the capo is at; 0 for none. Frets count from it.
    pub capo: u8,
    /// The highest fret above the capo that can be played.
    pub frets: u8,
}

/// A note to fit: its pitch, when it sounds and how loud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// The MIDI note it starts on.
    pub pitch: u8,
    /// Seconds it starts at.
    pub start: f64,
    /// Seconds it ends at.
    pub end: f64,
    /// How loud, 0 to 1.
    pub velocity: f32,
    /// How far the pitch rose while it sounded, in semitones; 0 if not.
    pub bend: f32,
}

/// Where a note is played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Place {
    /// The string, 1 for the highest-pitched.
    pub string: u8,
    /// The fret, counted from the capo.
    pub fret: u8,
}

/// How [`fit`] places notes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitOptions {
    /// Keep fretted notes between these frets, both included, where the
    /// notes allow it: "play this up at the 7th fret" is `Some((7, 10))`.
    /// Open strings count as outside a region that starts above 0.
    pub region: Option<(u8, u8)>,
    /// Notes starting within this many seconds of the first note of a
    /// group are one chord: a strum takes a few tens of milliseconds.
    pub chord_window: f64,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            region: None,
            chord_window: 0.035,
        }
    }
}

/// What [`techniques`] read from a fitted note.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Techniques {
    /// Sounded by hammering on from the note before on the string.
    pub hammer_on: bool,
    /// Sounded by pulling off from the note before on the string.
    pub pull_off: bool,
    /// Damped at the bridge: short, low and spaced like a chug.
    pub palm_mute: bool,
    /// Bent up this many semitones, to the nearest half.
    pub bend: Option<f32>,
}

/// Widest fretted stretch a chord may have, in frets between its lowest
/// and highest fretted note.
const MAX_SPAN: u8 = 5;
/// Most candidate fingerings kept per chord group.
const MAX_CANDIDATES: usize = 48;
/// Intervals at which a string's own harmonics sound above it.
const HARMONICS: [u8; 3] = [12, 19, 24];

impl Fretboard {
    /// A fretboard with `frets` frets.
    pub fn new(tuning: Vec<u8>, capo: u8, frets: u8) -> Self {
        Self {
            tuning,
            capo,
            frets,
        }
    }

    /// How many strings.
    pub fn strings(&self) -> usize {
        self.tuning.len()
    }

    /// The pitch string `string` sounds open, with the capo.
    pub fn open(&self, string: u8) -> Option<u8> {
        let n = self.tuning.len();
        let s = usize::from(string);
        (1..=n)
            .contains(&s)
            .then(|| self.tuning[n - s].saturating_add(self.capo))
    }

    /// The lowest pitch the fretboard plays.
    pub fn lowest(&self) -> u8 {
        self.tuning.iter().min().copied().unwrap_or(0) + self.capo
    }

    /// The highest pitch the fretboard plays.
    pub fn highest(&self) -> u8 {
        self.tuning.iter().max().copied().unwrap_or(0) + self.capo + self.frets
    }

    /// The pitch a place sounds.
    pub fn pitch(&self, place: Place) -> Option<u8> {
        Some(self.open(place.string)? + place.fret)
    }

    /// Every place a pitch can be played, highest string first.
    pub fn places(&self, pitch: u8) -> Vec<Place> {
        (1..=self.tuning.len() as u8)
            .filter_map(|string| {
                let open = self.open(string)?;
                let fret = pitch.checked_sub(open)?;
                (fret <= self.frets).then_some(Place { string, fret })
            })
            .collect()
    }
}

/// Cleans a transcription for `board`: [`limit_range`], then
/// [`suppress_harmonics`], then [`merge_bends`]. Notes come back in
/// order of start.
pub fn clean(board: &Fretboard, notes: &mut Vec<Note>) {
    limit_range(board, notes);
    suppress_harmonics(notes);
    merge_bends(notes);
}

/// Moves notes the fretboard cannot play by octaves until it can: up from
/// below the lowest open string, down from above the highest fret. A note
/// moved onto one that already sounds at the same time is dropped.
pub fn limit_range(board: &Fretboard, notes: &mut Vec<Note>) {
    let (low, high) = (board.lowest(), board.highest());
    if low > high {
        return;
    }
    let mut moved = vec![false; notes.len()];
    for (note, moved) in notes.iter_mut().zip(&mut moved) {
        while note.pitch < low {
            note.pitch += 12;
            *moved = true;
        }
        while note.pitch > high {
            note.pitch -= 12;
            *moved = true;
        }
    }
    let mut keep = vec![true; notes.len()];
    for i in 0..notes.len() {
        if !moved[i] {
            continue;
        }
        let doubled = notes.iter().enumerate().any(|(j, other)| {
            j != i
                && keep[j]
                && other.pitch == notes[i].pitch
                && other.start < notes[i].end
                && notes[i].start < other.end
        });
        if doubled {
            keep[i] = false;
        }
    }
    retain(notes, &keep);
}

/// Drops notes that are a louder note's harmonics: an octave, a twelfth
/// or two octaves above a note sounding at their start, no louder than
/// it, when every note sounding with them is also at one of those
/// intervals. A note a fifth or a third above the root makes the group a
/// chord, and then nothing is dropped, so a power chord keeps its octave.
pub fn suppress_harmonics(notes: &mut Vec<Note>) {
    sort(notes);
    let mut keep = vec![true; notes.len()];
    for (i, h) in notes.iter().enumerate() {
        let sounding = |n: &&Note| n.start <= h.start + 0.05 && n.end > h.start;
        let Some(root) = notes
            .iter()
            .filter(sounding)
            .filter(|n| {
                n.velocity >= h.velocity
                    && h.pitch
                        .checked_sub(n.pitch)
                        .is_some_and(|d| HARMONICS.contains(&d))
            })
            .min_by_key(|n| n.pitch)
        else {
            continue;
        };
        let chord = notes.iter().filter(sounding).any(|n| {
            n.pitch
                .checked_sub(root.pitch)
                .is_none_or(|d| d != 0 && !HARMONICS.contains(&d))
        });
        if !chord {
            keep[i] = false;
        }
    }
    retain(notes, &keep);
}

/// Merges a bend that transcription heard as two notes: a note one or
/// two semitones above the note before it, starting as that note ends
/// (or while it still sounds), when that note's pitch rose most of the
/// way to it. The first note takes the second's end and the bend.
pub fn merge_bends(notes: &mut Vec<Note>) {
    sort(notes);
    let mut keep = vec![true; notes.len()];
    for i in 0..notes.len() {
        if !keep[i] {
            continue;
        }
        let a = notes[i];
        let Some(j) = (i + 1..notes.len()).find(|&j| {
            let b = &notes[j];
            keep[j]
                && b.start >= a.start
                && b.start <= a.end + 0.03
                && matches!(b.pitch.checked_sub(a.pitch), Some(1 | 2))
                && a.bend >= f32::from(b.pitch - a.pitch) - 0.5
        }) else {
            continue;
        };
        let b = notes[j];
        notes[i].end = notes[i].end.max(b.end);
        notes[i].bend = notes[i].bend.max(f32::from(b.pitch - a.pitch));
        keep[j] = false;
    }
    retain(notes, &keep);
}

fn sort(notes: &mut [Note]) {
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
}

fn retain(notes: &mut Vec<Note>, keep: &[bool]) {
    let mut i = 0;
    notes.retain(|_| {
        i += 1;
        keep[i - 1]
    });
}

/// A way to play one chord group: a place per note, `None` for a note
/// that could not be placed.
#[derive(Debug, Clone)]
struct Fingering {
    places: Vec<Option<Place>>,
    cost: f32,
}

impl Fingering {
    /// The lowest fretted fret, where the hand is; `None` if all open.
    fn position(&self) -> Option<u8> {
        self.places
            .iter()
            .flatten()
            .map(|p| p.fret)
            .filter(|&f| f > 0)
            .min()
    }
}

/// Chooses a place for every note: `places[i]` is where `notes[i]` is
/// played, `None` for a note the fretboard cannot play (out of range, or
/// more notes at once than strings).
pub fn fit(board: &Fretboard, notes: &[Note], options: FitOptions) -> Vec<Option<Place>> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&a, &b| {
        notes[a]
            .start
            .total_cmp(&notes[b].start)
            .then(notes[a].pitch.cmp(&notes[b].pitch))
    });
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in order {
        match groups.last_mut() {
            Some(g) if notes[i].start - notes[g[0]].start <= options.chord_window => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    let mut out = vec![None; notes.len()];
    if groups.is_empty() {
        return out;
    }

    // Viterbi over the groups: for each candidate fingering of a group,
    // the cheapest way to reach it and where that came from.
    let candidates: Vec<Vec<Fingering>> = groups
        .iter()
        .map(|g| fingerings(board, notes, g, options))
        .collect();
    let mut cost: Vec<f32> = candidates[0].iter().map(|f| f.cost).collect();
    let mut back: Vec<Vec<usize>> = vec![vec![0; candidates[0].len()]];
    for k in 1..groups.len() {
        let mut next = Vec::with_capacity(candidates[k].len());
        let mut from = Vec::with_capacity(candidates[k].len());
        for b in &candidates[k] {
            let (best, c) = candidates[k - 1]
                .iter()
                .enumerate()
                .map(|(ai, a)| {
                    let t = transition(notes, &groups[k - 1], a, &groups[k], b);
                    (ai, cost[ai] + t)
                })
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .unwrap_or((0, 0.0));
            next.push(c + b.cost);
            from.push(best);
        }
        cost = next;
        back.push(from);
    }
    let mut at = (0..cost.len())
        .min_by(|&a, &b| cost[a].total_cmp(&cost[b]))
        .unwrap_or(0);
    for k in (0..groups.len()).rev() {
        let fingering = &candidates[k][at];
        for (&i, place) in groups[k].iter().zip(&fingering.places) {
            out[i] = *place;
        }
        at = back[k][at];
    }
    out
}

/// The ways to play one group, cheapest first, at most
/// [`MAX_CANDIDATES`]. When no way places every note, the notes with the
/// fewest places are left out one at a time until one does.
fn fingerings(
    board: &Fretboard,
    notes: &[Note],
    group: &[usize],
    options: FitOptions,
) -> Vec<Fingering> {
    let options_per_note: Vec<Vec<Place>> = group
        .iter()
        .map(|&i| board.places(notes[i].pitch))
        .collect();
    let mut dropped = vec![false; group.len()];
    loop {
        let mut found = Vec::new();
        let mut current = Vec::with_capacity(group.len());
        search(
            &options_per_note,
            &dropped,
            &mut current,
            &mut found,
            options,
        );
        if !found.is_empty() || dropped.iter().all(|&d| d) {
            if found.is_empty() {
                found.push(Fingering {
                    places: vec![None; group.len()],
                    cost: 0.0,
                });
            }
            found.sort_by(|a, b| a.cost.total_cmp(&b.cost));
            found.truncate(MAX_CANDIDATES);
            return found;
        }
        // Leave out the note with the fewest places (the hardest to fit),
        // the highest of those when they tie.
        let worst = (0..group.len())
            .filter(|&i| !dropped[i])
            .min_by_key(|&i| {
                (
                    options_per_note[i].len(),
                    std::cmp::Reverse(notes[group[i]].pitch),
                )
            })
            .expect("a note is left");
        dropped[worst] = true;
    }
}

fn search(
    options_per_note: &[Vec<Place>],
    dropped: &[bool],
    current: &mut Vec<Option<Place>>,
    found: &mut Vec<Fingering>,
    options: FitOptions,
) {
    if found.len() >= MAX_CANDIDATES * 8 {
        return;
    }
    let i = current.len();
    if i == options_per_note.len() {
        if let Some(cost) = shape_cost(current, options) {
            found.push(Fingering {
                places: current.clone(),
                cost,
            });
        }
        return;
    }
    if dropped[i] {
        current.push(None);
        search(options_per_note, dropped, current, found, options);
        current.pop();
        return;
    }
    for &place in &options_per_note[i] {
        if current.iter().flatten().any(|p| p.string == place.string) {
            continue;
        }
        current.push(Some(place));
        search(options_per_note, dropped, current, found, options);
        current.pop();
    }
}

/// Places the notes at `selection` again, inside `options.region` where
/// they fit, and leaves every other note's place as it is in `places`.
/// The selection is fitted as a passage of its own, so it moves as one
/// hand position: "play this up at the 7th fret".
pub fn refit(
    board: &Fretboard,
    notes: &[Note],
    places: &[Option<Place>],
    selection: &[usize],
    options: FitOptions,
) -> Vec<Option<Place>> {
    let chosen: Vec<Note> = selection.iter().map(|&i| notes[i]).collect();
    let fitted = fit(board, &chosen, options);
    let mut out = places.to_vec();
    for (&i, place) in selection.iter().zip(fitted) {
        out[i] = place;
    }
    out
}

/// What a fingering costs on its own; `None` past the widest stretch.
fn shape_cost(places: &[Option<Place>], options: FitOptions) -> Option<f32> {
    let fretted: Vec<u8> = places
        .iter()
        .flatten()
        .map(|p| p.fret)
        .filter(|&f| f > 0)
        .collect();
    let low = fretted.iter().min().copied();
    let span = match low {
        Some(low) => fretted.iter().max().copied().unwrap_or(low) - low,
        None => 0,
    };
    if span > MAX_SPAN {
        return None;
    }
    let position = low.unwrap_or(0);
    let mut cost = f32::from(span) * 0.25 + f32::from(span.saturating_sub(3)) * 1.5;
    for place in places.iter().flatten() {
        if place.fret == 0 {
            // Open strings ring easily in low positions; up the neck a
            // player reaches for them less.
            cost += if position <= 5 { -0.35 } else { 0.4 };
        } else {
            cost += f32::from(place.fret) * 0.03;
        }
        if let Some((lo, hi)) = options.region {
            let inside = (lo..=hi).contains(&place.fret) || (place.fret == 0 && lo == 0);
            if !inside {
                cost += 10.0;
            }
        }
    }
    // A string left silent between two that sound is hard to strum
    // cleanly, so chords keep their strings together.
    let mut strings: Vec<u8> = places.iter().flatten().map(|p| p.string).collect();
    strings.sort_unstable();
    if let (Some(first), Some(last)) = (strings.first(), strings.last()) {
        let skipped = usize::from(last - first) + 1 - strings.len();
        cost += skipped as f32 * 3.0;
    }
    // Leaving a note out costs more than any awkward way of playing it.
    cost += places.iter().filter(|p| p.is_none()).count() as f32 * 20.0;
    Some(cost)
}

/// What moving from one fingering to the next costs.
fn transition(notes: &[Note], ga: &[usize], a: &Fingering, gb: &[usize], b: &Fingering) -> f32 {
    let mut cost = 0.0;
    if let (Some(pa), Some(pb)) = (a.position(), b.position()) {
        let shift = f32::from(pa.abs_diff(pb));
        cost += shift * 0.4 + (shift - 4.0).max(0.0) * 0.6;
    }
    let start = gb
        .iter()
        .map(|&i| notes[i].start)
        .fold(f64::INFINITY, f64::min);
    for (&i, pa) in ga.iter().zip(&a.places) {
        let Some(pa) = pa else { continue };
        for (&j, pb) in gb.iter().zip(&b.places) {
            let Some(pb) = pb else { continue };
            // Cutting off a string that is still ringing.
            if pa.string == pb.string && notes[i].end > start + 0.02 {
                cost += 0.8;
            }
            // The same pitch again is played in the same place.
            if notes[i].pitch == notes[j].pitch && pa != pb {
                cost += 1.0;
            }
        }
    }
    cost
}

/// Reads techniques from fitted notes: `places[i]` is where `notes[i]`
/// is played.
///
/// - A **hammer-on** or **pull-off** is a note on the same string as the
///   note just before it, starting as that one ends, a fret or five away,
///   and noticeably softer: there was no pick attack. Up the neck is a
///   hammer-on, down a pull-off.
/// - A **palm mute** is a note on one of the three lowest strings shorter
///   than 150 ms and than 60% of the time to the next note on its string:
///   the clipped decay of a chug.
/// - A **bend** is a rise of at least half a semitone, rounded to the
///   nearest half.
pub fn techniques(board: &Fretboard, notes: &[Note], places: &[Option<Place>]) -> Vec<Techniques> {
    let n = board.strings() as u8;
    let mut out = vec![Techniques::default(); notes.len()];
    for (i, note) in notes.iter().enumerate() {
        let Some(place) = places[i] else { continue };
        if note.bend >= 0.5 {
            out[i].bend = Some((note.bend * 2.0).round() / 2.0);
        }
        let same_string = |j: &usize| places[*j].is_some_and(|p| p.string == place.string);
        let before = (0..notes.len())
            .filter(same_string)
            .filter(|&j| notes[j].start < note.start)
            .max_by(|&a, &b| notes[a].start.total_cmp(&notes[b].start));
        if let Some(j) = before {
            let prev = &notes[j];
            let fret = places[j].map(|p| p.fret).unwrap_or(0);
            let touching = note.start - prev.end <= 0.02;
            let soft = note.velocity < prev.velocity * 0.8;
            let reach = (1..=5).contains(&fret.abs_diff(place.fret));
            if touching && soft && reach {
                if place.fret > fret {
                    out[i].hammer_on = true;
                } else {
                    out[i].pull_off = true;
                }
            }
        }
        let next = (0..notes.len())
            .filter(same_string)
            .filter(|&j| notes[j].start > note.start)
            .map(|j| notes[j].start)
            .fold(f64::INFINITY, f64::min);
        let length = note.end - note.start;
        let low_string = place.string + 2 >= n;
        if low_string && length < 0.15 && next.is_finite() && length < 0.6 * (next - note.start) {
            out[i].palm_mute = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDARD: [u8; 6] = [40, 45, 50, 55, 59, 64];

    fn guitar() -> Fretboard {
        Fretboard::new(STANDARD.to_vec(), 0, 22)
    }

    fn note(pitch: u8, start: f64, end: f64) -> Note {
        Note {
            pitch,
            start,
            end,
            velocity: 0.8,
            bend: 0.0,
        }
    }

    fn chord(pitches: &[u8]) -> Vec<Note> {
        pitches.iter().map(|&p| note(p, 0.0, 1.0)).collect()
    }

    fn line(pitches: &[u8]) -> Vec<Note> {
        pitches
            .iter()
            .enumerate()
            .map(|(i, &p)| note(p, i as f64 * 0.25, i as f64 * 0.25 + 0.25))
            .collect()
    }

    /// The fitted (string, fret) pairs, in note order.
    fn shape(board: &Fretboard, notes: &[Note], options: FitOptions) -> Vec<(u8, u8)> {
        fit(board, notes, options)
            .into_iter()
            .map(|p| {
                let p = p.expect("placed");
                (p.string, p.fret)
            })
            .collect()
    }

    #[test]
    fn places_cover_every_string_that_reaches() {
        let g = guitar();
        // E4: open first string, 5th fret B, 9th G, 14th D, 19th A.
        let frets: Vec<(u8, u8)> = g.places(64).iter().map(|p| (p.string, p.fret)).collect();
        assert_eq!(frets, [(1, 0), (2, 5), (3, 9), (4, 14), (5, 19)]);
        assert!(g.places(39).is_empty());
        for p in g.places(64) {
            assert_eq!(g.pitch(p), Some(64));
        }
    }

    #[test]
    fn open_chords_take_their_open_shapes() {
        let g = guitar();
        let o = FitOptions::default();
        // G major: 320003.
        assert_eq!(
            shape(&g, &chord(&[43, 47, 50, 55, 59, 67]), o),
            [(6, 3), (5, 2), (4, 0), (3, 0), (2, 0), (1, 3)]
        );
        // C major: x32010.
        assert_eq!(
            shape(&g, &chord(&[48, 52, 55, 60, 64]), o),
            [(5, 3), (4, 2), (3, 0), (2, 1), (1, 0)]
        );
        // E minor: 022000.
        assert_eq!(
            shape(&g, &chord(&[40, 47, 52, 55, 59, 64]), o),
            [(6, 0), (5, 2), (4, 2), (3, 0), (2, 0), (1, 0)]
        );
    }

    #[test]
    fn power_chords_keep_their_shape() {
        let g = guitar();
        let o = FitOptions::default();
        assert_eq!(
            shape(&g, &chord(&[40, 47, 52]), o),
            [(6, 0), (5, 2), (4, 2)]
        );
        // G5 at the third fret: root on the sixth string.
        assert_eq!(
            shape(&g, &chord(&[43, 50, 55]), o),
            [(6, 3), (5, 5), (4, 5)]
        );
    }

    #[test]
    fn a_line_stays_in_one_position() {
        let g = guitar();
        // E minor pentatonic up from E3.
        let notes = line(&[52, 55, 57, 59, 62, 64, 62, 59, 57, 55, 52]);
        let frets = shape(&g, &notes, FitOptions::default());
        assert!(frets.iter().all(|&(_, f)| f <= 3), "{frets:?}");
        // The same line up at the 12th fret.
        let up = shape(
            &g,
            &notes,
            FitOptions {
                region: Some((12, 15)),
                ..FitOptions::default()
            },
        );
        assert!(up.iter().all(|&(_, f)| (12..=15).contains(&f)), "{up:?}");
        for (place, n) in up.iter().zip(&notes) {
            let p = Place {
                string: place.0,
                fret: place.1,
            };
            assert_eq!(g.pitch(p), Some(n.pitch));
        }
    }

    #[test]
    fn a_repeated_note_stays_put() {
        let g = guitar();
        let frets = shape(&g, &line(&[57, 57, 57, 57]), FitOptions::default());
        assert!(frets.windows(2).all(|w| w[0] == w[1]), "{frets:?}");
    }

    #[test]
    fn drop_d_and_capo_and_baritone() {
        // Drop D: the D5 chord is three open strings.
        let drop_d = Fretboard::new(vec![38, 45, 50, 55, 59, 64], 0, 22);
        assert_eq!(
            shape(&drop_d, &chord(&[38, 45, 50]), FitOptions::default()),
            [(6, 0), (5, 0), (4, 0)]
        );
        // A capo at 2: F#2 is the open low string.
        let capo = Fretboard::new(STANDARD.to_vec(), 2, 20);
        assert_eq!(
            shape(&capo, &[note(42, 0.0, 1.0)], FitOptions::default()),
            [(6, 0)]
        );
        assert!(capo.places(41).is_empty());
        // A seven-string's low B, and a baritone in B standard.
        let seven = Fretboard::new(vec![35, 40, 45, 50, 55, 59, 64], 0, 24);
        assert_eq!(
            shape(&seven, &[note(35, 0.0, 1.0)], FitOptions::default()),
            [(7, 0)]
        );
        let baritone = Fretboard::new(vec![35, 40, 45, 50, 54, 59], 0, 24);
        assert_eq!(
            shape(&baritone, &chord(&[35, 42, 47]), FitOptions::default()),
            [(6, 0), (5, 2), (4, 2)]
        );
    }

    #[test]
    fn bass_lines_fit_low() {
        let bass = Fretboard::new(vec![28, 33, 38, 43], 0, 20);
        let notes = line(&[28, 28, 40, 28, 31, 33, 33, 45, 36, 35]);
        let frets = shape(&bass, &notes, FitOptions::default());
        assert!(frets.iter().all(|&(_, f)| f <= 7), "{frets:?}");
        assert_eq!(frets[0], (4, 0));
        let five = Fretboard::new(vec![23, 28, 33, 38, 43], 0, 24);
        assert_eq!(
            shape(&five, &[note(23, 0.0, 1.0)], FitOptions::default()),
            [(5, 0)]
        );
    }

    #[test]
    fn what_cannot_be_played_is_left_out() {
        let g = guitar();
        let low = fit(&g, &[note(30, 0.0, 1.0)], FitOptions::default());
        assert_eq!(low, [None]);
        // Seven notes at once on six strings: one is left out.
        let seven = fit(
            &g,
            &chord(&[40, 45, 50, 55, 59, 64, 69]),
            FitOptions::default(),
        );
        assert_eq!(seven.iter().filter(|p| p.is_none()).count(), 1);
        let strings: std::collections::HashSet<u8> =
            seven.iter().flatten().map(|p| p.string).collect();
        assert_eq!(strings.len(), 6);
    }

    #[test]
    fn range_limit_moves_low_roots_up_and_drops_doubles() {
        let g = guitar();
        // A driven E5 heard with its root an octave low, and a sub-octave
        // ghost under a real root.
        let mut notes = vec![note(28, 0.0, 1.0), note(47, 0.0, 1.0), note(52, 0.0, 1.0)];
        limit_range(&g, &mut notes);
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [40, 47, 52]);
        let mut doubled = vec![note(28, 0.0, 1.0), note(40, 0.0, 1.0)];
        limit_range(&g, &mut doubled);
        assert_eq!(doubled.len(), 1);
        assert_eq!(doubled[0].pitch, 40);
        let mut high = vec![note(100, 0.0, 1.0)];
        limit_range(&g, &mut high);
        assert!(high[0].pitch <= g.highest());
    }

    #[test]
    fn harmonics_go_but_chords_stay() {
        // A single E2 heard with its octave and twelfth, quieter.
        let ghost = |p| Note {
            velocity: 0.3,
            ..note(p, 0.01, 0.8)
        };
        let mut single = vec![note(40, 0.0, 1.0), ghost(52), ghost(59)];
        suppress_harmonics(&mut single);
        assert_eq!(single.len(), 1);
        // A power chord's octave is played, and the fifth shows it.
        let mut power = vec![note(40, 0.0, 1.0), note(47, 0.0, 1.0), ghost(52)];
        suppress_harmonics(&mut power);
        assert_eq!(power.len(), 3);
        // A louder octave is a note of its own.
        let mut loud = vec![note(40, 0.0, 1.0), note(52, 0.5, 1.0)];
        loud[1].velocity = 0.9;
        suppress_harmonics(&mut loud);
        assert_eq!(loud.len(), 2);
    }

    #[test]
    fn a_bend_heard_as_two_notes_is_one() {
        let mut bent = vec![
            Note {
                bend: 1.8,
                ..note(59, 0.0, 0.3)
            },
            note(61, 0.31, 1.0),
            note(57, 1.0, 1.5),
        ];
        merge_bends(&mut bent);
        assert_eq!(bent.len(), 2);
        assert_eq!(bent[0].pitch, 59);
        assert_eq!(bent[0].end, 1.0);
        assert_eq!(bent[0].bend, 2.0);
        // Without the pitch rising, the next note is a new note.
        let mut picked = vec![note(59, 0.0, 0.3), note(61, 0.31, 1.0)];
        merge_bends(&mut picked);
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn clean_then_fit_a_driven_riff() {
        let g = guitar();
        // E5 heard an octave low with a harmonic ghost, then a bent B.
        let mut notes = vec![
            note(28, 0.0, 0.5),
            note(47, 0.0, 0.5),
            note(52, 0.0, 0.5),
            Note {
                bend: 2.0,
                ..note(59, 0.5, 0.8)
            },
            note(61, 0.8, 1.2),
        ];
        clean(&g, &mut notes);
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [40, 47, 52, 59]);
        let places = fit(&g, &notes, FitOptions::default());
        assert!(places.iter().all(Option::is_some));
        let t = techniques(&g, &notes, &places);
        assert_eq!(t[3].bend, Some(2.0));
    }

    #[test]
    fn legato_and_palm_mutes_are_read_from_timing() {
        let g = guitar();
        // A picked A on the third string at the 2nd fret, then softer
        // notes straight after at the 4th and back at the 2nd.
        let mut notes = vec![note(57, 0.0, 0.2), note(59, 0.2, 0.4), note(57, 0.4, 0.6)];
        notes[1].velocity = 0.5;
        notes[2].velocity = 0.3;
        let places = vec![
            Some(Place { string: 3, fret: 2 }),
            Some(Place { string: 3, fret: 4 }),
            Some(Place { string: 3, fret: 2 }),
        ];
        let t = techniques(&g, &notes, &places);
        assert!(!t[0].hammer_on && !t[0].pull_off);
        assert!(t[1].hammer_on);
        assert!(t[2].pull_off);
        // Picked at full strength: not legato.
        notes[1].velocity = 0.8;
        assert!(!techniques(&g, &notes, &places)[1].hammer_on);

        // Chugs: short low E notes an eighth apart.
        let chugs: Vec<Note> = (0..4)
            .map(|i| note(40, i as f64 * 0.214, i as f64 * 0.214 + 0.1))
            .collect();
        let places = fit(&g, &chugs, FitOptions::default());
        let t = techniques(&g, &chugs, &places);
        assert!(t[..3].iter().all(|t| t.palm_mute), "{t:?}");
        // Held notes are not muted.
        let held = line(&[40, 40]);
        let places = fit(&g, &held, FitOptions::default());
        assert!(!techniques(&g, &held, &places)[0].palm_mute);
    }

    #[test]
    fn a_selection_moves_up_the_neck_alone() {
        let g = guitar();
        let notes = line(&[52, 55, 57, 59, 62, 64]);
        let places = fit(&g, &notes, FitOptions::default());
        let moved = refit(
            &g,
            &notes,
            &places,
            &[2, 3, 4],
            FitOptions {
                region: Some((7, 10)),
                ..FitOptions::default()
            },
        );
        for i in [0, 1, 5] {
            assert_eq!(moved[i], places[i], "note {i} was not selected");
        }
        for i in [2, 3, 4] {
            let p = moved[i].unwrap();
            assert!((7..=10).contains(&p.fret), "note {i} at {p:?}");
            assert_eq!(g.pitch(p), Some(notes[i].pitch));
        }
    }

    /// The hand-written reference takes in `takes/`, as notes: pitch
    /// names and beats at the take's tempo.
    fn takes() -> Vec<(String, Fretboard, Vec<Note>)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../takes");
        let mut out = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(".take.json"))
            .collect();
        paths.sort();
        let midi = |name: &str| -> u8 {
            let b = name.as_bytes();
            let step = [9, 11, 0, 2, 4, 5, 7][usize::from(b[0] - b'A')];
            let (acc, rest) = match b[1] {
                b'#' => (1, &name[2..]),
                b'b' => (-1, &name[2..]),
                _ => (0, &name[1..]),
            };
            let octave: i32 = rest.parse().unwrap();
            ((octave + 1) * 12 + step + acc) as u8
        };
        for path in paths {
            let take: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let beat = 60.0 / take["tempo"].as_f64().unwrap();
            let board = if take["instrument"] == "bass" {
                Fretboard::new(vec![28, 33, 38, 43], 0, 20)
            } else {
                guitar()
            };
            let mut notes = Vec::new();
            for n in take["notes"].as_array().unwrap() {
                let start = n["beat"].as_f64().unwrap() * beat;
                let end = start + n["length"].as_f64().unwrap() * beat;
                let pitches: Vec<u8> = match &n["pitch"] {
                    serde_json::Value::Array(a) => {
                        a.iter().map(|p| midi(p.as_str().unwrap())).collect()
                    }
                    p => vec![midi(p.as_str().unwrap())],
                };
                for p in pitches {
                    notes.push(note(p, start, end));
                }
            }
            let id = path.file_name().unwrap().to_string_lossy().to_string();
            out.push((id, board, notes));
        }
        out
    }

    #[test]
    fn every_reference_take_fits_playably() {
        let takes = takes();
        assert_eq!(takes.len(), 6);
        for (id, board, notes) in takes {
            let places = fit(&board, &notes, FitOptions::default());
            for (n, p) in notes.iter().zip(&places) {
                let p = p.unwrap_or_else(|| panic!("{id}: {n:?} was not placed"));
                assert_eq!(board.pitch(p), Some(n.pitch), "{id}");
            }
            // Every chord within a stretch, on strings next to each other.
            let mut starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
            starts.dedup();
            for s in starts {
                let chord: Vec<Place> = notes
                    .iter()
                    .zip(&places)
                    .filter(|(n, _)| n.start == s)
                    .map(|(_, p)| p.unwrap())
                    .collect();
                let fretted: Vec<u8> = chord.iter().map(|p| p.fret).filter(|&f| f > 0).collect();
                if let (Some(lo), Some(hi)) = (fretted.iter().min(), fretted.iter().max()) {
                    assert!(hi - lo <= MAX_SPAN, "{id} at {s}: {chord:?}");
                }
                let mut strings: Vec<u8> = chord.iter().map(|p| p.string).collect();
                strings.sort_unstable();
                assert_eq!(
                    usize::from(strings[strings.len() - 1] - strings[0]) + 1,
                    strings.len(),
                    "{id} at {s}: {chord:?}"
                );
            }
            // A part in open position stays low on the neck.
            let highest = places.iter().flatten().map(|p| p.fret).max().unwrap();
            assert!(highest <= 7, "{id} goes up to fret {highest}");
        }
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(fit(&guitar(), &[], FitOptions::default()).is_empty());
        let mut none = Vec::new();
        clean(&guitar(), &mut none);
        assert!(none.is_empty());
    }
}
