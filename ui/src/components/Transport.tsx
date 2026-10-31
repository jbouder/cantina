import {
  clock,
  type EngineStatus,
  type TrackStatus,
  type TransportStatus,
  transportPlay,
  transportRecord,
  transportStop,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Led } from "./Led";

type Props = {
  transport: TransportStatus;
  tracks: TrackStatus[];
  running: boolean;
  /** Timeline position in seconds, from the meter frames. */
  position: number;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** Record, play and stop for the whole project, with the position. */
export function Transport({ transport, tracks, running, position, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const rolling = transport.playing || transport.recording;
  const armed = tracks.filter((t) => t.armed);
  const hasClips = tracks.some((t) => t.clips.length > 0);
  const note = transport.recording
    ? `Recording ${armed.map((t) => t.name).join(", ")}…`
    : armed.length === 0
      ? "Arm a track to record"
      : `${armed.map((t) => t.name).join(", ")} armed`;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="transport"
        aria-pressed={transport.recording}
        disabled={!running || armed.length === 0 || rolling}
        onClick={() => run(transportRecord())}
      >
        <Led on={transport.recording} tone="record" label="Recording" />
        Record
      </Button>
      <Button
        variant="transport"
        aria-pressed={transport.playing && !transport.recording}
        disabled={!running || !hasClips || rolling}
        onClick={() => run(transportPlay())}
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
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
          <path d="M3 3h10v10H3z" />
        </svg>
        Stop
      </Button>
      <Lcd aria-label="Position" className="min-w-28 text-center tabular-nums">
        {clock(position)}
      </Lcd>
      <span className="text-ink-dim text-sm">{note}</span>
    </div>
  );
}
