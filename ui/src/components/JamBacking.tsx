import { useEffect, useId, useState } from "react";
import {
  type JamBacking as Backing,
  chooseAudioPaths,
  type Instrument,
  type JamPreset,
  type JamRhythm,
  type JamTrack,
  jamTracks,
  projectList,
  type RigInfo,
} from "../lib/engine";
import { Button } from "./Button";
import { Select } from "./Select";

type Props = {
  backing: Backing;
  presets: JamPreset[];
  rigs: RigInfo[];
  /** The open project, offered first for a track. */
  project: string;
  missing: string[];
  onChange: (backing: Backing) => void;
  onError: (message: string) => void;
};

const RHYTHMS: { id: JamRhythm; label: string }[] = [
  { id: "whole", label: "Once a chord" },
  { id: "half", label: "Half notes" },
  { id: "quarter", label: "Every beat" },
  { id: "eighth", label: "Eighths" },
];

/** The rig id a rig file names: its file name less `.rig.json`. */
export function rigId(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.rig\.json$/, "");
}

/** What the track picker says under its label: how long and how fast. */
function trackHint(tracks: JamTrack[], name: string): { hint?: string } {
  const t = tracks.find((t) => t.name === name);
  return t ? { hint: `${Math.round(t.seconds)} s at ${t.bpm} bpm` } : {};
}

/** The level slider, in dB, for any backing. */
function Level({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number;
  onChange: (db: number) => void;
}) {
  const [level, setLevel] = useState(value);
  useEffect(() => setLevel(value), [value]);
  return (
    <div className="grid grid-cols-[1fr_auto] items-center gap-3">
      <label htmlFor={id} className="text-body">
        Level
      </label>
      <span className="flex items-center gap-2">
        <input
          id={id}
          type="range"
          min={-30}
          max={6}
          step={0.5}
          value={level}
          onChange={(e) => setLevel(Number(e.target.value))}
          onPointerUp={() => onChange(level)}
          onKeyUp={() => onChange(level)}
          className="fader-slim w-32"
        />
        <span className="w-14 text-right text-body tabular-nums">{level.toFixed(1)} dB</span>
      </span>
    </div>
  );
}

/**
 * What plays with the jam's drums: nothing, a chord progression played by
 * a built-in guitar or bass through a rig, an audio file, or a project's
 * track. Audio sets the jam's tempo.
 */
