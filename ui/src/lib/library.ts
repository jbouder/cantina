// The library folders under ~/Music/Cantina (Pedals, Kits and Patterns),
// as the shell reports them. Kept apart from engine.ts so tests can
// import the helpers without a window.

/** Which kind of library file. */
export type LibraryKind = "pedal" | "kit" | "pattern";

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

/**
 * The library folders under ~/Music/Cantina, which the shell watches:
 * a file written to one appears within a second. Mirrors `LibraryStatus`.
 */
export interface LibraryStatus {
  pedalsDir: string;
  kitsDir: string;
  patternsDir: string;
  patterns: LibraryPattern[];
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
