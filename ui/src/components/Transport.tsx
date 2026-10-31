import {
  type EngineStatus,
  type TrackStatus,
  trackArm,
  trackPlay,
  trackRecord,
  trackStop,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Led } from "./Led";

type Props = {
  track: TrackStatus;
  running: boolean;
  /** Timeline position in seconds, from the meter frames. */
  position: number;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

function clock(seconds: number) {
  const m = Math.floor(seconds / 60);
  const s = seconds - m * 60;
  return `${String(m).padStart(2, "0")}:${s.toFixed(1).padStart(4, "0")}`;
}

/** Arm, record, play and stop for the one track, with the position. */
export function Transport({ track, running, position, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const rolling = track.playing || track.recording;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        aria-pressed={track.armed}
        onClick={() => run(trackArm(!track.armed))}
        variant={track.armed ? "transport" : "panel"}
      >
        <Led on={track.armed} tone="record" label="Armed" />
        Arm
      </Button>
      <Button
        variant="transport"
        aria-pressed={track.recording}
        disabled={!running || !track.armed || rolling}
        onClick={() => run(trackRecord())}
      >
        <Led on={track.recording} tone="record" label="Recording" />
        Record
      </Button>
      <Button
        variant="transport"
        aria-pressed={track.playing && !track.recording}
        disabled={!running || !track.take || rolling}
        onClick={() => run(trackPlay())}
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
          <path d="M3 2l11 6-11 6z" />
        </svg>
        Play
      </Button>
      <Button variant="transport" disabled={!running || !rolling} onClick={() => run(trackStop())}>
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
          <path d="M3 3h10v10H3z" />
        </svg>
        Stop
      </Button>
      <Lcd aria-label="Position" className="min-w-28 text-center tabular-nums">
        {clock(position)}
      </Lcd>
      <span className="text-ink-dim text-sm">
        {track.recording
          ? "Recording…"
          : track.take
            ? `${track.take.name} · ${clock(track.take.seconds)}`
            : "No take yet"}
      </span>
    </div>
  );
}
