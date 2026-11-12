import { useEffect, useId, useMemo, useRef, useState } from "react";
import { isTyping } from "../lib/actions";
import {
  countInSet,
  type EngineStatus,
  type GrooveSection,
  type Jam,
  jamCaptureSet,
  jamKeep,
  jamLoad,
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
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { Field } from "./Field";
import { JamBacking, rigId } from "./JamBacking";
import { KeepSheet } from "./KeepSheet";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { Metronome } from "./Metronome";
import { SaveJamSheet } from "./SaveJamSheet";
import { Select } from "./Select";

type Props = {
  status: EngineStatus;
  frame: MeterFrame;
  /** Space, the play key, starts and stops the jam from the app's bindings. */
  onPlay: () => void;
  /** Plays the jam on stage: performance mode with the jam's controls. */
  onPerform: () => void;
  onLeave: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/** A groove from the project's own patterns rather than a stock genre. */
const PROJECT = "\u0000project";

/**
 * Jam mode: drums to play along with and your rig on the input, heard in
 * place of the project, which jamming never edits. Pick a groove, a kit,
 * the key and a rig; play; change section at the next bar and nudge the
 * tempo while it plays.
 *
 * Keys: Space plays and stops, 1 to 6 pick a section, ↑ and ↓ nudge the
 * tempo (⇧ by 5), T taps it.
 */
export function JamView({ status, frame, onPlay, onPerform, onLeave, onStatus, onError }: Props) {
  const ids = useId();
  const jam = status.jam;
  const report = (action: Promise<EngineStatus>) =>
    action.then(onStatus, (e) => onError(String(e)));

  const genres = useMemo(
    () => [...new Set(status.grooves.map((g) => g.genre))].sort((a, b) => a.localeCompare(b)),
    [status.grooves],
  );
  const rigs = status.rigs;
  // Running with no input: the jam plays along with an amp in the room.
  const noInput = status.running && status.inputName === null;
  const rolling = frame.playing || frame.countIn > 0;

  const [taps, setTaps] = useState<number[]>([]);
  // The jam picker in the header: a menu of stock jams and yours.
  const [jamMenu, setJamMenu] = useState<{ x: number; y: number } | null>(null);
  const jamKey = useRef<HTMLButtonElement>(null);
  // `?savejam` opens the save sheet, for screenshots.
  const [saving, setSaving] = useState(() =>
    new URLSearchParams(window.location.search).has("savejam"),
  );

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
      } else if (e.key === "Enter") {
        if (target?.tagName === "BUTTON") return;
        k.report(jamKeep());
      } else if (k.jam.tempoLocked && ["ArrowUp", "ArrowDown", "t", "T"].includes(e.key)) {
        return;
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

  const jamItems: MenuItem[] = [true, false].flatMap((stock) =>
    status.jams
      .filter((j) => j.stock === stock)
      .map((j, i) => ({
        label: `${j.name} · ${j.key}, ${Math.round(j.tempo)} bpm${stock ? "" : " (yours)"}`,
        ...(i === 0 && !stock ? { separated: true } : {}),
        onSelect: () => report(jamLoad(j.path)),
      })),
  );

  return (
    <section aria-label="Jam" className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] gap-3">
      <header className="flex items-center gap-2">
        <span className="caption shrink-0">Jam</span>
        <button
          ref={jamKey}
          type="button"
          aria-haspopup="menu"
          aria-expanded={jamMenu !== null}
          title="Open a stock jam or one of yours"
          onClick={() => {
            const r = jamKey.current?.getBoundingClientRect();
            setJamMenu(jamMenu || !r ? null : { x: r.left, y: r.bottom + 2 });
          }}
          className="surface-lcd flex min-h-8 min-w-0 max-w-96 cursor-pointer items-center gap-2 border-0 px-2 text-left font-lcd text-lcd-fg text-readout"
        >
          <span className="lcd-text min-w-0 flex-1 truncate">
            {setup.name} · {setup.key.tonic} {setup.key.mode}
          </span>
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-2.5 shrink-0 fill-current">
            <path d="M3 6l5 5 5-5z" />
          </svg>
        </button>
        {jamMenu ? (
          <ContextMenu
            label="Open a jam"
            x={jamMenu.x}
            y={jamMenu.y}
            onClose={() => setJamMenu(null)}
            items={jamItems}
          />
        ) : null}
        <span className="flex-1" />
        <Button onClick={() => setSaving(true)} title="Save these settings to your Jams">
          Save as jam…
        </Button>
        <Button onClick={onPerform} title="Jam on stage, full screen (P)">
          Stage
        </Button>
        <Button
          onClick={onLeave}
          title={`Back to ${status.project.name}, which jamming never edits`}
        >
          Back to project
        </Button>
      </header>

      <div className="grid min-h-0 items-start gap-3 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
        <div className="surface-panel scroll-area grid max-h-full min-h-0 content-start gap-4 p-3">
          <section aria-labelledby={`${ids}-band`} className="grid content-start gap-2">
            <h3 id={`${ids}-band`} className="caption m-0">
              Band
            </h3>
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
                    value={String(
                      patterns.find((p) => p.name === setup.drums.patterns?.[section]?.name)?.id ??
                        "",
                    )}
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
            <JamBacking
              backing={setup.backing ?? { kind: "none" }}
              presets={jam.presets}
              rigs={rigs}
              project={status.project.name}
              missing={jam.backingMissing}
              onChange={(backing) => change({ backing })}
              onError={onError}
            />
          </section>

          <section aria-labelledby={`${ids}-song`} className="grid content-start gap-2">
            <h3 id={`${ids}-song`} className="caption m-0">
              Song
            </h3>
            <Field label="Key" hint="What the backing plays in">
              <span className="grid w-full grid-cols-2 gap-1">
                <select
                  aria-label="Tonic"
                  className="surface-field min-h-8 w-full min-w-0 py-0 pl-2"
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
                  className="surface-field min-h-8 w-full min-w-0 py-0 pl-2"
                  value={setup.key.mode}
                  onChange={(e) =>
                    change({ key: { ...setup.key, mode: e.target.value as "major" | "minor" } })
                  }
                >
                  <option value="major">major</option>
                  <option value="minor">minor</option>
                </select>
              </span>
            </Field>
            <Field
              label="Tempo"
              hint={
                jam.tempoLocked
                  ? "Set by the backing's audio: the drums follow it"
                  : "↑ ↓ nudge it, T taps it"
              }
              htmlFor={`${ids}-bpm`}
            >
              <span className="flex w-full items-center gap-1">
                <Button
                  aria-label="Slower"
                  disabled={jam.tempoLocked}
                  title="Slower (↓, ⇧↓ by 5)"
                  onClick={() => report(jamTempo(clampTempo(Math.round(tempo) - 1)))}
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
                  disabled={jam.tempoLocked}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                  }}
                  onBlur={(e) => {
                    const bpm = Number(e.currentTarget.value);
                    if (Number.isFinite(bpm) && bpm !== tempo) report(jamTempo(clampTempo(bpm)));
                  }}
                  className="surface-field min-h-8 w-16 min-w-0 flex-1 px-2 text-center tabular-nums"
                />
                <Button
                  aria-label="Faster"
                  disabled={jam.tempoLocked}
                  title="Faster (↑, ⇧↑ by 5)"
                  onClick={() => report(jamTempo(clampTempo(Math.round(tempo) + 1)))}
                  square
                >
                  +
                </Button>
                <Button onClick={tap} title="Tap the beat (T)" disabled={jam.tempoLocked}>
                  Tap
                </Button>
              </span>
            </Field>
          </section>

          <section aria-labelledby={`${ids}-sound`} className="grid content-start gap-2">
            <h3 id={`${ids}-sound`} className="caption m-0">
              Your sound
            </h3>
            <Select
              id={`${ids}-rig`}
              label="Rig"
              hint={
                noInput
                  ? "No input is open: play through your amp along with the jam"
                  : rig
                    ? rig.chain.map((spec) => spec.replace(/^cab:.*/, "cab")).join(" → ")
                    : "What your input plays through"
              }
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
          </section>
          {jam.missing.length > 0 ? (
            <p role="status" className="m-0 text-ink-dim text-label">
              Left out: {jam.missing.join("; ")}
            </p>
          ) : null}
        </div>

        <div className="surface-panel grid min-h-0 content-start gap-3 p-3">
          <Lcd aria-live="polite" className="grid gap-1 py-2 text-center">
            <span className="block text-display tabular-nums">
              {playingSection
                ? SECTION_LABELS[playingSection]
                : SECTION_LABELS[jam.marks[0]?.section ?? "verse"]}
              {" · "}
              {frame.countIn > 0
                ? `Count ${frame.countIn}`
                : rolling && place
                  ? `Bar ${place.bar} · ${place.beat}`
                  : "Ready"}
              {" · "}
              {Math.round((rolling && place ? place.bpm : tempo) * 10) / 10} bpm
            </span>
            <span className="block min-h-6 text-readout">
              {rolling && place?.next
                ? `${SECTION_LABELS[place.next.section]} at bar ${place.next.bar}`
                : null}
              {rolling && place?.next && place?.nextTempo ? " · " : null}
              {rolling && place?.nextTempo
                ? `${place.nextTempo.bpm} bpm at bar ${place.nextTempo.bar}`
                : null}
              {rolling && !place?.next && jam.end !== null ? "Ending" : null}
              {!rolling ? "Space plays · 1 to 6 pick a section" : null}
            </span>
          </Lcd>

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
                    className="button-standard grid min-h-16 w-full place-items-center content-center gap-1 border-0 px-2 py-2"
                  >
                    <span className="flex items-center gap-1">
                      <Led on={playing} label={SECTION_LABELS[section]} />
                      <span className="font-bold text-label">{SECTION_LABELS[section]}</span>
                    </span>
                    <span className="text-ink-dim text-label">
                      {coming ? "next" : has && bars ? `${bars} bar${bars === 1 ? "" : "s"}` : "—"}
                      {" · "}
                      {i + 1}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              tier="primary"
              aria-pressed={rolling}
              onClick={onPlay}
              title="Play or stop (Space)"
              className="min-w-32"
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
              aria-pressed={status.countIn}
              onClick={() => report(countInSet(!status.countIn))}
              title="Count in a bar before the jam starts"
            >
              <Led on={status.countIn} label="Count in" />
              Count in
            </Button>
            <Metronome metronome={status.metronome} onStatus={onStatus} onError={onError} />
            <span className="flex-1" />
            <span className="flex items-center gap-1">
              <Button
                onClick={() => report(jamKeep())}
                disabled={!status.running || noInput}
                title={
                  noInput
                    ? "Keep that needs an input; pick one in Settings"
                    : `Keep the last ${jam.captureMinutes} minutes you played (Return)`
                }
              >
                <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
                  <path d="M3 1h8l3 3v11H2V1zm1.5 1.5v4h6v-4zM5 10h6v3.5H5z" />
                </svg>
                Keep that
              </Button>
              <select
                aria-label="How much Keep that keeps"
                title="Keep that always keeps this much of your playing"
                className="surface-field min-h-8 py-0 pl-2"
                value={String(jam.captureMinutes)}
                onChange={(e) => report(jamCaptureSet(Number(e.target.value)))}
              >
                {[1, 2, 5, 10].map((m) => (
                  <option key={m} value={m}>
                    last {m} min
                  </option>
                ))}
              </select>
            </span>
          </div>
        </div>
      </div>
      {saving ? (
        <SaveJamSheet
          setup={setup}
          mine={status.jams.filter((j) => !j.stock).map((j) => j.name)}
          onClose={() => setSaving(false)}
          onStatus={onStatus}
          onError={onError}
        />
      ) : null}
      {jam.kept ? (
        <KeepSheet
          kept={jam.kept}
          project={status.project.name}
          onClose={() => {}}
          onStatus={onStatus}
          onError={onError}
        />
      ) : null}
    </section>
  );
}
