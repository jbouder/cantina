import { type FormEvent, useState } from "react";
import {
  clipRemove,
  clock,
  type EngineStatus,
  type TrackStatus,
  trackAdd,
  trackArm,
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";
import { PedalChain } from "./PedalChain";

type Props = {
  tracks: TrackStatus[];
  sampleRate: number;
  kinds: string[];
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The project's tracks in order, each with its arm switch, chain and clips. */
export function TrackList({ tracks, sampleRate, kinds, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  return (
    <div className="grid gap-3">
      <ol className="m-0 grid list-none gap-3 p-0">
        {tracks.map((track, index) => (
          <li key={track.id}>
            <TrackCard
              track={track}
              index={index}
              count={tracks.length}
              sampleRate={sampleRate}
              kinds={kinds}
              onStatus={onStatus}
              onError={onError}
            />
          </li>
        ))}
      </ol>
      <div>
        <Button onClick={() => run(trackAdd())}>Add track</Button>
      </div>
    </div>
  );
}

type CardProps = {
  track: TrackStatus;
  index: number;
  count: number;
  sampleRate: number;
  kinds: string[];
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

function TrackCard({ track, index, count, sampleRate, kinds, onStatus, onError }: CardProps) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(track.name);
  const headingId = `track-${track.id}-title`;

  const rename = (e: FormEvent) => {
    e.preventDefault();
    setEditing(false);
    if (name.trim() && name.trim() !== track.name) run(trackRename(track.id, name));
    else setName(track.name);
  };

  return (
    <section aria-labelledby={headingId} className="surface-panel grid gap-3 p-3">
      <header className="flex flex-wrap items-center gap-2">
        <Button
          aria-pressed={track.armed}
          onClick={() => run(trackArm(track.id, !track.armed))}
          variant={track.armed ? "transport" : "panel"}
        >
          <Led on={track.recording || track.armed} tone="record" label="Armed" />
          Arm
        </Button>
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
        kinds={kinds}
        onStatus={onStatus}
        onError={onError}
      />
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
        {track.clips.length === 0 ? (
          <li className="text-ink-dim text-sm">No clips yet: arm the track and record.</li>
        ) : null}
      </ul>
    </section>
  );
}
