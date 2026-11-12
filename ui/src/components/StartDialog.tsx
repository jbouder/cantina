import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import {
  type EngineStatus,
  type ProjectStart,
  projectNew,
  projectOpen,
  projectRecent,
  type RecentProject,
} from "../lib/engine";
import { defaultProject, METERS, SAMPLE_RATES, STARTS, savedLabel } from "../lib/start";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  /** The project open now: at launch the last one, or a new Untitled. */
  current: string;
  /**
   * Shown at launch, where closing it opens the most recent project;
   * otherwise New project opened it, and closing it keeps `current`.
   */
  atLaunch: boolean;
  /** Whether `current` has unsaved edits, which another project drops. */
  dirty: boolean;
  /** The app's sample rate, the new project's default. */
  sampleRate: number;
  /** Whether a drum kit is installed, for "Guitar and drums". */
  hasKits: boolean;
  onStatus: (status: EngineStatus) => void;
  /** Closes the dialog on the project already open. */
  onClose: () => void;
  /** Into the jam view. */
  onJam: () => void;
};

const heading = "caption m-0";

/**
 * What the app shows at launch and for New project: start a new project
 * with its rate, tempo, meter and tracks; open a recent one; or go
 * straight into a jam. At launch Escape picks the most recent project, or
 * the new Untitled the app opened when there is none; from New project it
 * keeps the open project.
 */
