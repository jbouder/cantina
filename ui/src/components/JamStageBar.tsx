import { useState } from "react";
import {
  type EngineStatus,
  type GrooveSection,
  type JamStatus,
  jamSection,
  jamTempo,
  type MeterFrame,
} from "../lib/engine";
import { addTap, clampTempo, JAM_SECTIONS, jamPlace, SECTION_LABELS, tapTempo } from "../lib/jam";
import { Button } from "./Button";
import { Led } from "./Led";

type Props = {
  jam: JamStatus;
  frame: MeterFrame;
  sampleRate: number;
  onPlay: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The jam's controls over the stage in performance mode: play, where the
 * jam is and what is coming, the sections and the tempo. The keys are
 * the Jam view's, which stays underneath.
 */
export function JamStageBar({ jam, frame, sampleRate, onPlay, onStatus, onError }: Props) {
  const report = (action: Promise<EngineStatus>) =>
    action.then(onStatus, (e) => onError(String(e)));
  const [taps, setTaps] = useState<number[]>([]);
  const tap = () => {
    const next = addTap(taps, performance.now());
    setTaps(next);
    const bpm = tapTempo(next);
    if (bpm !== null) report(jamTempo(bpm));
  };
  const rolling = frame.playing || frame.countIn > 0;
  const place = jamPlace(
    frame.position,
    jam.marks,
    sampleRate,
    jam.barTicks,
    jam.setup.timeSignature.numerator,
  );
  const tempo = Math.round(jam.tempo * 10) / 10;
  const playing = rolling ? (place?.section ?? null) : null;

  return (
    <section
      aria-label="Jam"
      className="surface-panel absolute inset-x-4 bottom-4 grid gap-3 p-3 opacity-90"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="transport" aria-pressed={rolling} onClick={onPlay} className="min-w-24">
          {rolling ? "Stop" : "Play"}
        </Button>
        <span aria-live="polite" className="min-w-0 flex-1 font-lcd text-lcd-fg text-lg">
          {frame.countIn > 0
            ? `Count ${frame.countIn}`
            : rolling && place
              ? `${SECTION_LABELS[place.section]} · bar ${place.bar}.${place.beat} · ${place.bpm} bpm`
              : `${jam.setup.name} · ${tempo} bpm`}
          {rolling && place?.next ? (
            <span className="ml-3 text-ink-dim text-sm">
              {SECTION_LABELS[place.next.section]} at bar {place.next.bar}
            </span>
          ) : null}
        </span>
        <span className="flex items-center gap-1">
          <Button
            aria-label="Slower"
            disabled={jam.tempoLocked}
            onClick={() => report(jamTempo(clampTempo(Math.round(tempo) - 1)))}
          >
            −
          </Button>
          <span className="min-w-16 text-center font-lcd text-lcd-fg tabular-nums">{tempo}</span>
          <Button
            aria-label="Faster"
            disabled={jam.tempoLocked}
            onClick={() => report(jamTempo(clampTempo(Math.round(tempo) + 1)))}
          >
            +
          </Button>
          <Button onClick={tap} disabled={jam.tempoLocked}>
            Tap
          </Button>
        </span>
      </div>
      <ul aria-label="Sections" className="m-0 grid list-none grid-cols-6 gap-2 p-0">
        {JAM_SECTIONS.map((section, i) => {
          const has = jam.sections.includes(section);
          return (
            <li key={section}>
              <button
                type="button"
                disabled={!has}
                aria-pressed={playing === section}
                aria-label={`${SECTION_LABELS[section]}, key ${i + 1}`}
                onClick={() => report(jamSection(section as GrooveSection))}
                className="surface-pbutton grid min-h-14 w-full place-items-center gap-1 border-0 px-2 py-1 disabled:opacity-40"
              >
                <Led on={playing === section} label={SECTION_LABELS[section]} />
                <span className="font-bold text-[11px] uppercase tracking-[0.06em]">
                  {SECTION_LABELS[section]}
                  <span className="ml-1 text-ink-dim">{i + 1}</span>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <span className="text-ink-dim text-xs">
        Space plays · 1–6 sections · ↑ ↓ tempo · T tap · Return keeps · ← → scenes · B hides this
      </span>
    </section>
  );
}
