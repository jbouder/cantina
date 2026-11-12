import { TONICS } from "./jam";

/**
 * Roman numerals spelled out as chord names in a key, the way the
 * backing hears them (`crates/project/src/chords.rs`): a numeral counts
 * from the tonic as a major scale does, whatever the mode, so in E minor
 * `i5 | bVI5 | bVII5 | i5` is `E5 | C5 | D5 | E5`. Upper case is major,
 * lower case minor; chord names, `|` and `-` pass through unchanged.
 */

/** Semitones above the tonic of each degree, longest numerals first. */
const DEGREES: [string, number][] = [
  ["vii", 11],
  ["iii", 4],
  ["vi", 9],
  ["iv", 5],
  ["ii", 2],
  ["v", 7],
  ["i", 0],
];

/** Suffixes that already say the chord's third, or that it has none. */
const OWN_THIRD = ["5", "dim", "°", "ø", "m7b5", "m"];

/** A numeral's root as a pitch class, whether it is minor, and its suffix; null if not a numeral. */
function numeral(
  text: string,
  tonic: number,
): { root: number; minor: boolean; rest: string } | null {
  let shift = 0;
  let rest = text;
  for (;;) {
    const c = rest[0];
    if (c === "b" || c === "♭") shift -= 1;
    else if (c === "#" || c === "♯") shift += 1;
    else break;
    rest = rest.slice(1);
  }
  const lower = rest.toLowerCase();
  const found = DEGREES.find(([n]) => lower.startsWith(n));
  if (!found) return null;
  const [n, semitones] = found;
  const written = rest.slice(0, n.length);
  const minor = written === written.toLowerCase();
  if (!minor && written !== written.toUpperCase()) return null;
  return {
    root: (((tonic + semitones + shift) % 12) + 12) % 12,
    minor,
    rest: rest.slice(n.length),
  };
}

/** One chord token as a name; a token that is not a numeral comes back as it is. */
export function spellChord(token: string, tonic: number): string {
  const [body = "", bass] = token.split("/", 2);
  const n = numeral(body, tonic);
  if (!n) return token;
  const third = n.minor && !OWN_THIRD.some((s) => n.rest.startsWith(s)) ? "m" : "";
  let name = `${TONICS[n.root]}${third}${n.rest}`;
  if (bass !== undefined) {
    const b = numeral(bass, tonic);
    name += `/${b && b.rest === "" ? TONICS[b.root] : bass}`;
  }
  return name;
}

/** A whole progression spelled in the key whose tonic is `tonic` (a name in `TONICS`). */
export function spellChords(text: string, tonic: string): string {
  const pc = Math.max(0, TONICS.indexOf(tonic));
  return text
    .split(/(\s+|\|)/)
    .map((part) =>
      part.trim() === "" || part === "|" || part === "-" ? part : spellChord(part, pc),
    )
    .join("")
    .replace(/\s*\|\s*/g, " | ")
    .trim();
}
