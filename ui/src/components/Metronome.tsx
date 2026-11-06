import { useEffect, useState } from "react";
import { type EngineStatus, type MetronomeStatus, metronomeSet } from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";

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
    <span className="flex items-center gap-1">
      <Button
        variant="transport"
        aria-pressed={on}
        onClick={() => set({ on: !on })}
        title="Metronome (K)"
      >
        <Led on={on} label="Metronome" />
        Click
      </Button>
      <span className="relative">
        <select
          aria-label="Metronome sound"
          title="Metronome sound"
          className="surface-lcd min-h-8 w-28 appearance-none truncate py-0 pr-7 pl-2 font-lcd text-base"
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
          className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 fill-lcd-fg"
        >
          <path d="M2 5l6 6 6-6z" />
        </svg>
      </span>
      <input
        type="range"
        className="fader-slim w-20"
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
