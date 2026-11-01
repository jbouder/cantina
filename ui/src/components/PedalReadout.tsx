import type { PedalInfo } from "../lib/engine";
import { noteFor } from "../lib/readouts";
import { Led } from "./Led";

type Props = {
  pedal: PedalInfo;
  /** The pedal's readout values from the latest frame, in `pedal.readouts` order. */
  values: number[] | undefined;
};

/**
 * What a pedal measures, drawn for its kind: a tuner gets a note and a
 * cents strip, a compressor a gain reduction meter, a gate an open
 * light, anything else a list of its values.
 */
export function PedalReadout({ pedal, values }: Props) {
  if (pedal.readouts.length === 0) return null;
  const at = (name: string) => values?.[pedal.readouts.indexOf(name)] ?? 0;
  if (pedal.spec === "tuner") {
    const reference = pedal.params.find((p) => p.name === "Reference")?.value ?? 440;
    return <TunerDisplay hz={at("Pitch")} clarity={at("Clarity")} reference={reference} />;
  }
  if (pedal.spec === "compressor") {
    return <ReductionMeter db={at("Reduction")} />;
  }
  if (pedal.spec === "gate") {
    const open = at("Open") >= 0.5;
    return (
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-[0.08em]">
        <Led on={open} label="Gate open" />
        {open ? "Open" : "Closed"}
      </div>
    );
  }
  return (
    <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-sm">
      {pedal.readouts.map((name, i) => (
        <div key={name} className="contents">
          <dt className="font-bold text-[11px] uppercase tracking-[0.08em]">{name}</dt>
          <dd className="m-0 text-right font-lcd tabular-nums">{(values?.[i] ?? 0).toFixed(2)}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Cents either side of the note the strip spans. */
const STRIP_CENTS = 50;
/** Lights across the strip, an odd number so one sits at zero. */
const LIGHTS = 11;
/** Within this many cents the centre light reads in tune. */
const IN_TUNE_CENTS = 3;

function TunerDisplay({
  hz,
  clarity,
  reference,
}: {
  hz: number;
  clarity: number;
  reference: number;
}) {
  const note = clarity > 0 ? noteFor(hz, reference) : null;
  const cents = note?.cents ?? 0;
  const inTune = note !== null && Math.abs(cents) < IN_TUNE_CENTS;
  // Which light the needle is nearest; the centre light is index 5.
  const lit =
    note === null ? -1 : Math.round(((cents + STRIP_CENTS) / (2 * STRIP_CENTS)) * (LIGHTS - 1));
  const description =
    note === null
      ? "No note"
      : inTune
        ? `${note.name}${note.octave}, in tune`
        : `${note.name}${note.octave}, ${Math.abs(Math.round(cents))} cents ${cents > 0 ? "sharp" : "flat"}`;
  return (
    <div className="grid gap-1" role="img" aria-label={description}>
      <div className="surface-lcd flex items-baseline justify-between px-2 py-1 font-lcd leading-none">
        <span className="lcd-text text-3xl">
          {note ? note.name : "–"}
          <span className="text-base">{note ? note.octave : ""}</span>
        </span>
        <span className={`lcd-text text-base tabular-nums ${inTune ? "" : "text-lcd-error"}`}>
          {note ? `${cents >= 0 ? "+" : "−"}${Math.abs(cents).toFixed(0)}¢` : ""}
        </span>
      </div>
      <div aria-hidden="true" className="flex justify-between px-1">
        {Array.from({ length: LIGHTS }, (_, i) => {
          const centre = i === (LIGHTS - 1) / 2;
          const on = i === lit;
          const color = !on ? "bg-led-off" : inTune && centre ? "bg-led-on" : "bg-vol-mid";
          return (
            <span
              key={`light-${String(i)}`}
              className={`block rounded-full ${centre ? "size-3" : "size-2 self-center"} ${color}`}
            />
          );
        })}
      </div>
      <div className="flex justify-between text-[10px] text-ink-dim uppercase tracking-[0.08em]">
        <span>♭</span>
        <span>{hz > 0 ? `${hz.toFixed(1)} Hz` : ""}</span>
        <span>♯</span>
      </div>
    </div>
  );
}

/** Most reduction the meter shows, in dB. */
const REDUCTION_FLOOR = -24;

function ReductionMeter({ db }: { db: number }) {
  const reduction = Math.min(0, Math.max(REDUCTION_FLOOR, db));
  const fraction = reduction / REDUCTION_FLOOR;
  return (
    <div className="grid grid-cols-[auto_1fr_3.5rem] items-center gap-2">
      <span className="font-bold text-[11px] uppercase tracking-[0.08em]">GR</span>
      <span className="relative block h-3">
        <meter
          aria-label="Gain reduction"
          min={REDUCTION_FLOOR}
          max={0}
          value={reduction}
          aria-valuetext={`${(-reduction).toFixed(1)} dB`}
          className="sr-only"
        />
        <span aria-hidden="true" className="surface-lcd absolute inset-0 block overflow-hidden">
          <span
            className="surface-meter absolute inset-0 block"
            style={{ clipPath: `inset(0 0 0 ${(1 - fraction) * 100}%)` }}
          />
        </span>
      </span>
      <span className="surface-lcd px-1 text-right font-lcd text-sm leading-tight tabular-nums">
        <span className="lcd-text">{`−${(-reduction).toFixed(1)}`}</span>
      </span>
    </div>
  );
}
