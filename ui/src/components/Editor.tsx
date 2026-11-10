import {
  type FormEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import {
  type AuditionStatus,
  brokenOf,
  clipRemove,
  clock,
  type DrumRows,
  type EngineStatus,
  type InstrumentProfile,
  type IrInfo,
  inTauri,
  type KitInfo,
  type LibraryStatus,
  type PatternClipInfo,
  type PatternInfo,
  type PedalKindInfo,
  type Practice,
  patternClipGenerateFill,
  patternClipRemove,
  patternClipSetFill,
  type RigAudition,
  type RigInfo,
  type StockGroove,
  type Tempo,
  type TrackStatus,
  trackBounce,
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { Button } from "./Button";
import { ContextMenu } from "./ContextMenu";
import { DrumGrid } from "./DrumGrid";
import { RenderBar, useRenderProgress } from "./ExportSheet";
import { PedalChain } from "./PedalChain";
import { RigPicker } from "./RigPicker";
import { TabView } from "./TabView";
import { TranscribeSheet } from "./TranscribeSheet";

/** The editor's tabs; a drum track has Beats, an audio track does not. */
export type EditorTab = "beats" | "pedalboard" | "clips" | "tab";

export function isEditorTab(value: unknown): value is EditorTab {
  return value === "beats" || value === "pedalboard" || value === "clips" || value === "tab";
}

/** No rows muted or soloed. */
const NO_ROWS: DrumRows = { muted: [], soloed: [] };

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
  /** The rigs, and the one being auditioned, for an audio track's pedalboard. */
  rigs: RigInfo[];
  rigAudition: RigAudition | null;
  /** The rig picker starts open: `?rigs` in the browser, for screenshots. */
  rigsOpen?: boolean;
  /** What the player plays through: the picker's and the transcription's defaults. */
  profile: InstrumentProfile;
  /** Rows muted or soloed on drum tracks, by track id. */
  drumRows: Record<number, DrumRows>;
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
  rigs,
  rigAudition,
  rigsOpen = false,
  profile,
  drumRows,
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

  // The overflow menu, opened under its key.
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const menuKey = useRef<HTMLButtonElement>(null);
  const openMenu = useCallback(() => {
    const r = menuKey.current?.getBoundingClientRect();
    if (r) setMenu({ x: r.right, y: r.bottom + 2 });
  }, []);
  // In a browser `?trackmenu` opens it, for screenshots.
  useEffect(() => {
    if (!inTauri && new URLSearchParams(window.location.search).has("trackmenu")) openMenu();
  }, [openMenu]);

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
      <header className="flex items-center gap-3 pb-2">
        {editing ? (
          <form onSubmit={rename} className="flex w-56 shrink-0 items-center gap-2">
            <label htmlFor={`${headingId}-input`} className="sr-only">
              Track name
            </label>
            <input
              id={`${headingId}-input`}
              // biome-ignore lint/a11y/noAutofocus: the field appears on request, under the pointer
              autoFocus
              className="surface-field min-h-8 min-w-0 flex-1 px-2"
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
            className="m-0 flex min-w-0 max-w-64 shrink-0 items-center gap-2 truncate font-bold text-body"
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
              className="min-h-8 truncate border-0 bg-transparent p-0 text-left font-bold text-body text-inherit"
            >
              {track.name}
            </button>
            {track.recording ? <span className="text-led-rec">recording</span> : null}
          </h3>
        )}
        <div
          role="tablist"
          aria-label={`${track.name} editor`}
          className="tab-rule flex min-w-0 flex-1 items-end gap-0.5 self-stretch"
        >
          {tabs.map((t) => (
            <button
              key={t}
              type="button"
              id={`tab-${t}`}
              role="tab"
              aria-selected={shown === t}
              aria-controls={`panel-${t}`}
              tabIndex={shown === t ? 0 : -1}
              onClick={() => setTab(t)}
              onKeyDown={onTabKey}
              className="tab min-h-7 px-3 font-bold text-label"
            >
              {labels[t]}
            </button>
          ))}
        </div>
        {bouncing ? (
          <RenderBar label={`Bouncing ${track.name}`} fraction={bounceProgress ?? 0} />
        ) : null}
        <Button
          tier="quiet"
          square
          aria-label={`${track.name} actions`}
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          title="Track actions: rename, bounce, move, remove"
          ref={menuKey}
          onClick={openMenu}
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
            <circle cx="3" cy="8" r="1.6" />
            <circle cx="8" cy="8" r="1.6" />
            <circle cx="13" cy="8" r="1.6" />
          </svg>
        </Button>
        {menu ? (
          <ContextMenu
            label={`${track.name} actions`}
            x={menu.x}
            y={menu.y}
            alignEnd
            onClose={() => setMenu(null)}
            items={[
              { label: "Rename…", onSelect: () => setEditing(true) },
              {
                label: hasContent ? "Bounce to one clip" : "Bounce (nothing to bounce)",
                disabled: !hasContent || track.recording || bouncing,
                onSelect: bounce,
              },
              {
                label: "Move up",
                separated: true,
                disabled: index === 0,
                onSelect: () => run(trackMove(track.id, index - 1)),
              },
              {
                label: "Move down",
                disabled: index === count - 1,
                onSelect: () => run(trackMove(track.id, index + 1)),
              },
              {
                label: "Remove track",
                separated: true,
                disabled: track.recording,
                onSelect: () => run(trackRemove(track.id)),
              },
            ]}
          />
        ) : null}
      </header>
      <div
        role="tabpanel"
        id={`panel-${shown}`}
        aria-labelledby={`tab-${shown}`}
        className="scroll-area min-h-0"
      >
        {shown === "beats" && track.kit ? (
          <DrumGrid
            track={track}
            patterns={patterns}
            kits={kits}
            library={library}
            grooves={grooves}
            rows={drumRows[track.id] ?? NO_ROWS}
            audition={audition}
            tempo={tempo}
            position={position}
            playing={playing}
            onStatus={onStatus}
            onError={onError}
          />
        ) : shown === "pedalboard" ? (
          <div className="flex h-full min-h-0 flex-col gap-2">
            {track.kit ? null : (
              <RigPicker
                track={track}
                rigs={rigs}
                audition={rigAudition}
                broken={brokenOf(library, "rig")}
                onStatus={onStatus}
                onError={onError}
                initiallyOpen={rigsOpen}
                defaultInstrument={profile.instrument}
              />
            )}
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
          </div>
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
            profile={profile}
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
  profile: InstrumentProfile;
  patterns: PatternInfo[];
  sampleRate: number;
  tempo: Tempo;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The track's clips as a list with a delete each: the timeline for the keyboard. */
function ClipList({ track, profile, patterns, sampleRate, tempo, onStatus, onError }: ClipsProps) {
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
            className="surface-lcd flex items-center gap-2 py-1 pr-1 pl-3 font-lcd text-readout"
          >
            <span className="lcd-text">
              {clip.name} · {clock(clip.start / sampleRate)} +{clock(clip.length / sampleRate)}
              {clip.tab ? " · tab" : ""}
            </span>
            <Button
              aria-label={`Transcribe ${clip.name} to tab`}
              title={clip.tab ? "Transcribe again or re-quantize" : "Transcribe to tab"}
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
              className="surface-lcd flex items-center gap-2 py-1 pr-1 pl-3 font-lcd text-readout"
            >
              <span className="lcd-text min-w-0 flex-1 truncate">
                {name} · bar {at} · {bars} bar{bars === "1" ? "" : "s"}
              </span>
              <FillControls
                clip={clip}
                label={`${name} at bar ${at}`}
                patterns={patterns}
                run={run}
              />
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
          <li className="text-ink-dim text-body">No clips yet: arm the track and record.</li>
        ) : null}
        {track.kit && track.patternClips.length === 0 ? (
          <li className="text-ink-dim text-body">
            No patterns placed yet: Place at playhead on the Beats tab, or drag a pattern onto the
            timeline.
          </li>
        ) : null}
      </ul>
      {sheetClip ? (
        <TranscribeSheet
          clip={sheetClip}
          profile={profile}
          onStatus={onStatus}
          onClose={() => setTranscribing(null)}
        />
      ) : null}
    </>
  );
}

/** How often a fill comes round, as the menu offers it. */
const FILL_EVERY = [0, 2, 4, 8, 16];

/**
 * A pattern clip's fill: which pattern plays on fill bars, every how many
 * bars, and whether its last bar is one. **Generate** writes a fill from
 * the clip's own pattern and picks it. Each change is one undoable edit.
 */
function FillControls({
  clip,
  label,
  patterns,
  run,
}: {
  clip: PatternClipInfo;
  label: string;
  patterns: PatternInfo[];
  run: (action: Promise<EngineStatus>) => void;
}) {
  const id = useId();
  const fill = clip.fill;
  const set = (next: { pattern?: number | null; every?: number; atEnd?: boolean }) => {
    const pattern = next.pattern === undefined ? fill?.pattern : next.pattern;
    if (pattern === null || pattern === undefined) {
      run(patternClipSetFill(clip.id, null));
      return;
    }
    run(
      patternClipSetFill(clip.id, {
        pattern,
        every: next.every ?? fill?.every ?? 4,
        atEnd: next.atEnd ?? fill?.atEnd ?? true,
      }),
    );
  };
  const others = patterns.filter((p) => p.id !== clip.pattern);
  return (
    <span className="flex items-center gap-1">
      <label htmlFor={`${id}-fill`} className="sr-only">
        Fill for {label}
      </label>
      <select
        id={`${id}-fill`}
        value={fill?.pattern ?? ""}
        onChange={(e) => set({ pattern: e.target.value === "" ? null : Number(e.target.value) })}
        className="surface-field min-h-8 max-w-40 appearance-none truncate px-2"
      >
        <option value="">No fill</option>
        {others.map((p) => (
          <option key={p.id} value={p.id}>
            Fill: {p.name}
          </option>
        ))}
      </select>
      {fill ? (
        <>
          <label htmlFor={`${id}-every`} className="sr-only">
            How often {label} plays its fill
          </label>
          <select
            id={`${id}-every`}
            value={fill.every}
            onChange={(e) => set({ every: Number(e.target.value) })}
            className="surface-field min-h-8 appearance-none px-2"
          >
            {FILL_EVERY.map((n) => (
              <option key={n} value={n}>
                {n === 0 ? "Not repeating" : `Every ${n} bars`}
              </option>
            ))}
          </select>
          <Button
            aria-pressed={fill.atEnd}
            onClick={() => set({ atEnd: !fill.atEnd })}
            title="Play the fill on the clip's last bar, into what comes next"
            className="min-h-8 px-2"
          >
            At end
          </Button>
        </>
      ) : null}
      <Button
        onClick={() => run(patternClipGenerateFill(clip.id, Date.now() % 1_000_000))}
        title="Write a fill from this clip's pattern and play it every four bars and at the end"
        aria-label={`Generate a fill for ${label}`}
        className="min-h-8 px-2"
      >
        Generate
      </Button>
    </span>
  );
}
