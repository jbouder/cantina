import { useId } from "react";
import type { ParamInfo } from "../lib/engine";

type Props = {
  param: ParamInfo;
  value: number;
  onChange: (value: number) => void;
};

function format(value: number, unit: string) {
  const digits = unit === "Hz" ? 0 : 1;
  return `${value.toFixed(digits)}${unit ? ` ${unit}` : ""}`;
}

/** One pedal parameter as a labelled fader with a readout. Double-click resets. */
export function Fader({ param, value, onChange }: Props) {
  const id = useId();
  const span = param.max - param.min;
  const step = param.unit === "Hz" ? 1 : span > 50 ? 0.5 : 0.1;
  return (
    <div className="grid gap-0.5">
      <div className="flex items-baseline justify-between gap-2">
        <label htmlFor={id} className="font-bold text-[11px] uppercase tracking-[0.08em]">
          {param.name}
        </label>
        <output htmlFor={id} className="font-lcd text-lcd-fg text-sm leading-none">
          {format(value, param.unit)}
        </output>
      </div>
      <input
        id={id}
        type="range"
        className="fader"
        min={param.min}
        max={param.max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        onDoubleClick={() => onChange(param.default)}
        aria-valuetext={format(value, param.unit)}
      />
    </div>
  );
}
