// The start dialog's pure parts: what each recent project says, and what
// Escape picks. Kept apart from engine.ts so Vitest can import them.
import type { ProjectStart, RecentProject, Tempo } from "./engine";

/** The sample rates a new project may be made at, as the shell allows. */
export const SAMPLE_RATES = [44_100, 48_000, 88_200, 96_000] as const;

/** The time signatures the dialog offers. */
export const METERS: readonly Pick<Tempo, "numerator" | "denominator">[] = [
  { numerator: 4, denominator: 4 },
  { numerator: 3, denominator: 4 },
  { numerator: 2, denominator: 4 },
  { numerator: 5, denominator: 4 },
  { numerator: 6, denominator: 8 },
  { numerator: 7, denominator: 8 },
  { numerator: 12, denominator: 8 },
];

/** What a new project can start with, in the order the dialog lists them. */
export const STARTS: readonly { id: ProjectStart; label: string }[] = [
  { id: "guitar", label: "One guitar track" },
  { id: "guitarBass", label: "Guitar and bass" },
  { id: "guitarDrums", label: "Guitar and drums" },
  { id: "empty", label: "Empty" },
];

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** When a recent project was saved, as the dialog says it: "Saved 3 hours ago". */
export function savedLabel(recent: RecentProject, now: number): string {
  if (recent.missing || recent.saved === null) return "Moved or deleted";
  const ago = Math.max(0, now - recent.saved);
  if (ago < MINUTE) return "Saved just now";
  const plural = (n: number, unit: string) => `Saved ${n} ${unit}${n === 1 ? "" : "s"} ago`;
  if (ago < HOUR) return plural(Math.floor(ago / MINUTE), "minute");
  if (ago < DAY) return plural(Math.floor(ago / HOUR), "hour");
  if (ago < 2 * DAY) return "Saved yesterday";
  if (ago < 30 * DAY) return plural(Math.floor(ago / DAY), "day");
  const date = new Date(recent.saved);
  return `Saved ${date.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`;
}

/**
 * What Escape picks: the most recent project that is still there, or
 * null for the project already open (a new Untitled when there was none).
 */
export function defaultProject(recent: RecentProject[]): string | null {
  return recent.find((r) => !r.missing)?.name ?? null;
}

/**
 * The name a new project starts with: `Untitled`, or the first of
 * `Untitled 2`, `Untitled 3`… that no project in `taken` has. Names are
 * compared without case, as the Mac's file system does.
 */
export function freeProjectName(taken: readonly string[]): string {
  const used = new Set(taken.map((n) => n.toLowerCase()));
  if (!used.has("untitled")) return "Untitled";
  let n = 2;
  while (used.has(`untitled ${n}`)) n += 1;
  return `Untitled ${n}`;
}
