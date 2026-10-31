import { type FormEvent, useEffect, useState } from "react";
import {
  type AuditionStatus,
  clipRemove,
  clock,
  type EngineStatus,
  type KitInfo,
  type PatternInfo,
  type Tempo,
  type TrackStatus,
  trackArm,
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { Button } from "./Button";
import { DrumGrid } from "./DrumGrid";
import { Led } from "./Led";
import { PedalChain } from "./PedalChain";

type Props = {
  track: TrackStatus;
  index: number;
  count: number;
  sampleRate: number;
  kinds: string[];
  /** For a drum track: the patterns, kits and audition, and the clock. */
  patterns: PatternInfo[];
  kits: KitInfo[];
  audition: AuditionStatus | null;
  tempo: Tempo;
  position: number;
  playing: boolean;
  onBrowsePlugins: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The selected track: arm, rename, move and remove it, its pedal chain,
 * and its clips as a list (the timeline draws them; this is the list a
 * screen reader or the keyboard gets). A drum track has no input to
 * arm; it gets the step sequencer instead of the clip list.
 */
export function TrackPanel({
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
  onBrowsePlugins,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(track.name);
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

  return (
    <section aria-labelledby={headingId} className="surface-panel grid gap-3 p-3">
      <header className="flex flex-wrap items-center gap-2">
        {track.kit ? (
          <span
            role="img"
            aria-label="Drum track"
            title={track.kit.name}
            className="grid size-11 shrink-0 place-items-center text-ink-dim"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-5 fill-current">
              <path d="M8 2c3.3 0 6 1.1 6 2.5v6C14 12.9 11.3 14 8 14s-6-1.1-6-3.5v-6C2 3.1 4.7 2 8 2zm0 1.5c-2.8 0-4.5.8-4.5 1s1.7 1 4.5 1 4.5-.8 4.5-1-1.7-1-4.5-1z" />
            </svg>
          </span>
        ) : (
          <Button
            aria-pressed={track.armed}
            onClick={() => run(trackArm(track.id, !track.armed))}
            variant={track.armed ? "transport" : "panel"}
          >
            <Led on={track.recording || track.armed} tone="record" label="Armed" />
            Arm
          </Button>
        )}
        {editing ? (
          <form onSubmit={rename} className="flex flex-1 items-center gap-2">
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
            className="m-0 flex-1 truncate font-bold text-[11px] uppercase tracking-[0.1em]"
          >
            <button
              type="button"
              onClick={() => setEditing(true)}
              title="Rename"
              className="min-h-11 border-0 bg-transparent p-0 text-left font-bold text-[11px] text-inherit uppercase tracking-[0.1em]"
            >
              {track.name}
            </button>
            {track.recording ? (
              <span className="ml-2 text-led-rec normal-case">recording</span>
            ) : null}
          </h3>
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
      <PedalChain
        track={track.id}
        chain={track.chain}
        sampleRate={sampleRate}
        kinds={kinds}
        onBrowsePlugins={onBrowsePlugins}
        onStatus={onStatus}
        onError={onError}
      />
      {track.kit ? (
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
      ) : null}
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
              <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3 fill-current">
                <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
              </svg>
            </Button>
          </li>
        ))}
        {track.clips.length === 0 && !track.kit ? (
          <li className="text-ink-dim text-sm">No clips yet: arm the track and record.</li>
        ) : null}
      </ul>
    </section>
  );
}
