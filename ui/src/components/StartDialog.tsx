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
  /** The project the app opened at launch: the last one, or a new Untitled. */
  current: string;
  /** The app's sample rate, the new project's default. */
  sampleRate: number;
  /** Whether a drum kit is installed, for "Guitar and drums". */
  hasKits: boolean;
  onStatus: (status: EngineStatus) => void;
  /** Closes the dialog on the project already open. */
  onClose: () => void;
  /** File ▸ Open…: the Projects sheet, for a project not in the list. */
  onBrowse: () => void;
  /** Into the jam view. */
  onJam: () => void;
};

const heading = "m-0 font-bold text-[11px] uppercase tracking-[0.1em]";

/**
 * What the app shows at launch: start a new project with its rate, tempo,
 * meter and tracks; open a recent one; or go straight into a jam. Escape
 * picks the most recent project, or the new Untitled the app opened when
 * there is none.
 */
export function StartDialog({
  current,
  sampleRate,
  hasKits,
  onStatus,
  onClose,
  onBrowse,
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
  // Return opens the most recent project: it has focus once the list is
  // in. With nothing to reopen, the new project's name has it.
  useEffect(() => {
    if (recent === null) return;
    const first = document.querySelector<HTMLElement>("[data-start-recent]:not([aria-disabled])");
    (first ?? document.getElementById("start-name"))?.focus();
  }, [recent]);

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
    const project = recent ? defaultProject(recent) : null;
    if (project === null) onClose();
    else open(project);
  }, [recent, open, onClose]);

  const tempo = Number(bpm);
  const tempoOk = Number.isFinite(tempo) && tempo >= 20 && tempo <= 300;
  const create = (e: FormEvent) => {
    e.preventDefault();
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
    <Sheet title="Welcome to Cantina" onClose={pickDefault} wide>
      {error ? (
        <Lcd role="alert" error className="text-base">
          {error}
        </Lcd>
      ) : null}
      <div className="grid gap-4 md:grid-cols-2">
        <section aria-labelledby={recentLabel} className="grid content-start gap-2">
          <h3 id={recentLabel} className={heading}>
            Open a recent project
          </h3>
          <ul
            aria-labelledby={recentLabel}
            className="surface-lcd m-0 grid max-h-64 list-none content-start gap-0.5 overflow-y-auto p-1"
          >
            {recent === null ? (
              <li className="px-2 py-1 font-lcd text-lg">...</li>
            ) : recent.length === 0 ? (
              <li className="lcd-text px-2 py-1 font-lcd text-lg">No projects yet</li>
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
                      <span className="lcd-text truncate text-lg leading-tight">{r.name}</span>
                      <span
                        className={`lcd-text text-sm leading-tight ${r.missing ? "text-lcd-error" : ""}`}
                      >
                        {when}
                      </span>
                    </button>
                  </li>
                );
              })
            )}
          </ul>
          <div className="flex">
            <Button onClick={onBrowse}>Open…</Button>
          </div>
        </section>
        <section aria-labelledby="start-new-label" className="grid content-start gap-2">
          <h3 id="start-new-label" className={heading}>
            New project
          </h3>
          <form onSubmit={create} className="grid gap-2">
            <TextInput
              id="start-name"
              label="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="off"
              className="min-w-36"
            />
            <Select
              id="start-rate"
              label="Sample rate"
              value={rate}
              onChange={(e) => setRate(Number(e.target.value))}
              className="min-w-36"
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
              className="min-w-36"
            />
            <Select
              id="start-meter"
              label="Time signature"
              value={meter}
              onChange={(e) => setMeter(Number(e.target.value))}
              className="min-w-36"
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
              className="min-w-36"
            >
              {STARTS.map((s) => (
                <option key={s.id} value={s.id} disabled={s.id === "guitarDrums" && !hasKits}>
                  {s.label}
                </option>
              ))}
            </Select>
            <div className="flex justify-end">
              <Button type="submit" variant="transport" disabled={!name.trim() || !tempoOk}>
                Create
              </Button>
            </div>
          </form>
        </section>
      </div>
      <section
        aria-labelledby="start-jam-label"
        className="flex flex-wrap items-center gap-3 border-0 border-ink-dim border-t border-solid pt-3"
      >
        <div className="min-w-0 flex-1">
          <h3 id="start-jam-label" className={heading}>
            Jam session
          </h3>
          <p className="m-0 text-ink-dim text-sm">
            Play over a drum loop and a backing track, nothing to set up first.
          </p>
        </div>
        <Button variant="transport" onClick={onJam}>
          Jam
        </Button>
      </section>
    </Sheet>
  );
}
