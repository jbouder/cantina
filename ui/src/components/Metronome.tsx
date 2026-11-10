import { useEffect, useState } from "react";
import { type EngineStatus, type MetronomeStatus, metronomeSet } from "../lib/engine";
import { Button } from "./Button";

type Props = {
  metronome: MetronomeStatus;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The metronome in the transport strip: a switch, the sound, and the
 * level. It clicks on every beat of the tempo while the transport plays.
 */
export function Metronome({ metronome, onStatus, onError }: Props) {
  const { on, sound, levelDb, sounds } = metronome;
  const set = (change: Partial<MetronomeStatus>) =>
    metronomeSet({ on, sound, levelDb, ...change }).then(onStatus, (e) => onError(String(e)));

  // The slider moves at once; the engine hears each step.
  const [level, setLevel] = useState(levelDb);
  useEffect(() => {
    setLevel(levelDb);
  }, [levelDb]);

  return (
    <span className="flex min-w-0 items-center gap-1">
      <Button
        aria-label="Metronome"
        aria-pressed={on}
        onClick={() => set({ on: !on })}
        title="Metronome (K)"
        tight
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
          <path d="M6 1h4l3.5 14h-11zm1.2 1.5L5.9 8h2.3l2.6-4.1.9.6L9.5 8h.6L8.8 2.5zM5.5 9.5l-1 4h7l-1-4z" />
        </svg>
        <span className="@5xl:inline hidden">Click</span>
      </Button>
      <span className="relative min-w-20 shrink">
        <select
          aria-label="Metronome sound"
          title="Metronome sound"
          className="surface-field min-h-8 w-28 min-w-20 max-w-full appearance-none truncate py-0 pr-6 pl-2"
          value={sound}
          onChange={(e) => set({ sound: e.target.value })}
        >
          {sounds.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <svg
          aria-hidden="true"
          viewBox="0 0 16 16"
          className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 fill-field-fg"
        >
          <path d="M2 5l6 6 6-6z" />
        </svg>
      </span>
      <input
        type="range"
        className="fader-slim w-16 min-w-10 shrink"
        min={-40}
        max={6}
        step={1}
        value={level}
        aria-label="Metronome level"
        aria-valuetext={`${level.toFixed(0)} dB`}
        title={`Metronome level: ${level.toFixed(0)} dB; double-click for -6 dB`}
        onChange={(e) => {
          const v = Number(e.target.value);
          setLevel(v);
          set({ levelDb: v });
        }}
        onDoubleClick={() => {
          setLevel(-6);
          set({ levelDb: -6 });
        }}
      />
    </span>
  );
}
