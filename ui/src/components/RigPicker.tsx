import { type FormEvent, type KeyboardEvent, useId, useMemo, useRef, useState } from "react";
import {
  type BrokenFile,
  chooseRigExportPath,
  chooseRigImport,
  type EngineStatus,
  type Instrument,
  type RigAudition,
  type RigInfo,
  rigAb,
  rigApply,
  rigAudition,
  rigDelete,
  rigExport,
  rigImport,
  rigRename,
  rigSave,
  type TrackStatus,
} from "../lib/engine";
import { chainSummary, filterRigs, rigGenres } from "../lib/rigs";
import { Button } from "./Button";
import { Led } from "./Led";
import { Select } from "./Select";

type Props = {
  /** The audio track rigs are heard on and applied to. */
  track: TrackStatus;
  rigs: RigInfo[];
  audition: RigAudition | null;
  /** Rig files in the Rigs folder that will not load. */
  broken: BrokenFile[];
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
  /** Starts open, for screenshots. */
  initiallyOpen?: boolean;
  /** The instrument the list starts on: the profile's. */
  defaultInstrument?: Instrument;
};

/** What the form under the list is doing, if anything. */
type Form =
  | { kind: "save"; name: string; genre: string; description: string; instrument: Instrument }
  | { kind: "rename"; rig: RigInfo; name: string }
  | { kind: "delete"; rig: RigInfo };

/**
 * The rig picker on the pedalboard: browse rigs by instrument and genre,
 * audition one on the live input in place of the chain, flip between it
 * (A) and the chain (B), and apply it, which replaces the chain as one
 * undoable edit. Save the chain as a rig of your own, rename, delete,
 * import and export rigs.
 *
 * The list is one tab stop: the arrow keys move through it, Space
 * auditions the rig (again stops), Return applies it, Escape stops.
 */
