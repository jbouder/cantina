import { useEffect, useState } from "react";
import {
  type EngineStatus,
  PAD_MIX_DEFAULT,
  type PadMix,
  padMixSet,
  type TrackStatus,
} from "../lib/engine";

type Props = {
  /** A drum track: `track.kit` is set. */
  track: TrackStatus;
  run: (action: Promise<EngineStatus>) => void;
};

/** One of a pad's faders: its range, how its value reads, and its field. */
interface Control {
  field: "levelDb" | "pan" | "tune" | "decay";
  label: string;
  min: number;
  max: number;
  step: number;
  show: (v: number) => string;
}

const CONTROLS: Control[] = [
  {
    field: "levelDb",
    label: "Level",
    min: -60,
    max: 12,
    step: 0.5,
    show: (v) => `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`,
  },
  {
    field: "pan",
    label: "Pan",
    min: -1,
    max: 1,
    step: 0.05,
    show: (v) => (v === 0 ? "C" : `${v < 0 ? "L" : "R"}${Math.round(Math.abs(v) * 100)}`),
  },
  {
    field: "tune",
    label: "Tune",
    min: -12,
    max: 12,
    step: 0.5,
    show: (v) => `${v > 0 ? "+" : ""}${v} st`,
  },
  {
    field: "decay",
    label: "Decay",
    min: 0.05,
    max: 1,
    step: 0.05,
    show: (v) => `${Math.round(v * 100)}%`,
  },
];

/**
 * A fader that moves freely while dragged and sends one edit when let
 * go; double-click puts it back to the kit's value.
 */
function PadFader({
  label,
  control,
  value,
  onCommit,
}: {
  label: string;
  control: Control;
  value: number;
  onCommit: (value: number) => void;
}) {
  const [shown, setShown] = useState(value);
  useEffect(() => setShown(value), [value]);
  const commit = () => {
    if (shown !== value) onCommit(shown);
  };
  return (
    <span className="flex items-center gap-1.5">
      <input
        type="range"
        className="fader w-24"
        aria-label={label}
        aria-valuetext={control.show(shown)}
        min={control.min}
        max={control.max}
        step={control.step}
        value={shown}
        onChange={(e) => setShown(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
        onDoubleClick={() => onCommit(PAD_MIX_DEFAULT[control.field])}
      />
      <output className="surface-lcd w-16 px-1 py-0.5 text-right font-lcd text-sm leading-none tabular-nums">
        <span className="lcd-text">{control.show(shown)}</span>
      </output>
    </span>
  );
}

/**
 * The pad mixer: each pad's level, pan, tune and decay, and mute, saved
 * with the drum track. Level and mute are smoothed as they change; pan,
 * tune and decay apply from the next hit. Each change is one edit.
 */
export function PadMixer({ track, run }: Props) {
  const pads = track.kit?.pads ?? [];
  const set = (pad: string, mix: PadMix) => run(padMixSet(track.id, pad, mix));
  return (
    <section aria-label={`${track.name} pad mixer`} className="surface-lcd overflow-x-auto p-2">
      <table className="border-separate border-spacing-x-2 border-spacing-y-0.5">
        <thead>
          <tr className="font-bold text-[11px] text-lcd-fg uppercase tracking-[0.08em]">
            <th scope="col" className="text-left">
              <span className="lcd-text">Pad</span>
            </th>
            <th scope="col">
              <span className="lcd-text">Mute</span>
            </th>
            {CONTROLS.map((c) => (
              <th key={c.field} scope="col" className="text-left">
                <span className="lcd-text">{c.label}</span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {pads.map((pad) => {
            const mix = track.pads[pad.id] ?? PAD_MIX_DEFAULT;
            return (
              <tr key={pad.id}>
                <th
                  scope="row"
                  className={`lcd-text w-24 max-w-24 truncate text-left font-lcd font-normal text-base ${mix.mute ? "opacity-40" : ""}`}
                >
                  {pad.name}
                </th>
                <td>
                  <button
                    type="button"
                    aria-pressed={mix.mute}
                    aria-label={`Mute ${pad.name}`}
                    onClick={() => set(pad.id, { ...mix, mute: !mix.mute })}
                    className={`h-7 w-7 border-0 p-0 font-bold font-lcd text-xs ${mix.mute ? "bg-lcd-fg text-lcd-bg" : "bg-tl-lane text-lcd-fg"}`}
                  >
                    M
                  </button>
                </td>
                {CONTROLS.map((c) => (
                  <td key={c.field}>
                    <PadFader
                      label={`${pad.name} ${c.label.toLowerCase()}`}
                      control={c}
                      value={mix[c.field]}
                      onCommit={(v) => set(pad.id, { ...mix, [c.field]: v })}
                    />
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
