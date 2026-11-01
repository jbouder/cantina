import { useEffect, useState } from "react";
import {
  clock,
  type EngineStatus,
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
import type { TimelineControls } from "./Timeline";

type Props = {
  transport: TransportStatus;
  tracks: TrackStatus[];
  tempo: Tempo;
  running: boolean;
  /** Timeline position in seconds, from the meter frames. */
  position: number;
  /** True when a drum pattern is looping on a track, which is something to play. */
  auditioning: boolean;
  /** The timeline's view: snap, zoom and the bar readout. */
  timeline: TimelineControls;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The strip above the lanes: record, play and stop, one clock in bars
 * and time, the tempo, then snap and zoom for the timeline.
 */
export function Transport({
  transport,
  tracks,
  tempo,
  running,
  position,
  auditioning,
  timeline,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const rolling = transport.playing || transport.recording;
  const armed = tracks.filter((t) => t.armed && !t.kit);
  const hasClips =
    auditioning || tracks.some((t) => t.clips.length > 0 || t.patternClips.length > 0);
  const note = transport.recording
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
        variant="transport"
        aria-pressed={transport.recording}
        disabled={!running || armed.length === 0 || rolling}
        onClick={() => run(transportRecord())}
        title="Record (R)"
      >
        <Led on={transport.recording} tone="record" label="Recording" />
        Record
      </Button>
      <Button
        variant="transport"
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
        variant="transport"
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
        className="min-h-11 whitespace-nowrap px-3 py-2 text-center text-xl tabular-nums"
      >
        {timeline.bars} · {clock(position)}
      </Lcd>
      <span className="flex items-center gap-1 text-sm">
        <label htmlFor="tempo-bpm" className="font-bold text-[11px] uppercase tracking-[0.08em]">
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
          className="surface-lcd min-h-8 w-20 px-2 text-center font-lcd text-base tabular-nums"
        />
        <span className="whitespace-nowrap text-ink-dim">
          {tempo.numerator}/{tempo.denominator} · grid {timeline.divisionLabel}
        </span>
      </span>
      <Button aria-pressed={timeline.snapOn} onClick={timeline.toggleSnap} size="lg">
        Snap
      </Button>
      <Button aria-label="Zoom out" onClick={timeline.zoomOut} size="lg">
        −
      </Button>
      <Button aria-label="Zoom in" onClick={timeline.zoomIn} size="lg">
        +
      </Button>
      <Button onClick={timeline.fit} title="Zoom to show every clip" size="lg">
        Fit
      </Button>
      <span className="text-ink-dim text-sm">{note}</span>
    </div>
  );
}
