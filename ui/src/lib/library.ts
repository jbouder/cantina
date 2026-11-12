// The library folders under ~/Music/Cantina (Pedals, Kits, Patterns, Rigs, Jams,
// Exercises, Courses and Grooves), as the shell reports them. Kept apart from engine.ts so tests can
// import the helpers without a window.

/** Which kind of library file. */
export type LibraryKind =
  | "pedal"
  | "kit"
  | "pattern"
  | "rig"
  | "jam"
  | "exercise"
  | "course"
  | "groove";

/** A library file that does not load. Mirrors `BrokenFile` in src-tauri. */
export interface BrokenFile {
  kind: LibraryKind;
  /** The file or folder name. */
  name: string;
  path: string;
  /** What is wrong, in words. */
  reason: string;
}

/** A pattern file in the library. Mirrors `LibraryPattern` in src-tauri. */
export interface LibraryPattern {
  name: string;
  path: string;
  stepsPerBeat: number;
  length: number;
}

/** A section of a groove, as the palette lists it. Mirrors `GrooveSectionInfo` in src-tauri. */
export interface GrooveSectionInfo {
  name: string;
  kind: "intro" | "verse" | "chorus" | "bridge" | "fill" | "ending" | null;
}

/** A groove of the user's: a whole drum part. Mirrors `GrooveInfo` in src-tauri. */
export interface GrooveInfo {
  /** What jams name it by. */
  name: string;
  genre: string;
  description: string;
  path: string;
  tempo: number;
  timeSignature: { numerator: number; denominator: number };
  kit: string;
  sections: GrooveSectionInfo[];
  /** Bars in its order. */
  bars: number;
}

/**
 * The library folders under ~/Music/Cantina, which the shell watches:
 * a file written to one appears within a second. Mirrors `LibraryStatus`.
 */
export interface LibraryStatus {
  pedalsDir: string;
  kitsDir: string;
  patternsDir: string;
  rigsDir: string;
  jamsDir: string;
  exercisesDir: string;
  coursesDir: string;
  groovesDir: string;
  patterns: LibraryPattern[];
  /** Whole drum parts the drum designer saved, by name. */
  grooves: GrooveInfo[];
  broken: BrokenFile[];
}

/** The broken files of one kind. */
export function brokenOf(library: LibraryStatus, kind: LibraryKind): BrokenFile[] {
  return library.broken.filter((b) => b.kind === kind);
}

/** A file name for a pattern, as the shell would write it. */
export function patternFileName(name: string): string {
  const stem = name.replace(/[^\p{L}\p{N} _-]/gu, "-").trim();
  return `${stem || "Pattern"}.pattern.json`;
}
