import { useEffect, useId, useMemo, useRef, useState } from "react";
import { isTyping } from "../lib/actions";
import { beatDiagram, chordDiagram, type Diagram } from "../lib/diagram";
import {
  countInSet,
  type EngineStatus,
  type ExerciseInfo,
  jamTempo,
  type MeterFrame,
  type PracticeOptions,
  practiceLoad,
  practiceOptions,
  type Teaches,
} from "../lib/engine";
import { ladder, loopPlace, stepTempo, TEACHES_LABELS } from "../lib/exercise";
import { jamPlace } from "../lib/jam";
import { Button } from "./Button";
import { ChordDiagram } from "./ChordDiagram";
import { Field } from "./Field";
import { Fretboard } from "./Fretboard";
import { rigId } from "./JamBacking";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { Metronome } from "./Metronome";
import { Select } from "./Select";
import { TabStaff } from "./TabStaff";

type Props = {
  status: EngineStatus;
  frame: MeterFrame;
  /** Space, the play key, starts and stops it from the app's bindings. */
  onPlay: () => void;
  onLeave: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

const ALL = "";
/** How many passes a climb may wait: 0 holds the tempo. */
const CLIMBS = [0, 1, 2, 4, 8];

/** A log date as a person says it: today, yesterday, or the date. */
function dayName(date: string): string {
  const day = (back: number) => {
    const d = new Date();
    d.setDate(d.getDate() - back);
    const two = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
  };
  if (date === day(0)) return "today";
  if (date === day(1)) return "yesterday";
  return date;
}
const TEACHES: Teaches[] = ["chord", "change", "scale", "technique", "riff", "rhythm"];

/**
 * Practice: an exercise to read and play along with, heard in place of
 * the project, which practice never edits. Pick one by style and by what
 * it teaches; read its instructions and its tab; hear it played through
 * its rig over its groove, with your input through the same rig; loop
 * it, count in, and climb its tempo ladder.
 *
 * Keys: Space plays and stops, ↑ and ↓ climb or drop a rung of the
 * ladder, L loops, G turns the groove on or off.
 */
export function PracticeView({ status, frame, onPlay, onLeave, onStatus, onError }: Props) {
  const ids = useId();
  const practice = status.practicing;
  const jam = status.jam;
  const report = (action: Promise<EngineStatus>) =>
    action.then(onStatus, (e) => onError(String(e)));
  const rolling = frame.playing || frame.countIn > 0;

  const [style, setStyle] = useState(ALL);
  const [teaches, setTeaches] = useState<Teaches | typeof ALL>(ALL);
  const styles = useMemo(
    () => [...new Set(status.exercises.map((e) => e.style))].sort((a, b) => a.localeCompare(b)),
    [status.exercises],
  );
  const shown = status.exercises.filter(
    (e) => (style === ALL || e.style === style) && (teaches === ALL || e.teaches === teaches),
  );

  const options = (next: Partial<PracticeOptions>) => {
    if (!practice) return;
    report(
      practiceOptions({
        looping: practice.looping,
        groove: practice.groove,
        rig: practice.rig,
        climb: practice.climb,
        ...next,
      }),
    );
  };

  // The latest state for the key handler, which is installed once.
  const keys = useRef({ practice, report, options, now: practice?.tempo ?? 0 });
  keys.current = { ...keys.current, practice, report, options };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as Element | null;
      if (isTyping(target) || target?.tagName === "SELECT") return;
      const k = keys.current;
      if (!k.practice) return;
      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        const by = e.key === "ArrowUp" ? 1 : -1;
        k.report(jamTempo(stepTempo(k.practice.exercise, k.now, by)));
      } else if (e.key === "g" || e.key === "G") {
        if (!k.practice.exercise.drums) return;
        k.options({ groove: !k.practice.groove });
      } else {
        return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!practice || !jam) {
    return (
      <section aria-label="Practice" className="grid flex-1 place-items-center text-ink-dim">
        Opening practice…
      </section>
    );
  }

  const exercise = practice.exercise;
  const numerator = exercise.timeSignature?.numerator ?? 4;
  const place = jamPlace(frame.position, jam.marks, status.sampleRate, jam.barTicks, numerator);
  const where = rolling && place ? loopPlace(place.tick, practice.loopTicks) : null;
  const passBar = where ? Math.floor(where.tick / jam.barTicks) + 1 : null;
  // The tempo playing: the climb moves it between status updates.
  const tempo = Math.round((rolling && place ? place.bpm : practice.tempo) * 10) / 10;
  keys.current.now = tempo;
  const rungs = ladder(exercise);
  const rung = rungs.filter((t) => t <= tempo + 1e-6).length;
  const countBars = exercise.countIn ?? 1;
  const ownRig = status.rigs.find((r) => rigId(r.path) === exercise.rig) ?? null;
  const rigs = status.rigs.filter((r) => r.instrument === exercise.instrument);
  const loaded = (e: ExerciseInfo) => e.path === practice.path;
  // The chord sounding and the one after it: the exercise's shapes, or
  // the notes of the beat when it is a phrase of notes.
  const diagrams: Diagram[] = (() => {
    const at = where?.tick ?? 0;
    const phrase = exercise.phrase;
    if (phrase.kind === "chords") {
      const order = [...phrase.chords].sort((a, b) => a.start - b.start);
      const now = [...order].reverse().find((c) => c.start <= at) ?? order[0];
      if (!now) return [];
      const after =
        order.find((c) => c.start > now.start && c.name !== now.name) ??
        order.find((c) => c.name !== now.name);
      const capo = exercise.capo ?? 0;
      return [now, ...(after ? [after] : [])].map((c) => chordDiagram(c, capo));
    }
    const d = beatDiagram(practice.tab, at);
    return d ? [d] : [];
  })();

  return (
    <section
      aria-label="Practice"
      className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] gap-3"
    >
      <header className="flex items-center gap-2">
        <span className="caption shrink-0">Practice</span>
        <Lcd className="min-w-0 max-w-[32rem] truncate">
          {exercise.name} · {exercise.key.tonic} {exercise.key.mode} · {exercise.style}
        </Lcd>
        <span className="flex-1" />
        <Button
          onClick={onLeave}
          title={`Back to ${status.project.name}, which practice never edits`}
        >
          Back to project
        </Button>
      </header>

      <div className="grid min-h-0 items-start gap-3 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)]">
        <div className="surface-panel grid max-h-full min-h-0 grid-rows-[auto_auto_minmax(0,1fr)] gap-2 p-3">
          <Select
            id={`${ids}-style`}
            label="Style"
            value={style}
            onChange={(e) => setStyle(e.target.value)}
          >
            <option value={ALL}>Every style</option>
            {styles.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </Select>
          <Select
            id={`${ids}-teaches`}
            label="Teaches"
            value={teaches}
            onChange={(e) => setTeaches(e.target.value as Teaches | typeof ALL)}
          >
            <option value={ALL}>Everything</option>
            {TEACHES.map((t) => (
              <option key={t} value={t}>
                {TEACHES_LABELS[t]}
              </option>
            ))}
          </Select>
          <ul
            aria-label="Exercises"
            className="scroll-area m-0 grid list-none content-start gap-1 p-0"
          >
            {shown.length === 0 ? (
              <li className="text-ink-dim text-label">No exercises match.</li>
            ) : null}
            {shown.map((e) => (
              <li key={e.path}>
                <button
                  type="button"
                  aria-current={loaded(e) ? "true" : undefined}
                  onClick={() => report(practiceLoad(e.path))}
                  className="button-quiet flex min-h-8 w-full items-center justify-between gap-2 border-0 px-2 text-left text-body"
                  title={`${e.name}: ${TEACHES_LABELS[e.teaches].toLowerCase()}, ${e.start} to ${e.goal} bpm${e.best ? `, your best ${e.best}` : ""}${e.last ? `, last practiced ${dayName(e.last)}` : ""}`}
                >
                  <span className="truncate">
                    {e.name}
                    {e.instrument === "bass" ? " · bass" : ""}
                    {e.stock ? "" : " (yours)"}
                  </span>
                  <span
                    className={`shrink-0 text-label tabular-nums ${loaded(e) ? "" : "text-ink-dim"}`}
                  >
                    {e.best ? `best ${e.best}` : `${e.start}–${e.goal}`}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>

        <div className="surface-panel scroll-area grid max-h-full min-h-0 grid-cols-[minmax(0,1fr)] content-start gap-3 p-3">
          <Lcd aria-live="polite" className="grid gap-1 py-2 text-center">
            <span className="block text-display tabular-nums">
              {frame.countIn > 0
                ? `Count ${frame.countIn}`
                : where && passBar
                  ? `Pass ${where.pass} · bar ${passBar} · ${place?.beat ?? 1}`
                  : "Ready"}
              {" · "}
              {tempo} bpm
            </span>
            <span className="block min-h-6 text-readout">
              Rung {rung} of {rungs.length} · from {exercise.tempo.start} to {exercise.tempo.goal}{" "}
              by {exercise.tempo.step}
              {rolling && place?.nextTempo
                ? ` · ${place.nextTempo.bpm} bpm at bar ${place.nextTempo.bar}`
                : ""}
            </span>
          </Lcd>
          <p className="m-0 text-ink-dim text-label">
            {practice.best ? `Best ${practice.best} bpm` : "No pass played yet"}
            {practice.last ? ` · last practiced ${dayName(practice.last.date)}` : ""}
            {practice.streak > 0
              ? ` · ${practice.streak} day${practice.streak === 1 ? "" : "s"} in a row`
              : ""}
            {practice.todayMinutes >= 1 ? ` · ${Math.round(practice.todayMinutes)} min today` : ""}
          </p>

          <section aria-labelledby={`${ids}-how`} className="grid gap-1">
            <h3 id={`${ids}-how`} className="caption m-0">
              {TEACHES_LABELS[exercise.teaches]}
            </h3>
            <p className="m-0 max-w-prose text-body">{exercise.instructions}</p>
          </section>

          <div className="flex min-w-0 flex-wrap items-start gap-3">
            {diagrams.length > 0 ? (
              <div className="surface-lcd flex shrink-0 items-start gap-3 p-1">
                {diagrams.map((d, i) => (
                  <ChordDiagram key={i === 0 ? "now" : "next"} diagram={d} coming={i > 0} />
                ))}
              </div>
            ) : null}
            <div className="min-w-0 flex-1">
              <Fretboard tab={practice.tab} tick={where ? where.tick : null} />
            </div>

            <TabStaff
              tab={practice.tab}
              tick={where ? where.tick : null}
              follow={rolling}
              label={`Tab of ${exercise.name}`}
            />
          </div>

          <div role="toolbar" aria-label="Transport" className="flex min-w-0 items-center gap-2">
            <Button
              tier="primary"
              aria-pressed={rolling}
              onClick={onPlay}
              title="Play or stop (Space)"
              className="min-w-28"
            >
              {rolling ? (
                <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
                  <path d="M3 3h10v10H3z" />
                </svg>
              ) : (
                <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
                  <path d="M4 2l10 6-10 6z" />
                </svg>
              )}
              {rolling ? "Stop" : "Play"}
            </Button>
            <Button
              aria-pressed={status.countIn && countBars > 0}
              disabled={countBars === 0}
              onClick={() => report(countInSet(!status.countIn))}
              title={
                countBars === 0
                  ? "This exercise starts without a count-in"
                  : `Count in ${countBars} bar${countBars === 1 ? "" : "s"} first`
              }
            >
              <Led on={status.countIn && countBars > 0} label="Count in" />
              Count in
            </Button>
            <Button
              aria-pressed={practice.looping}
              onClick={() => options({ looping: !practice.looping })}
              title="Play the phrase round and round, or once (L)"
            >
              <Led on={practice.looping} label="Loop" />
              Loop
            </Button>
            <Button
              aria-pressed={practice.groove}
              disabled={!exercise.drums}
              onClick={() => options({ groove: !practice.groove })}
              title={
                exercise.drums
                  ? `${exercise.drums.groove} drums, or only the click (G)`
                  : "This exercise has no groove; the click keeps time"
              }
            >
              <Led on={practice.groove} label="Groove" />
              Groove
            </Button>
            <Metronome metronome={status.metronome} onStatus={onStatus} onError={onError} />
          </div>

          <Field
            label="Tempo"
            hint={`↑ ↓ a rung of ${exercise.tempo.step} bpm, from the next bar`}
            htmlFor={`${ids}-bpm`}
          >
            <span className="flex w-full items-center gap-1">
              <Button
                aria-label="Slower"
                title="A rung down the ladder (↓)"
                onClick={() => report(jamTempo(stepTempo(exercise, tempo, -1)))}
                square
              >
                −
              </Button>
              <input
                key={tempo}
                id={`${ids}-bpm`}
                type="number"
                min={30}
                max={300}
                step={1}
                defaultValue={tempo}
                onKeyDown={(e) => {
                  if (e.key === "Enter") e.currentTarget.blur();
                }}
                onBlur={(e) => {
                  const bpm = Number(e.currentTarget.value);
                  if (Number.isFinite(bpm) && bpm !== tempo) report(jamTempo(bpm));
                }}
                className="surface-field min-h-8 w-16 min-w-0 flex-1 px-2 text-center tabular-nums"
              />
              <Button
                aria-label="Faster"
                title="A rung up the ladder (↑)"
                onClick={() => report(jamTempo(stepTempo(exercise, tempo, 1)))}
                square
              >
                +
              </Button>
            </span>
          </Field>

          <Select
            id={`${ids}-climb`}
            label="Climb"
            hint="Up a rung after this many passes, on the bar line"
            value={String(practice.climb)}
            onChange={(e) => options({ climb: Number(e.target.value) })}
          >
            {CLIMBS.map((n) => (
              <option key={n} value={n}>
                {n === 0 ? "Hold the tempo" : n === 1 ? "Every pass" : `Every ${n} passes`}
              </option>
            ))}
          </Select>
          <Select
            id={`${ids}-rig`}
            label="Your rig"
            hint={
              practice.rig
                ? "The example still plays through the exercise's rig"
                : "What the example and your input play through"
            }
            value={practice.rig ?? ""}
            onChange={(e) => options({ rig: e.target.value || null })}
          >
            <option value="">
              {ownRig ? `The exercise's: ${ownRig.name}` : "The exercise's (dry)"}
            </option>
            {rigs
              .filter((r) => rigId(r.path) !== exercise.rig)
              .map((r) => (
                <option key={r.path} value={rigId(r.path)}>
                  {r.name}
                  {r.stock ? "" : " (yours)"}
                </option>
              ))}
          </Select>
          {jam.missing.length > 0 || practice.exampleMissing.length > 0 ? (
            <p role="status" className="m-0 text-ink-dim text-label">
              Left out: {[...jam.missing, ...practice.exampleMissing].join("; ")}
            </p>
          ) : null}
        </div>
      </div>
    </section>
  );
}
