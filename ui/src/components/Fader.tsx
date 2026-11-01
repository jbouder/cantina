import { useId } from "react";
import type { ParamInfo } from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";

type Props = {
  param: ParamInfo;
  value: number;
  onChange: (value: number) => void;
};

function format(value: number, unit: string) {
  const digits = unit === "Hz" ? 0 : unit === "ms" && value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)}${unit ? ` ${unit}` : ""}`;
}

const LABEL = "font-bold text-[11px] uppercase tracking-[0.08em]";
/** Positions on a log-mapped slider. */
const LOG_STEPS = 1000;

/**
 * One pedal parameter: a labelled fader with a readout, or for a switch
 * a button, or for a pick from a list a drop-down. Double-click resets
 * a fader.
 */
export function Fader({ param, value, onChange }: Props) {
  const id = useId();
  if (param.toggle) {
    const on = value >= 0.5;
    return (
      <div className="flex min-w-0 items-center justify-between gap-2">
        <span id={id} className={LABEL}>
          {param.name}
        </span>
        <Button
          aria-pressed={on}
          aria-labelledby={id}
          onClick={() => onChange(on ? 0 : 1)}
          variant={on ? "transport" : "panel"}
        >
          <Led on={on} label={param.name} />
          {on ? "On" : "Off"}
        </Button>
      </div>
    );
  }
  if (param.choices.length > 0) {
    return (
      <div className="grid min-w-0 gap-0.5">
        <label htmlFor={id} className={LABEL}>
          {param.name}
        </label>
        <span className="relative">
          <select
            id={id}
            className="surface-lcd min-h-8 w-full appearance-none truncate py-0 pr-8 pl-2 font-lcd text-base"
            value={Math.round(value)}
            onChange={(e) => onChange(Number(e.target.value))}
          >
            {param.choices.map((choice, i) => (
              <option key={choice} value={i}>
                {choice}
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
      </div>
    );
  }
  const span = param.max - param.min;
  // A frequency over a wide range moves in octaves, not hertz, so the
  // slider works in the log domain.
  const log = param.unit === "Hz" && param.min > 0 && param.max / param.min >= 16;
  const step = param.unit === "Hz" ? 1 : span > 50 ? 0.5 : 0.1;
  const toSlider = (v: number) =>
    log ? (Math.log(v / param.min) / Math.log(param.max / param.min)) * LOG_STEPS : v;
  const fromSlider = (t: number) =>
    log ? Math.round(param.min * (param.max / param.min) ** (t / LOG_STEPS)) : t;
  return (
    <div className="grid min-w-0 gap-0.5">
      <div className="flex items-baseline justify-between gap-2">
        <label htmlFor={id} className={LABEL}>
          {param.name}
        </label>
        <output
          htmlFor={id}
          className="surface-lcd px-1.5 py-0.5 font-lcd text-sm leading-none tabular-nums"
        >
          <span className="lcd-text">{format(value, param.unit)}</span>
        </output>
      </div>
      <input
        id={id}
        type="range"
        className="fader"
        min={log ? 0 : param.min}
        max={log ? LOG_STEPS : param.max}
        step={log ? 1 : step}
        value={toSlider(value)}
        onChange={(e) => onChange(fromSlider(Number(e.target.value)))}
        onDoubleClick={() => onChange(param.default)}
        aria-valuemin={param.min}
        aria-valuemax={param.max}
        aria-valuenow={value}
        aria-valuetext={format(value, param.unit)}
      />
    </div>
  );
}
