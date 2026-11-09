import { useEffect, useId, useMemo, useRef, useState } from "react";
import { isTyping } from "../lib/actions";
import {
  countInSet,
  type EngineStatus,
  type GrooveSection,
  type Jam,
  jamSection,
  jamSet,
  jamTempo,
  type MeterFrame,
} from "../lib/engine";
import {
  addTap,
  clampTempo,
  JAM_SECTIONS,
  jamPlace,
  SECTION_LABELS,
  TONICS,
  tapTempo,
} from "../lib/jam";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { Meter } from "./Meter";
import { Metronome } from "./Metronome";
import { Select } from "./Select";

type Props = {
  status: EngineStatus;
  frame: MeterFrame;
  /** Space, the play key, starts and stops the jam from the app's bindings. */
  onPlay: () => void;
  onLeave: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/** A groove from the project's own patterns rather than a stock genre. */
const PROJECT = "\u0000project";

/** The rig id a rig file names: its file name less `.rig.json`. */
function rigId(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.rig\.json$/, "");
}

/**
 * Jam mode: drums to play along with and your rig on the input, heard in
 * place of the project, which jamming never edits. Pick a groove, a kit,
 * the key and a rig; play; change section at the next bar and nudge the
 * tempo while it plays.
 *
 * Keys: Space plays and stops, 1 to 6 pick a section, ↑ and ↓ nudge the
 * tempo (⇧ by 5), T taps it.
 */
