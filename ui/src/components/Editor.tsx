import { type FormEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import {
  type AuditionStatus,
  clipRemove,
  clock,
  type EngineStatus,
  type IrInfo,
  type KitInfo,
  type LibraryStatus,
  type PatternInfo,
  type PedalKindInfo,
  type Practice,
  patternClipRemove,
  type StockGroove,
  type Tempo,
  type TrackStatus,
  trackBounce,
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { Button } from "./Button";
import { DrumGrid } from "./DrumGrid";
import { RenderBar, useRenderProgress } from "./ExportSheet";
import { PedalChain } from "./PedalChain";
import { TabView } from "./TabView";
import { TranscribeSheet } from "./TranscribeSheet";

/** The editor's tabs; a drum track has Beats, an audio track does not. */
export type EditorTab = "beats" | "pedalboard" | "clips" | "tab";

export function isEditorTab(value: unknown): value is EditorTab {
  return value === "beats" || value === "pedalboard" || value === "clips" || value === "tab";
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
  library: LibraryStatus;
  audition: AuditionStatus | null;
  /** Hearing tabs and practice speed, for the Tab tab. */
  practice: Practice;
  /** The stock grooves, for the Beats tab. */
  grooves: StockGroove[];
  tempo: Tempo;
  position: number;
  playing: boolean;
  /** The latest frame's pedal readouts by pedal id. */
  readouts: Map<number, number[]>;
  irs: IrInfo[];
  /** The tab to open on; the track's kind picks one when unset. */
  initialTab?: EditorTab | undefined;
  /** Counts File ▸ Bounce Track in Place; each new value bounces the track once. */
  bounceRequest?: number;
  /** Counts Track ▸ Rename Track; each new value opens the name field. */
  renameRequest?: number;
  /** Opens the picker, which adds at the end of the chain. */
  onAdd: () => void;
  onDesign: (pedal: number) => void;
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
  library,
  audition,
  practice,
  grooves,
  tempo,
  position,
  playing,
  readouts,
  irs,
  initialTab,
  bounceRequest = 0,
  renameRequest = 0,
  onAdd,
  onDesign,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(track.name);
  const tabbed = track.clips.filter((c) => c.tab);
  const tabs: EditorTab[] = track.kit
    ? ["beats", "pedalboard", "clips"]
    : tabbed.length > 0
      ? ["pedalboard", "clips", "tab"]
      : ["pedalboard", "clips"];
  const [tab, setTab] = useState<EditorTab>(() =>
    initialTab && tabs.includes(initialTab) ? initialTab : track.kit ? "beats" : "pedalboard",
  );
  const headingId = `track-${track.id}-title`;
  const [bouncing, setBouncing] = useState(false);
  const bounceProgress = useRenderProgress("bounce");
  const hasContent = track.kit ? track.patternClips.length > 0 : track.clips.length > 0;
  // A bounced drum track has no Beats tab any more.
  const shown: EditorTab = tabs.includes(tab) ? tab : "pedalboard";

  const bounce = () => {
    setBouncing(true);
    trackBounce(track.id)
      .then(onStatus, (e) => onError(String(e)))
      .finally(() => setBouncing(false));
  };
  // A rename asked for from the menu opens the name field.
  const renamed = useRef(renameRequest);
  useEffect(() => {
    if (renameRequest === renamed.current) return;
    renamed.current = renameRequest;
    setEditing(true);
  });
  // A bounce asked for from the menu, once per request.
  const bounced = useRef(bounceRequest);
  useEffect(() => {
    if (bounceRequest === bounced.current) return;
    bounced.current = bounceRequest;
    if (hasContent && !bouncing) bounce();
  });

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
    const at = tabs.indexOf(shown);
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
    tab: "Tab",
  };

  return (
    <section
      aria-labelledby={headingId}
      className="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)]"
    >
      <header className="flex flex-wrap items-center gap-2 pb-2">
        {editing ? (
          <form onSubmit={rename} className="flex min-w-48 flex-1 items-center gap-2">
            <label htmlFor={`${headingId}-input`} className="sr-only">
              Track name
            </label>
            <input
              id={`${headingId}-input`}
              // biome-ignore lint/a11y/noAutofocus: the field appears on request, under the pointer
              autoFocus
              className="surface-lcd min-h-8 min-w-0 flex-1 px-2 font-lcd text-base"
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
            className="m-0 flex min-w-0 flex-1 items-center gap-2 truncate font-bold text-sm uppercase tracking-[0.1em]"
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
              className="min-h-8 truncate border-0 bg-transparent p-0 text-left font-bold text-inherit text-sm uppercase tracking-[0.1em]"
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
              aria-selected={shown === t}
              aria-controls={`panel-${t}`}
              tabIndex={shown === t ? 0 : -1}
              variant={shown === t ? "transport" : "panel"}
              onClick={() => setTab(t)}
              onKeyDown={onTabKey}
            >
              {labels[t]}
            </Button>
          ))}
        </div>
        {bouncing ? (
          <RenderBar label={`Bouncing ${track.name}`} fraction={bounceProgress ?? 0} />
        ) : (
          <Button
            disabled={!hasContent || track.recording}
            title={
              hasContent
                ? `Render ${track.name} through its ${track.kit ? "kit and " : ""}pedals to one clip in its place. Undo brings them back.`
                : `${track.name} has nothing on it to bounce`
            }
            onClick={bounce}
          >
            Bounce
          </Button>
        )}
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
        id={`panel-${shown}`}
        aria-labelledby={`tab-${shown}`}
        className="min-h-0 overflow-auto"
      >
        {shown === "beats" && track.kit ? (
          <DrumGrid
            track={track}
            patterns={patterns}
            kits={kits}
            library={library}
            grooves={grooves}
            audition={audition}
            tempo={tempo}
            position={position}
            playing={playing}
            onStatus={onStatus}
            onError={onError}
          />
        ) : shown === "pedalboard" ? (
          <PedalChain
            chain={track.chain}
            sampleRate={sampleRate}
            kinds={kinds}
            readouts={readouts}
            irs={irs}
            onAdd={onAdd}
            onDesign={onDesign}
            onStatus={onStatus}
            onError={onError}
          />
        ) : shown === "tab" ? (
          <TabView
            clips={tabbed}
            sampleRate={sampleRate}
            position={position}
            playing={playing}
            practice={practice}
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
  // `?transcribe=<clip id>` opens the sheet in the browser, for screenshots.
  const [transcribing, setTranscribing] = useState<number | null>(() => {
    const id = Number(new URLSearchParams(window.location.search).get("transcribe"));
    return track.clips.some((c) => c.id === id) ? id : null;
  });
  const sheetClip = track.clips.find((c) => c.id === transcribing);
  const remove = (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3 fill-current">
      <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
    </svg>
  );
  return (
    <>
      <ul aria-label={`${track.name} clips`} className="m-0 flex list-none flex-wrap gap-2 p-0">
        {track.clips.map((clip) => (
          <li
            key={clip.id}
            className="surface-lcd flex items-center gap-2 py-1 pr-1 pl-3 font-lcd text-base"
          >
            <span className="lcd-text">
              {clip.name} · {clock(clip.start / sampleRate)} +{clock(clip.length / sampleRate)}
              {clip.tab ? " · tab" : ""}
            </span>
            <Button
              aria-label={`Transcribe ${clip.name} to tab`}
              title={clip.tab ? "Transcribe again or re-quantise" : "Transcribe to tab"}
              onClick={() => setTranscribing(clip.id)}
              className="min-h-9 px-2"
            >
              Tab
            </Button>
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
      {sheetClip ? (
        <TranscribeSheet
          clip={sheetClip}
          onStatus={onStatus}
          onClose={() => setTranscribing(null)}
        />
      ) : null}
    </>
  );
}
