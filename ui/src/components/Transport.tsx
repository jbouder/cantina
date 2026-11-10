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
  const note =
    countIn > 0
      ? "Counting in…"
      : transport.recording
        ? `Recording ${armed.map((t) => t.name).join(", ")}…`
        : armed.length === 0
          ? "Arm a track to record"
          : `${armed.map((t) => t.name).join(", ")} armed`;

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
    <div className="flex flex-wrap items-center gap-2">
      <Button
        tier="primary"
        aria-pressed={transport.recording}
        disabled={!running || armed.length === 0 || rolling}
        onClick={() => run(transportRecord())}
        title="Record after a four-beat count-in (R)"
      >
        <Led on={recording} tone="record" label="Recording" />
        Record
      </Button>
      <Button
        tier="primary"
        aria-pressed={transport.playing && !transport.recording}
        disabled={!running || !hasClips || rolling}
        onClick={() => run(transportPlay())}
        title="Play (Space)"
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
          <path d="M3 2l11 6-11 6z" />
        </svg>
        Play
      </Button>
      <Button
        tier="primary"
        disabled={!running || !rolling}
        onClick={() => run(transportStop())}
        title="Stop (Space)"
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
          <path d="M3 3h10v10H3z" />
        </svg>
        Stop
      </Button>
      <Lcd
        aria-label="Position"
        className="min-h-8 min-w-32 whitespace-nowrap px-2 text-center tabular-nums"
      >
        {countIn > 0 ? `Count-in ${countIn}` : `${timeline.bars} · ${clock(position)}`}
      </Lcd>
      <span className="flex items-center gap-1 text-body">
        <label htmlFor="tempo-bpm" className="caption">
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
          className="surface-field min-h-8 w-16 px-2 text-center tabular-nums"
        />
        <span className="whitespace-nowrap text-ink-dim">
          {tempo.numerator}/{tempo.denominator} · grid {timeline.divisionLabel}
        </span>
      </span>
      <Metronome metronome={metronome} onStatus={onStatus} onError={onError} />
      <Button aria-pressed={timeline.snapOn} onClick={timeline.toggleSnap}>
        Snap
      </Button>
      <Button aria-label="Zoom out" onClick={timeline.zoomOut}>
        −
      </Button>
      <Button aria-label="Zoom in" onClick={timeline.zoomIn}>
        +
      </Button>
      <Button onClick={timeline.fit} title="Zoom to show every clip">
        Fit
      </Button>
      <span className="text-ink-dim text-body">{note}</span>
    </div>
  );
}
