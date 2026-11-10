import { useEffect, useState } from "react";
import {
  clock,
  type EngineStatus,
  type MetronomeStatus,
  type Tempo,
  type TrackStatus,
  type TransportStatus,
  tempoSet,
  transportPlay,
  transportRecord,
  transportStop,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { Metronome } from "./Metronome";
import type { TimelineControls } from "./Timeline";

type Props = {
  transport: TransportStatus;
  tracks: TrackStatus[];
  tempo: Tempo;
  metronome: MetronomeStatus;
  running: boolean;
  /** Timeline position in seconds, from the meter frames. */
  position: number;
  /** The beat a count-in is on, from 1; zero with no count-in. */
  countIn: number;
  /** True when a drum pattern is looping on a track, which is something to play. The metronome is too. */
  auditioning: boolean;
  /** The timeline's view: snap, zoom and the bar readout. */
  timeline: TimelineControls;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The one status line: what the transport is doing or waiting for. It
 * lives at the bottom of the window, not in the header.
 */
export function transportNote(
  transport: TransportStatus,
  tracks: TrackStatus[],
  countIn: number,
): string {
  const armed = tracks.filter((t) => t.armed && !t.kit);
  const names = armed.map((t) => t.name).join(", ");
  if (countIn > 0) return "Counting in…";
  if (transport.recording) return `Recording ${names}…`;
  if (armed.length === 0) return "Arm a track to record";
  return `${names} armed`;
}

/** A key's caption, dropped when the strip is too narrow for it. */
const WIDE = "@5xl:inline hidden";

/**
 * The strip above the lanes, one row that never wraps: at a narrow
 * window the keys keep their glyphs and lose their captions.
 *
 * The strip above the lanes: record, play and stop, one clock in bars
 * and time, the tempo and the metronome, then snap and zoom for the
 * timeline.
 */
export function Transport({
  transport,
  tracks,
  tempo,
  metronome,
  running,
  position,
  countIn,
  auditioning,
  timeline,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const rolling = transport.playing || transport.recording;
  const armed = tracks.filter((t) => t.armed && !t.kit);
  const hasClips =
    auditioning ||
    metronome.on ||
    tracks.some((t) => t.clips.length > 0 || t.patternClips.length > 0);
  // Record stays down through the count-in; its light means audio is
  // going to disk, so it waits for the first frame.
  const recording = transport.recording && countIn === 0;
  const [bpm, setBpm] = useState(String(tempo.bpm));
  useEffect(() => {
    setBpm(String(tempo.bpm));
  }, [tempo.bpm]);
  const commitBpm = () => {
    const value = Number(bpm);
    if (Number.isFinite(value) && value > 0 && value !== tempo.bpm) {
      run(tempoSet({ ...tempo, bpm: value }));
    } else {
      setBpm(String(tempo.bpm));
    }
  };

  return (
    <div className="@container">
      <div className="flex items-center gap-3">
        <span className="flex shrink-0 items-center gap-1">
          <Button
            tier="primary"
            aria-label="Record"
            aria-pressed={transport.recording}
            disabled={!running || armed.length === 0 || rolling}
            onClick={() => run(transportRecord())}
            title="Record after a four-beat count-in (R)"
            tight
          >
            <Led on={recording} tone="record" label="Recording" />
            <span className={WIDE}>Record</span>
          </Button>
          <Button
            aria-label="Play"
            aria-pressed={transport.playing && !transport.recording}
            disabled={!running || !hasClips || rolling}
            onClick={() => run(transportPlay())}
            title="Play (Space)"
            tight
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 2l11 6-11 6z" />
            </svg>
            <span className={WIDE}>Play</span>
          </Button>
          <Button
            aria-label="Stop"
            disabled={!running || !rolling}
            onClick={() => run(transportStop())}
            title="Stop (Space)"
            tight
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 3h10v10H3z" />
            </svg>
            <span className={WIDE}>Stop</span>
          </Button>
        </span>
        <Lcd
          aria-label="Position"
          className="min-h-8 min-w-28 shrink-0 whitespace-nowrap px-2 text-center tabular-nums"
        >
          {countIn > 0 ? `Count-in ${countIn}` : `${timeline.bars} · ${clock(position)}`}
        </Lcd>
        <span className="flex shrink-0 items-center gap-1 text-body">
          <label htmlFor="tempo-bpm" className="caption @3xl:not-sr-only sr-only">
            Tempo
          </label>
          <input
            id="tempo-bpm"
            type="number"
            min={20}
            max={300}
            step={1}
            value={bpm}
            onChange={(e) => setBpm(e.target.value)}
            onBlur={commitBpm}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitBpm();
            }}
            className="surface-field min-h-8 w-14 px-1 text-center tabular-nums"
          />
          <span className={`${WIDE} whitespace-nowrap text-ink-dim`}>
            {tempo.numerator}/{tempo.denominator} · grid {timeline.divisionLabel}
          </span>
        </span>
        <Metronome metronome={metronome} onStatus={onStatus} onError={onError} />
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <Button
            aria-pressed={timeline.snapOn}
            onClick={timeline.toggleSnap}
            title="Snap edits to the grid"
          >
            Snap
          </Button>
          <Button square aria-label="Zoom out" title="Zoom out (⌘−)" onClick={timeline.zoomOut}>
            −
          </Button>
          <Button square aria-label="Zoom in" title="Zoom in (⌘=)" onClick={timeline.zoomIn}>
            +
          </Button>
          <Button onClick={timeline.fit} title="Zoom to show every clip">
            Fit
          </Button>
        </span>
      </div>
    </div>
  );
}