export function RigPicker({
  track,
  rigs,
  audition,
  broken,
  onStatus,
  onError,
  initiallyOpen = false,
  defaultInstrument = "guitar",
}: Props) {
  const id = useId();
  const [open, setOpen] = useState(initiallyOpen);
  const [instrument, setInstrument] = useState<Instrument>(defaultInstrument);
  const [genre, setGenre] = useState("");
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [form, setForm] = useState<Form | null>(null);
  const list = useRef<HTMLDivElement>(null);

  const genres = useMemo(() => rigGenres(rigs, instrument), [rigs, instrument]);
  const shown = filterRigs(rigs, instrument, genre, query);
  const current = shown[Math.min(active, shown.length - 1)];
  const here = audition?.track === track.id ? audition : null;
  const heard = here ? rigs.find((r) => r.path === here.path) : undefined;

  const fail = (e: unknown) => onError(String(e));
  const run = (p: Promise<EngineStatus>) => p.then(onStatus, fail);
  const listen = (r: RigInfo) =>
    run(here?.path === r.path ? rigAudition(null, null) : rigAudition(track.id, r.path));
  const stop = () => {
    if (here) run(rigAudition(null, null));
  };
  const apply = (r: RigInfo) => run(rigApply(track.id, r.path));
  const exportRig = (r: RigInfo) =>
    chooseRigExportPath(r.name).then((to) => (to ? rigExport(r.path, to) : undefined), fail);
  const importRig = () =>
    chooseRigImport().then((path) => (path ? run(rigImport(path)) : undefined), fail);

  const move = (to: number) => {
    const next = Math.max(0, Math.min(shown.length - 1, to));
    setActive(next);
    list.current
      ?.querySelector<HTMLElement>(`[data-index="${next}"]`)
      ?.scrollIntoView({ block: "nearest" });
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    switch (e.key) {
      case "ArrowDown":
        move(active + 1);
        break;
      case "ArrowUp":
        move(active - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(shown.length - 1);
        break;
      case " ":
        if (current) listen(current);
        break;
      case "Enter":
        if (current) apply(current);
        break;
      case "Escape":
        if (!here) return;
        stop();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!form) return;
    const done = (s: EngineStatus) => {
      onStatus(s);
      setForm(null);
    };
    if (form.kind === "save") {
      const { kind: _, ...draft } = form;
      const replace = rigs.some(
        (r) => !r.stock && r.name.toLowerCase() === draft.name.trim().toLowerCase(),
      );
      rigSave(track.id, { ...draft, replace }).then(done, fail);
    } else if (form.kind === "rename") {
      rigRename(form.rig.path, form.name).then(done, fail);
    } else {
      rigDelete(form.rig.path).then(done, fail);
    }
  };

  const audit = here ? (
    <fieldset className="surface-lcd m-0 flex flex-wrap items-center gap-x-3 gap-y-1 border-0 px-2 py-1">
      <legend className="sr-only">Audition</legend>
      <Led on label="Auditioning" />
      <span className="lcd-text font-lcd text-readout">
        Auditioning <strong className="font-normal">{heard?.name ?? "a rig"}</strong>
      </span>
      <fieldset className="m-0 flex gap-1 border-0 p-0">
        <legend className="sr-only">Hear</legend>
        <Button
          aria-pressed={here.hearingRig}
          onClick={() => run(rigAb(true))}
          title="Hear the rig (A)"
          className="min-h-7 px-2"
        >
          A: rig
        </Button>
        <Button
          aria-pressed={!here.hearingRig}
          onClick={() => run(rigAb(false))}
          title="Hear the track's own chain (B)"
          className="min-h-7 px-2"
        >
          B: your chain
        </Button>
      </fieldset>
      {heard ? (
        <Button onClick={() => apply(heard)} className="min-h-7 px-2">
          Apply
        </Button>
      ) : null}
      <Button onClick={stop} title="Stop auditioning (Esc)" className="min-h-7 px-2">
        Stop
      </Button>
      {here.missing.length > 0 ? (
        <p className="lcd-text m-0 w-full font-lcd text-body">
          Left out: {here.missing.join("; ")}
        </p>
      ) : null}
    </fieldset>
  ) : null;

  return (
    <section aria-label="Rigs" className="grid gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          aria-expanded={open}
          aria-controls={`${id}-panel`}
          onClick={() => setOpen(!open)}
          className="px-2"
        >
          <svg
            aria-hidden="true"
            viewBox="0 0 16 16"
            className={`size-3.5 fill-current ${open ? "" : "-rotate-90"}`}
          >
            <path d="M2 5l6 6 6-6z" />
          </svg>
          Rigs
        </Button>
        {open ? null : (
          <span className="text-ink-dim text-body">
            Pick a ready-made chain for a style, hear it, and apply it.
          </span>
        )}
        {audit}
      </div>
      {open ? (
        <div id={`${id}-panel`} className="grid gap-2">
          <div className="flex flex-wrap items-end gap-x-4 gap-y-2">
            <Select
              id={`${id}-instrument`}
              label="Instrument"
              value={instrument}
              onChange={(e) => {
                setInstrument(e.target.value as Instrument);
                setGenre("");
                setActive(0);
              }}
              className="min-w-28"
            >
              <option value="guitar">Guitar</option>
              <option value="bass">Bass</option>
            </Select>
            <Select
              id={`${id}-genre`}
              label="Genre"
              value={genre}
              onChange={(e) => {
                setGenre(e.target.value);
                setActive(0);
              }}
              className="min-w-36"
            >
              <option value="">All genres</option>
              {genres.map((g) => (
                <option key={g} value={g}>
                  {g}
                </option>
              ))}
            </Select>
            <label htmlFor={`${id}-search`} className="sr-only">
              Search rigs
            </label>
            <input
              id={`${id}-search`}
              type="search"
              placeholder="Search rigs"
              autoComplete="off"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              className="surface-field min-h-8 w-44 px-3 "
            />
            <span className="ml-auto flex gap-2">
              <Button
                disabled={track.chain.length === 0}
                onClick={() =>
                  setForm({ kind: "save", name: "", genre: "", description: "", instrument })
                }
              >
                Save chain as rig…
              </Button>
              <Button onClick={importRig}>Import…</Button>
            </span>
          </div>
          <div
            ref={list}
            role="listbox"
            tabIndex={0}
            aria-label={`Rigs: ${shown.length} of ${rigs.length}`}
            aria-activedescendant={current ? `${id}-r-${shown.indexOf(current)}` : undefined}
            onKeyDown={onKey}
            className="surface-lcd max-h-56 overflow-y-auto p-1"
          >
            {shown.length === 0 ? (
              <p className="m-0 px-2 py-1 font-lcd text-readout text-lcd-dim">
                <span className="lcd-text">No rigs match.</span>
              </p>
            ) : null}
            {shown.map((r, i) => {
              const isHeard = here?.path === r.path;
              const isActive = current === r;
              return (
                // biome-ignore lint/a11y/useKeyWithClickEvents: the listbox handles the keys for its options
                <div
                  key={r.path}
                  id={`${id}-r-${i}`}
                  data-index={i}
                  role="option"
                  aria-selected={isActive}
                  aria-description={r.description}
                  tabIndex={-1}
                  onClick={() => setActive(i)}
                  onDoubleClick={() => apply(r)}
                  className={`grid grid-cols-[auto_minmax(8rem,12rem)_1fr_auto] items-center gap-2 rounded px-2 py-0.5 font-lcd text-readout ${isActive ? "outline-2 outline-focus outline-solid -outline-offset-2" : ""}`}
                >
                  <Led on={isHeard} label={isHeard ? "Auditioning" : "Not auditioning"} />
                  <span className="lcd-text truncate">
                    {r.name}
                    <span className="block text-body">
                      {r.genre}
                      {r.stock ? "" : " · yours"}
                    </span>
                  </span>
                  <span className="lcd-text min-w-0 text-body">
                    <span className="line-clamp-2">{r.description}</span>
                    <span className="block truncate opacity-80">{chainSummary(r)}</span>
                  </span>
                  <span className="flex gap-1">
                    <Button
                      tabIndex={-1}
                      aria-pressed={isHeard}
                      aria-label={`Audition ${r.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setActive(i);
                        listen(r);
                      }}
                      className="min-h-7 px-2"
                    >
                      Audition
                    </Button>
                    <Button
                      tabIndex={-1}
                      aria-label={`Apply ${r.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        apply(r);
                      }}
                      className="min-h-7 px-2"
                    >
                      Apply
                    </Button>
                    <Button
                      tabIndex={-1}
                      aria-label={`Export ${r.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        exportRig(r);
                      }}
                      className="min-h-7 px-2"
                    >
                      Export
                    </Button>
                    {r.stock ? null : (
                      <>
                        <Button
                          tabIndex={-1}
                          aria-label={`Rename ${r.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setForm({ kind: "rename", rig: r, name: r.name });
                          }}
                          className="min-h-7 px-2"
                        >
                          Rename
                        </Button>
                        <Button
                          tabIndex={-1}
                          aria-label={`Delete ${r.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setForm({ kind: "delete", rig: r });
                          }}
                          className="min-h-7 px-2"
                        >
                          Delete
                        </Button>
                      </>
                    )}
                  </span>
                </div>
              );
            })}
          </div>
          {broken.length > 0 ? (
            <ul aria-label="Rig files that will not load" className="surface-lcd m-0 list-none p-1">
              {broken.map((b) => (
                <li key={b.path} className="px-2 py-0.5 font-lcd text-readout">
                  <span className="lcd-text text-lcd-error">{b.name}</span>
                  <span className="lcd-text text-lcd-dim"> · will not load: {b.reason}</span>
                </li>
              ))}
            </ul>
          ) : null}
          {form ? (
            <form
              onSubmit={submit}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setForm(null);
                }
              }}
              aria-label={
                form.kind === "save"
                  ? "Save chain as rig"
                  : form.kind === "rename"
                    ? `Rename ${form.rig.name}`
                    : `Delete ${form.rig.name}`
              }
              className="surface-lcd flex flex-wrap items-end gap-x-3 gap-y-2 p-2"
            >
              {form.kind === "delete" ? (
                <p className="lcd-text m-0 font-lcd text-readout">
                  Delete the rig {form.rig.name}? Its file goes from your Rigs folder.
                </p>
              ) : (
                <label className="lcd-text grid gap-1 font-lcd text-body">
                  Name
                  <input
                    // biome-ignore lint/a11y/noAutofocus: the form opens to be typed in
                    autoFocus
                    required
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                    className="surface-field min-h-8 w-48 px-2"
                  />
                </label>
              )}
              {form.kind === "save" ? (
                <>
                  <label className="lcd-text grid gap-1 font-lcd text-body">
                    Genre
                    <input
                      value={form.genre}
                      list={`${id}-genres`}
                      onChange={(e) => setForm({ ...form, genre: e.target.value })}
                      className="surface-field min-h-8 w-36 px-2"
                    />
                    <datalist id={`${id}-genres`}>
                      {rigGenres(rigs, form.instrument).map((g) => (
                        <option key={g} value={g} />
                      ))}
                    </datalist>
                  </label>
                  <label className="lcd-text grid gap-1 font-lcd text-body">
                    Instrument
                    <select
                      value={form.instrument}
                      onChange={(e) =>
                        setForm({ ...form, instrument: e.target.value as Instrument })
                      }
                      className="surface-field min-h-8 px-2"
                    >
                      <option value="guitar">Guitar</option>
                      <option value="bass">Bass</option>
                    </select>
                  </label>
                  <label className="lcd-text grid min-w-64 flex-1 gap-1 font-lcd text-body">
                    What it sounds like
                    <input
                      value={form.description}
                      onChange={(e) => setForm({ ...form, description: e.target.value })}
                      className="surface-field min-h-8 px-2"
                    />
                  </label>
                </>
              ) : null}
              <span className="flex gap-2">
                <Button type="submit">
                  {form.kind === "save" ? "Save" : form.kind === "rename" ? "Rename" : "Delete"}
                </Button>
                <Button onClick={() => setForm(null)}>Cancel</Button>
              </span>
            </form>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