export function JamView({ status, frame, onPlay, onLeave, onStatus, onError }: Props) {
  const ids = useId();
  const jam = status.jam;
  const report = (action: Promise<EngineStatus>) =>
    action.then(onStatus, (e) => onError(String(e)));

  const genres = useMemo(
    () => [...new Set(status.grooves.map((g) => g.genre))].sort((a, b) => a.localeCompare(b)),
    [status.grooves],
  );
  const rigs = status.rigs;
  const rolling = frame.playing || frame.countIn > 0;

  const [taps, setTaps] = useState<number[]>([]);

  const place = jam
    ? jamPlace(
        frame.position,
        jam.marks,
        status.sampleRate,
        jam.barTicks,
        jam.setup.timeSignature.numerator,
      )
    : null;

  // The latest state for the key handler, which is installed once.
  const keys = useRef({ jam, rolling, taps, report, setTaps });
  keys.current = { jam, rolling, taps, report, setTaps };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as Element | null;
      if (isTyping(target) || target?.tagName === "SELECT") return;
      const k = keys.current;
      if (!k.jam) return;
      const n = Number(e.key);
      if (n >= 1 && n <= JAM_SECTIONS.length) {
        const section = JAM_SECTIONS[n - 1];
        if (section && k.jam.sections.includes(section)) k.report(jamSection(section));
      } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        const step = (e.shiftKey ? 5 : 1) * (e.key === "ArrowUp" ? 1 : -1);
        k.report(jamTempo(clampTempo(Math.round(k.jam.tempo) + step)));
      } else if (e.key === "t" || e.key === "T") {
        const next = addTap(k.taps, performance.now());
        k.setTaps(next);
        const bpm = tapTempo(next);
        if (bpm !== null) k.report(jamTempo(bpm));
      } else {
        return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!jam) {
    return (
      <section aria-label="Jam" className="grid flex-1 place-items-center text-ink-dim">
        Opening the jam…
      </section>
    );
  }

  const setup = jam.setup;
  const rig = rigs.find((r) => rigId(r.path) === setup.rig) ?? null;
  const change = (next: Partial<Jam>) => report(jamSet({ ...setup, ...next }));
  const fromProject = !setup.drums.groove;
  const patterns = status.project.patterns;
  const playingSection = rolling ? (place?.section ?? null) : null;
  const tempo = Math.round(jam.tempo * 10) / 10;

  const tap = () => {
    const next = addTap(taps, performance.now());
    setTaps(next);
    const bpm = tapTempo(next);
    if (bpm !== null) report(jamTempo(bpm));
  };

  return (
    <section
      aria-label="Jam"
      className="grid min-h-0 flex-1 gap-3 overflow-y-auto lg:grid-cols-[minmax(0,22rem)_1fr]"
    >
      <div className="surface-panel grid content-start gap-3 p-3">
        <div className="flex items-center justify-between gap-2">
          <h2 className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">Jam</h2>
          <Button onClick={onLeave} title="Back to the project">
            Leave
          </Button>
        </div>
        <Select
          id={`${ids}-groove`}
          label="Groove"
          hint="Drums for every section"
          value={fromProject ? PROJECT : (setup.drums.groove ?? "")}
          onChange={(e) => {
            const value = e.target.value;
            if (value === PROJECT) {
              const first = patterns[0];
              if (!first) return;
              change({ drums: { ...setup.drums, groove: null, patterns: { verse: first } } });
            } else {
              change({ drums: { ...setup.drums, groove: value, patterns: {} } });
            }
          }}
        >
          {genres.map((g) => (
            <option key={g} value={g}>
              {g}
            </option>
          ))}
          <option value={PROJECT} disabled={patterns.length === 0}>
            This project's patterns
          </option>
        </Select>
        {fromProject
          ? JAM_SECTIONS.map((section) => (
              <Select
                key={section}
                id={`${ids}-pattern-${section}`}
                label={SECTION_LABELS[section]}
                value={String(setup.drums.patterns?.[section]?.id ?? "")}
                onChange={(e) => {
                  const chosen = patterns.find((p) => String(p.id) === e.target.value);
                  const own = { ...setup.drums.patterns };
                  if (chosen) own[section] = chosen;
                  else delete own[section];
                  if (Object.keys(own).length === 0) return;
                  change({ drums: { ...setup.drums, patterns: own } });
                }}
              >
                <option value="">None</option>
                {patterns.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </Select>
            ))
          : null}
        <Select
          id={`${ids}-kit`}
          label="Kit"
          value={setup.drums.kit}
          onChange={(e) => change({ drums: { ...setup.drums, kit: e.target.value } })}
        >
          {status.kits.map((k) => (
            <option key={k.id} value={k.id}>
              {k.name}
            </option>
          ))}
        </Select>
        <fieldset className="m-0 grid grid-cols-[1fr_auto] items-center gap-3 border-0 p-0">
          <legend className="float-left p-0 text-sm">
            Key
            <small className="block text-ink-dim text-xs">What the backing plays in</small>
          </legend>
          <span className="flex gap-1">
            <select
              aria-label="Tonic"
              className="surface-lcd min-h-8 w-16 appearance-none py-0 pl-2 font-lcd text-base"
              value={setup.key.tonic}
              onChange={(e) => change({ key: { ...setup.key, tonic: e.target.value } })}
            >
              {TONICS.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
            <select
              aria-label="Mode"
              className="surface-lcd min-h-8 w-24 appearance-none py-0 pl-2 font-lcd text-base"
              value={setup.key.mode}
              onChange={(e) =>
                change({ key: { ...setup.key, mode: e.target.value as "major" | "minor" } })
              }
            >
              <option value="major">major</option>
              <option value="minor">minor</option>
            </select>
          </span>
        </fieldset>
        <Select
          id={`${ids}-rig`}
          label="Rig"
          hint="What your input plays through"
          value={setup.rig ?? ""}
          onChange={(e) => change({ rig: e.target.value || null })}
        >
          <option value="">None (dry)</option>
          {(["guitar", "bass"] as const).map((instrument) => (
            <optgroup key={instrument} label={instrument === "guitar" ? "Guitar" : "Bass"}>
              {rigs
                .filter((r) => r.instrument === instrument)
                .map((r) => (
                  <option key={r.path} value={rigId(r.path)}>
                    {r.name}
                    {r.stock ? "" : " (yours)"}
                  </option>
                ))}
            </optgroup>
          ))}
        </Select>
        {rig ? (
          <p className="m-0 text-ink-dim text-xs">
            {rig.chain.map((spec) => spec.replace(/^cab:.*/, "cab")).join(" → ")}
          </p>
        ) : null}
        {jam.missing.length > 0 ? (
          <p role="status" className="m-0 text-ink-dim text-xs">
            Left out: {jam.missing.join("; ")}
          </p>
        ) : null}
        <div className="grid gap-1">
          <Meter label="In" level={frame.input} />
          <Meter label="Out" level={frame.output} />
        </div>
      </div>

      <div className="surface-panel grid content-start gap-4 p-3">
        <Lcd aria-live="polite" className="grid gap-1 py-3 text-center">
          <span className="block text-4xl">
            {playingSection
              ? SECTION_LABELS[playingSection]
              : SECTION_LABELS[jam.marks[0]?.section ?? "verse"]}
          </span>
          <span className="block text-2xl tabular-nums">
            {frame.countIn > 0
              ? `Count ${frame.countIn}`
              : rolling && place
                ? `Bar ${place.bar} · ${place.beat}`
                : "Ready"}
            {" · "}
            {Math.round((rolling && place ? place.bpm : tempo) * 10) / 10} bpm
          </span>
          <span className="block min-h-6 text-base text-ink-dim">
            {rolling && place?.next
              ? `${SECTION_LABELS[place.next.section]} at bar ${place.next.bar}`
              : null}
            {rolling && place?.next && place?.nextTempo ? " · " : null}
            {rolling && place?.nextTempo
              ? `${place.nextTempo.bpm} bpm at bar ${place.nextTempo.bar}`
              : null}
            {rolling && !place?.next && jam.end !== null ? "Ending" : null}
          </span>
        </Lcd>

        <div className="flex flex-wrap items-center justify-center gap-2">
          <Button
            variant="transport"
            aria-pressed={rolling}
            onClick={onPlay}
            title="Play or stop (Space)"
            className="min-h-14 min-w-32 text-sm"
          >
            {rolling ? (
              <svg aria-hidden="true" viewBox="0 0 16 16" className="size-5 fill-current">
                <path d="M3 3h10v10H3z" />
              </svg>
            ) : (
              <svg aria-hidden="true" viewBox="0 0 16 16" className="size-5 fill-current">
                <path d="M4 2l10 6-10 6z" />
              </svg>
            )}
            {rolling ? "Stop" : "Play"}
          </Button>
          <Button
            variant="transport"
            aria-pressed={status.countIn}
            onClick={() => report(countInSet(!status.countIn))}
            title="Count in a bar before the jam starts"
          >
            <Led on={status.countIn} label="Count in" />
            Count in
          </Button>
          <Metronome metronome={status.metronome} onStatus={onStatus} onError={onError} />
        </div>

        <div className="flex flex-wrap items-center justify-center gap-2">
          <Button
            aria-label="Slower"
            title="Slower (↓, ⇧↓ by 5)"
            onClick={() => report(jamTempo(clampTempo(Math.round(tempo) - 1)))}
          >
            −
          </Button>
          <label
            htmlFor={`${ids}-bpm`}
            className="font-bold text-[11px] uppercase tracking-[0.08em]"
          >
            Tempo
          </label>
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
              if (Number.isFinite(bpm) && bpm !== tempo) report(jamTempo(clampTempo(bpm)));
            }}
            className="surface-lcd min-h-8 w-20 px-2 text-center font-lcd text-base tabular-nums"
          />
          <Button
            aria-label="Faster"
            title="Faster (↑, ⇧↑ by 5)"
            onClick={() => report(jamTempo(clampTempo(Math.round(tempo) + 1)))}
          >
            +
          </Button>
          <Button onClick={tap} title="Tap the beat (T)">
            Tap
          </Button>
        </div>

        <ul
          aria-label="Sections"
          className="m-0 grid list-none grid-cols-3 gap-2 p-0 sm:grid-cols-6"
        >
          {JAM_SECTIONS.map((section, i) => {
            const has = jam.sections.includes(section);
            const playing = playingSection === section;
            const coming = rolling && place?.next?.section === section;
            const bars = jam.bars[section];
            return (
              <li key={section}>
                <button
                  type="button"
                  disabled={!has}
                  aria-pressed={playing}
                  aria-label={`${SECTION_LABELS[section]}, key ${i + 1}`}
                  onClick={() => report(jamSection(section as GrooveSection))}
                  className="surface-pbutton grid min-h-20 w-full place-items-center gap-1 border-0 px-2 py-2 disabled:opacity-40"
                >
                  <Led on={playing} label={SECTION_LABELS[section]} />
                  <span className="font-bold text-[11px] uppercase tracking-[0.06em]">
                    {SECTION_LABELS[section]}
                  </span>
                  <span className="text-ink-dim text-xs">
                    {coming ? "next" : has && bars ? `${bars} bar${bars === 1 ? "" : "s"}` : "—"}
                    {" · "}
                    {i + 1}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        <p className="m-0 text-center text-ink-dim text-xs">
          Space plays · 1–6 sections at the next bar · ↑ ↓ tempo · T tap. Jamming never edits{" "}
          {status.project.name}.
        </p>
      </div>
    </section>
  );
}