export function JamBacking({ backing, presets, rigs, project, missing, onChange, onError }: Props) {
  const ids = useId();
  const [projects, setProjects] = useState<string[]>([]);
  const [tracks, setTracks] = useState<JamTrack[]>([]);
  const chosenProject = backing.kind === "track" ? backing.project : null;

  useEffect(() => {
    if (backing.kind !== "track") return;
    projectList().then(setProjects, (e) => onError(String(e)));
  }, [backing.kind, onError]);
  useEffect(() => {
    if (chosenProject === null) return;
    jamTracks(chosenProject).then(setTracks, (e) => onError(String(e)));
  }, [chosenProject, onError]);

  const pick = (kind: Backing["kind"]) => {
    if (kind === backing.kind) return;
    if (kind === "none") onChange({ kind: "none" });
    else if (kind === "progression") {
      const bass = rigs.find((r) => rigId(r.path) === "bass-clean");
      onChange({
        kind: "progression",
        chords: presets[0]?.chords ?? "I | IV | V | IV",
        instrument: "bass",
        rhythm: "quarter",
        rig: bass ? "bass-clean" : null,
        levelDb: 0,
      });
    } else if (kind === "audio") {
      chooseAudioPaths().then(([file]) => {
        if (file) onChange({ kind: "audio", file, bpm: 120, levelDb: 0 });
      }, onError);
    } else {
      jamTracks(project).then((found) => {
        const first = found[0];
        if (first) onChange({ kind: "track", project, track: first.name, levelDb: 0 });
        else onError(`${project} has no audio track to play; pick another project`);
      }, onError);
    }
  };

  const genres = [...new Set(presets.map((p) => p.genre))];

  return (
    <fieldset className="m-0 grid gap-x-6 gap-y-3 border-0 p-0 md:grid-cols-2">
      <legend className="sr-only">Backing</legend>
      <Select
        id={`${ids}-kind`}
        label="Backing"
        hint="What plays with the drums"
        value={backing.kind}
        onChange={(e) => pick(e.target.value as Backing["kind"])}
      >
        <option value="none">Drums only</option>
        <option value="progression">Chord progression</option>
        <option value="audio">Audio file…</option>
        <option value="track">A project's track</option>
      </Select>

      {backing.kind === "progression" ? (
        <>
          <Select
            id={`${ids}-preset`}
            label="Progression"
            value={presets.find((p) => p.chords === backing.chords)?.chords ?? ""}
            onChange={(e) => {
              if (e.target.value) onChange({ ...backing, chords: e.target.value });
            }}
          >
            <option value="">Your own</option>
            {genres.map((genre) => (
              <optgroup key={genre} label={genre}>
                {presets
                  .filter((p) => p.genre === genre)
                  .map((p) => (
                    <option key={`${genre}-${p.name}`} value={p.chords}>
                      {p.name}
                    </option>
                  ))}
              </optgroup>
            ))}
          </Select>
          <label className="grid gap-1 text-body md:col-span-2" htmlFor={`${ids}-chords`}>
            Chords
            <input
              key={backing.chords}
              id={`${ids}-chords`}
              type="text"
              spellCheck={false}
              defaultValue={backing.chords}
              placeholder="I | IV | V | IV"
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
              onBlur={(e) => {
                const chords = e.currentTarget.value.trim();
                if (chords && chords !== backing.chords) onChange({ ...backing, chords });
              }}
              className="surface-field min-h-8 px-2"
            />
            <small className="text-ink-dim text-label">
              Numerals in the key (I, bVII, vi7) or names (Am, F, C/E); | between bars, - holds.
            </small>
          </label>
          <Select
            id={`${ids}-voice`}
            label="Played by"
            value={backing.instrument}
            onChange={(e) => {
              const instrument = e.target.value as Instrument;
              const rig = rigs.find((r) => rigId(r.path) === backing.rig);
              const keep = rig && rig.instrument === instrument;
              onChange({ ...backing, instrument, rig: keep ? (backing.rig ?? null) : null });
            }}
          >
            <option value="guitar">Guitar</option>
            <option value="bass">Bass</option>
          </Select>
          <Select
            id={`${ids}-rhythm`}
            label="Rhythm"
            value={backing.rhythm}
            onChange={(e) => onChange({ ...backing, rhythm: e.target.value as JamRhythm })}
          >
            {RHYTHMS.map((r) => (
              <option key={r.id} value={r.id}>
                {r.label}
              </option>
            ))}
          </Select>
          <Select
            id={`${ids}-rig`}
            label="Through"
            value={backing.rig ?? ""}
            onChange={(e) => onChange({ ...backing, rig: e.target.value || null })}
          >
            <option value="">No rig (dry)</option>
            {rigs
              .filter((r) => r.instrument === backing.instrument)
              .map((r) => (
                <option key={r.path} value={rigId(r.path)}>
                  {r.name}
                </option>
              ))}
          </Select>
        </>
      ) : null}

      {backing.kind === "audio" ? (
        <>
          <div className="grid grid-cols-[1fr_auto] items-center gap-3">
            <span className="min-w-0 truncate text-body" title={backing.file}>
              {backing.file.split("/").pop()}
            </span>
            <Button
              onClick={() =>
                chooseAudioPaths().then(([file]) => {
                  if (file) onChange({ ...backing, file });
                }, onError)
              }
            >
              Choose…
            </Button>
          </div>
          <div className="grid grid-cols-[1fr_auto] items-center gap-3">
            <label htmlFor={`${ids}-file-bpm`} className="text-body">
              Its tempo
              <small className="block text-ink-dim text-label">The drums play at it</small>
            </label>
            <input
              key={backing.bpm}
              id={`${ids}-file-bpm`}
              type="number"
              min={30}
              max={300}
              step={0.1}
              defaultValue={backing.bpm}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
              onBlur={(e) => {
                const bpm = Number(e.currentTarget.value);
                if (Number.isFinite(bpm) && bpm !== backing.bpm) onChange({ ...backing, bpm });
              }}
              className="surface-field min-h-8 w-20 px-2 text-center tabular-nums"
            />
          </div>
        </>
      ) : null}

      {backing.kind === "track" ? (
        <>
          <Select
            id={`${ids}-project`}
            label="Project"
            value={backing.project}
            onChange={(e) => {
              const next = e.target.value;
              jamTracks(next).then((found) => {
                const first = found[0];
                if (first) onChange({ ...backing, project: next, track: first.name });
                else onError(`${next} has no audio track to play`);
              }, onError);
            }}
          >
            {(projects.includes(backing.project) ? projects : [backing.project, ...projects]).map(
              (p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ),
            )}
          </Select>
          <Select
            id={`${ids}-track`}
            label="Track"
            {...trackHint(tracks, backing.track)}
            value={backing.track}
            onChange={(e) => onChange({ ...backing, track: e.target.value })}
          >
            {(tracks.some((t) => t.name === backing.track)
              ? tracks
              : [{ name: backing.track, bpm: 0, seconds: 0 }, ...tracks]
            ).map((t) => (
              <option key={t.name} value={t.name}>
                {t.name}
              </option>
            ))}
          </Select>
        </>
      ) : null}

      {backing.kind !== "none" ? (
        <Level
          id={`${ids}-level`}
          value={backing.levelDb}
          onChange={(levelDb) => onChange({ ...backing, levelDb })}
        />
      ) : null}
      {missing.length > 0 ? (
        <p role="status" className="m-0 text-ink-dim text-label">
          Left out: {missing.join("; ")}
        </p>
      ) : null}
    </fieldset>
  );
}
