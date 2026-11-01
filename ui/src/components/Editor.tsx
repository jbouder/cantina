import { type FormEvent, type KeyboardEvent, useEffect, useState } from "react";
import {
  type AuditionStatus,
  clipRemove,
  clock,
  type EngineStatus,
  type IrInfo,
  type KitInfo,
  type PatternInfo,
  type PedalKindInfo,
  patternClipRemove,
  type Tempo,
  type TrackStatus,
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { Button } from "./Button";
import { DrumGrid } from "./DrumGrid";
import { PedalChain } from "./PedalChain";

/** The editor's tabs; a drum track has Beats, an audio track does not. */
export type EditorTab = "beats" | "pedalboard" | "clips";

export function isEditorTab(value: unknown): value is EditorTab {
  return value === "beats" || value === "pedalboard" || value === "clips";
}

type Props = {
  track: TrackStatus;
  index: number;
  count: number;
  sampleRate: number;
  kinds: PedalKindInfo[];
  /** For a drum track: the patterns, kits and audition, and the clock. */
  patterns: PatternInfo[];
  kits: KitInfo[];
  audition: AuditionStatus | null;
  tempo: Tempo;
  position: number;
  playing: boolean;
  /** The latest frame's pedal readouts by pedal id. */
  readouts: Map<number, number[]>;
  irs: IrInfo[];
  /** The tab to open on; the track's kind picks one when unset. */
  initialTab?: EditorTab | undefined;
  onBrowsePlugins: () => void;
  onDesign: (pedal: number | null) => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The pane under the arrangement: the selected track's name (click to
 * rename), move and remove, and tabs for what the track has. An audio
 * track has its pedalboard and its clips as a list (the timeline draws
 * them; the list is what a screen reader or the keyboard gets). A drum
 * track opens on its beats, with the pedalboard and clips beside.
 */
export function Editor({
  track,
  index,
  count,
  sampleRate,
  kinds,
  patterns,
  kits,
  audition,
  tempo,
  position,
  playing,
  readouts,
  irs,
  initialTab,
  onBrowsePlugins,
  onDesign,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(track.name);
  const tabs: EditorTab[] = track.kit ? ["beats", "pedalboard", "clips"] : ["pedalboard", "clips"];
  const [tab, setTab] = useState<EditorTab>(() =>
    initialTab && tabs.includes(initialTab) ? initialTab : track.kit ? "beats" : "pedalboard",
  );
  const headingId = `track-${track.id}-title`;

  useEffect(() => {
    setName(track.name);
    setEditing(false);
  }, [track.name]);

  const rename = (e: FormEvent) => {
    e.preventDefault();
    setEditing(false);
    if (name.trim() && name.trim() !== track.name) run(trackRename(track.id, name));
    else setName(track.name);
  };

  const onTabKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    const at = tabs.indexOf(tab);
    const next =
      e.key === "ArrowRight"
        ? tabs[(at + 1) % tabs.length]
        : e.key === "ArrowLeft"
          ? tabs[(at - 1 + tabs.length) % tabs.length]
          : null;
    if (!next) return;
    e.preventDefault();
    setTab(next);
    (e.currentTarget.parentElement?.querySelector(`#tab-${next}`) as HTMLElement | null)?.focus();
  };

  const labels: Record<EditorTab, string> = {
    beats: "Beats",
    pedalboard: "Pedalboard",
    clips: "Clips",
  };

  return (
    <section
      aria-labelledby={headingId}
      className="surface-panel grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)]"
    >
      <header className="flex flex-wrap items-center gap-2 px-3 pt-2">
        {editing ? (
          <form onSubmit={rename} className="flex min-w-48 flex-1 items-center gap-2">
            <label htmlFor={`${headingId}-input`} className="sr-only">
              Track name
            </label>
            <input
              id={`${headingId}-input`}
              // biome-ignore lint/a11y/noAutofocus: the field appears on request, under the pointer
              autoFocus
              className="surface-lcd min-h-11 min-w-0 flex-1 px-2 font-lcd text-lg"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onBlur={rename}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setName(track.name);
                  setEditing(false);
                }
              }}
            />
          </form>
        ) : (
          <h3
            id={headingId}
            className="m-0 flex min-w-0 flex-1 items-center gap-2 truncate font-bold text-[11px] uppercase tracking-[0.1em]"
          >
            {track.kit ? (
              <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 shrink-0 fill-current">
                <path d="M8 2c3.3 0 6 1.1 6 2.5v6C14 12.9 11.3 14 8 14s-6-1.1-6-3.5v-6C2 3.1 4.7 2 8 2zm0 1.5c-2.8 0-4.5.8-4.5 1s1.7 1 4.5 1 4.5-.8 4.5-1-1.7-1-4.5-1z" />
              </svg>
            ) : null}
            <button
              type="button"
              onClick={() => setEditing(true)}
              title="Rename"
              className="min-h-11 truncate border-0 bg-transparent p-0 text-left font-bold text-[11px] text-inherit uppercase tracking-[0.1em]"
            >
              {track.name}
            </button>
            {track.recording ? <span className="text-led-rec normal-case">recording</span> : null}
          </h3>
        )}
        <div role="tablist" aria-label={`${track.name} editor`} className="flex gap-1">
          {tabs.map((t) => (
            <Button
              key={t}
              id={`tab-${t}`}
              role="tab"
              aria-selected={tab === t}
              aria-controls={`panel-${t}`}
              tabIndex={tab === t ? 0 : -1}
              variant={tab === t ? "transport" : "panel"}
              onClick={() => setTab(t)}
              onKeyDown={onTabKey}
            >
              {labels[t]}
            </Button>
          ))}
        </div>
        <Button
          aria-label={`Move ${track.name} up`}
          disabled={index === 0}
          onClick={() => run(trackMove(track.id, index - 1))}
          className="px-2"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M8 3l6 7H2z" />
          </svg>
        </Button>
        <Button
          aria-label={`Move ${track.name} down`}
          disabled={index === count - 1}
          onClick={() => run(trackMove(track.id, index + 1))}
          className="px-2"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M8 13l6-7H2z" />
          </svg>
        </Button>
        <Button
          aria-label={`Remove ${track.name}`}
          disabled={track.recording}
          onClick={() => run(trackRemove(track.id))}
          className="px-2"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </Button>
      </header>
      <div
        role="tabpanel"
        id={`panel-${tab}`}
        aria-labelledby={`tab-${tab}`}
        className="min-h-0 overflow-auto p-3"
      >
        {tab === "beats" && track.kit ? (
          <DrumGrid
            track={track}
            patterns={patterns}
            kits={kits}
            audition={audition}
            tempo={tempo}
            position={position}
            playing={playing}
            onStatus={onStatus}
            onError={onError}
          />
        ) : tab === "pedalboard" ? (
          <PedalChain
            track={track.id}
            chain={track.chain}
            sampleRate={sampleRate}
            kinds={kinds}
            readouts={readouts}
            irs={irs}
            onBrowsePlugins={onBrowsePlugins}
            onDesign={onDesign}
            onStatus={onStatus}
            onError={onError}
          />
        ) : (
          <ClipList
            track={track}
            patterns={patterns}
            sampleRate={sampleRate}
            tempo={tempo}
            onStatus={onStatus}
            onError={onError}
          />
        )}
      </div>
    </section>
  );
}

