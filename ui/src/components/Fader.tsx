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

/** The name at the left of a row; a long one is cut and kept in its tooltip. */
const LABEL = "caption truncate";
/** One row: the name, the control, and for a fader its value. */
const ROW = "grid min-w-0 grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-2";
/** Positions on a log-mapped slider. */
const LOG_STEPS = 1000;

/**
 * One pedal parameter on one line: its name, then a fader with its value
 * beside it, or for a switch a key, or for a pick from a list a
 * drop-down. Double-click resets a fader.
 */
export function Fader({ param, value, onChange }: Props) {
  const id = useId();
  if (param.toggle) {
    const on = value >= 0.5;
    return (
      <div className={ROW}>
        <span id={id} className={LABEL} title={param.name}>
          {param.name}
        </span>
        <Button
          aria-pressed={on}
          aria-labelledby={id}
          onClick={() => onChange(on ? 0 : 1)}
          className="min-h-6 justify-self-start"
        >
          <Led on={on} label={param.name} />
          {on ? "On" : "Off"}
        </Button>
      </div>
    );
  }
  if (param.choices.length > 0) {
    return (
      <div className={ROW}>
        <label htmlFor={id} className={LABEL} title={param.name}>
          {param.name}
        </label>
        <span className="relative min-w-0">
          <select
            id={id}
            className="surface-field min-h-6 w-full appearance-none truncate py-0 pr-6 pl-2"
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
            className="pointer-events-none absolute top-1/2 right-2 size-3 -translate-y-1/2 fill-field-fg"
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
    <div className={ROW}>
      <label htmlFor={id} className={LABEL} title={param.name}>
        {param.name}
      </label>
      <span className="grid min-w-0 grid-cols-[minmax(0,1fr)_3.75rem] items-center gap-2">
        <input
          id={id}
          type="range"
          className="fader-slim"
          min={log ? 0 : param.min}
          max={log ? LOG_STEPS : param.max}
          step={log ? 1 : step}
          value={toSlider(value)}
          onChange={(e) => onChange(fromSlider(Number(e.target.value)))}
          onDoubleClick={() => onChange(param.default)}
          title="Double-click to reset"
          aria-valuemin={param.min}
          aria-valuemax={param.max}
          aria-valuenow={value}
          aria-valuetext={format(value, param.unit)}
        />
        <output
          htmlFor={id}
          className="surface-lcd px-1 py-0.5 text-right font-lcd text-label leading-none tabular-nums"
        >
          {format(value, param.unit)}
        </output>
      </span>
    </div>
  );
}