export function StartDialog({
  current,
  atLaunch,
  dirty,
  sampleRate,
  hasKits,
  onStatus,
  onClose,
  onJam,
}: Props) {
  const [recent, setRecent] = useState<RecentProject[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [rate, setRate] = useState(() =>
    SAMPLE_RATES.includes(sampleRate as (typeof SAMPLE_RATES)[number]) ? sampleRate : 48_000,
  );
  const [bpm, setBpm] = useState("120");
  const [meter, setMeter] = useState(0);
  const [start, setStart] = useState<ProjectStart>("guitar");
  const recentLabel = useId();
  const now = Date.now();

  useEffect(() => {
    projectRecent().then(setRecent, (e) => {
      setRecent([]);
      setError(String(e));
    });
  }, []);
  // A new project comes first: its name has focus, so typing one and
  // pressing Return creates it.
  useEffect(() => {
    document.getElementById("start-name")?.focus();
  }, []);

  const finish = useCallback(
    (action: Promise<EngineStatus>) =>
      action.then(
        (s) => {
          onStatus(s);
          onClose();
        },
        (e) => setError(String(e)),
      ),
    [onStatus, onClose],
  );

  const open = useCallback(
    (project: string) => {
      if (project === current) onClose();
      else finish(projectOpen(project));
    },
    [current, onClose, finish],
  );

  // Escape, the close button and the scrim: the default.
  const pickDefault = useCallback(() => {
    const project = atLaunch && recent ? defaultProject(recent) : null;
    if (project === null) onClose();
    else open(project);
  }, [atLaunch, recent, open, onClose]);

  const tempo = Number(bpm);
  const tempoOk = Number.isFinite(tempo) && tempo >= 20 && tempo <= 300;
  // Create stays enabled, the dialog's main action: with no name it asks
  // for one instead.
  const [nameMissing, setNameMissing] = useState(false);
  const create = (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setNameMissing(true);
      document.getElementById("start-name")?.focus();
      return;
    }
    if (!tempoOk) return;
    finish(
      projectNew(name, {
        sampleRate: rate,
        tempo: { bpm: tempo, ...(METERS[meter] ?? { numerator: 4, denominator: 4 }) },
        start,
      }),
    );
  };

  return (
    <Sheet
      title={atLaunch ? "Welcome to Cantina" : "Start a project"}
      onClose={pickDefault}
      wide
      footer={
        <>
          <Button
            onClick={onJam}
            title="Jam over a drum loop and a backing track, with no project to set up"
            className="mr-auto"
          >
            Jam session
          </Button>
          {atLaunch ? null : <Button onClick={onClose}>Cancel</Button>}
          <Button type="submit" form="start-new-form" tier="primary" disabled={!tempoOk}>
            Create
          </Button>
        </>
      }
    >
      {error ? (
        <Lcd role="alert" error className="text-readout">
          {error}
        </Lcd>
      ) : null}
      {dirty ? (
        <Lcd role="status" className="text-readout">
          {current} has unsaved edits. Another project drops them; save first to keep them.
        </Lcd>
      ) : null}
      <div className="grid gap-4 md:grid-cols-2">
        <section aria-labelledby="start-new-label" className="grid content-start gap-2">
          <h3 id="start-new-label" className={heading}>
            New project
          </h3>
          <form id="start-new-form" onSubmit={create} className="grid gap-2">
            <TextInput
              id="start-name"
              label="Name"
              {...(nameMissing ? { hint: "Name the project to create it" } : {})}
              aria-invalid={nameMissing}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setNameMissing(false);
              }}
              autoComplete="off"
            />
            <Select
              id="start-rate"
              label="Sample rate"
              value={rate}
              onChange={(e) => setRate(Number(e.target.value))}
            >
              {SAMPLE_RATES.map((r) => (
                <option key={r} value={r}>
                  {(r / 1000).toFixed(1)} kHz
                </option>
              ))}
            </Select>
            <TextInput
              id="start-bpm"
              label="Tempo"
              hint={tempoOk ? "BPM" : "20 to 300 BPM"}
              inputMode="decimal"
              value={bpm}
              aria-invalid={!tempoOk}
              onChange={(e) => setBpm(e.target.value)}
              autoComplete="off"
            />
            <Select
              id="start-meter"
              label="Time signature"
              value={meter}
              onChange={(e) => setMeter(Number(e.target.value))}
            >
              {METERS.map((m, i) => (
                <option key={`${m.numerator}/${m.denominator}`} value={i}>
                  {m.numerator}/{m.denominator}
                </option>
              ))}
            </Select>
            <Select
              id="start-tracks"
              label="Start with"
              {...(hasKits ? {} : { hint: "No drum kits installed" })}
              value={start}
              onChange={(e) => setStart(e.target.value as ProjectStart)}
            >
              {STARTS.map((s) => (
                <option key={s.id} value={s.id} disabled={s.id === "guitarDrums" && !hasKits}>
                  {s.label}
                </option>
              ))}
            </Select>
          </form>
        </section>
        <section aria-labelledby={recentLabel} className="grid content-start gap-2">
          <h3 id={recentLabel} className={heading}>
            Open a recent project
          </h3>
          <ul
            aria-labelledby={recentLabel}
            className="surface-lcd m-0 grid max-h-64 list-none content-start gap-0.5 overflow-y-auto p-1"
          >
            {recent === null ? (
              <li className="px-2 py-1 font-lcd text-readout">...</li>
            ) : recent.length === 0 ? (
              <li className="lcd-text px-2 py-1 font-lcd text-readout">No projects yet</li>
            ) : (
              recent.map((r) => {
                const when = savedLabel(r, now);
                return (
                  <li key={r.name}>
                    <button
                      type="button"
                      data-start-recent
                      aria-disabled={r.missing ? "true" : undefined}
                      aria-description={when}
                      onClick={() => {
                        if (r.missing) setError(`${r.name} has been moved or deleted.`);
                        else open(r.name);
                      }}
                      className={`grid min-h-11 w-full border-0 bg-transparent px-2 py-1 text-left font-lcd text-lcd-fg focus-visible:bg-select`}
                    >
                      <span className="lcd-text truncate text-readout leading-tight">{r.name}</span>
                      <span
                        className={`lcd-text text-body leading-tight ${r.missing ? "text-lcd-error" : ""}`}
                      >
                        {when}
                      </span>
                    </button>
                  </li>
                );
              })
            )}
          </ul>
        </section>
      </div>
    </Sheet>
  );
}