type ClipsProps = {
  track: TrackStatus;
  patterns: PatternInfo[];
  sampleRate: number;
  tempo: Tempo;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The track's clips as a list with a delete each: the timeline for the keyboard. */
function ClipList({ track, patterns, sampleRate, tempo, onStatus, onError }: ClipsProps) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const remove = (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3 fill-current">
      <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
    </svg>
  );
  return (
    <ul aria-label={`${track.name} clips`} className="m-0 flex list-none flex-wrap gap-2 p-0">
      {track.clips.map((clip) => (
        <li
          key={clip.id}
          className="surface-lcd flex items-center gap-2 py-1 pr-1 pl-3 font-lcd text-base"
        >
          <span className="lcd-text">
            {clip.name} · {clock(clip.start / sampleRate)} +{clock(clip.length / sampleRate)}
          </span>
          <Button
            aria-label={`Delete ${clip.name}`}
            onClick={() => run(clipRemove(clip.id))}
            className="min-h-9 min-w-9 px-1"
          >
            {remove}
          </Button>
        </li>
      ))}
      {track.patternClips.map((clip) => {
        const name = patterns.find((p) => p.id === clip.pattern)?.name ?? "?";
        const bars = (clip.length / 960 / tempo.numerator).toFixed(1).replace(/\.0$/, "");
        const at = (clip.start / 960 / tempo.numerator + 1).toFixed(1).replace(/\.0$/, "");
        return (
          <li
            key={clip.id}
            className="surface-lcd flex items-center gap-2 py-1 pr-1 pl-3 font-lcd text-base"
          >
            <span className="lcd-text">
              {name} · bar {at} · {bars} bar{bars === "1" ? "" : "s"}
            </span>
            <Button
              aria-label={`Delete ${name} at bar ${at}`}
              onClick={() => run(patternClipRemove(clip.id))}
              className="min-h-9 min-w-9 px-1"
            >
              {remove}
            </Button>
          </li>
        );
      })}
      {track.clips.length === 0 && !track.kit ? (
        <li className="text-ink-dim text-sm">No clips yet: arm the track and record.</li>
      ) : null}
      {track.kit && track.patternClips.length === 0 ? (
        <li className="text-ink-dim text-sm">
          No patterns placed yet: Place at playhead on the Beats tab, or drag a pattern onto the
          timeline.
        </li>
      ) : null}
    </ul>
  );
}
