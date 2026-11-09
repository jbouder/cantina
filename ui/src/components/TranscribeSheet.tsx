import { useState } from "react";
import {
  type ClipInfo,
  clipTabRequantise,
  clipTranscribe,
  type EngineStatus,
  type Instrument,
  renderCancel,
  type TabGrid,
} from "../lib/engine";
import { Button } from "./Button";
import { RenderBar, useRenderProgress } from "./ExportSheet";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  clip: ClipInfo;
  onStatus: (status: EngineStatus) => void;
  onClose: () => void;
};

/** A named tuning: the open strings from the lowest, as MIDI notes. */
type Tuning = { name: string; notes: number[] };

/** The tunings offered for each instrument and string count. */
export const TUNINGS: Record<Instrument, Record<number, Tuning[]>> = {
  guitar: {
    6: [
      { name: "Standard (E A D G B E)", notes: [40, 45, 50, 55, 59, 64] },
      { name: "Drop D (D A D G B E)", notes: [38, 45, 50, 55, 59, 64] },
      { name: "E♭ standard", notes: [39, 44, 49, 54, 58, 63] },
      { name: "D standard", notes: [38, 43, 48, 53, 57, 62] },
      { name: "Drop C (C G C F A D)", notes: [36, 43, 48, 53, 57, 62] },
      { name: "DADGAD", notes: [38, 45, 50, 55, 57, 62] },
      { name: "Open G (D G D G B D)", notes: [38, 43, 50, 55, 59, 62] },
      { name: "Baritone B (B E A D F♯ B)", notes: [35, 40, 45, 50, 54, 59] },
    ],
    7: [
      { name: "Standard (B E A D G B E)", notes: [35, 40, 45, 50, 55, 59, 64] },
      { name: "Drop A (A E A D G B E)", notes: [33, 40, 45, 50, 55, 59, 64] },
    ],
    8: [
      { name: "Standard (F♯ B E A D G B E)", notes: [30, 35, 40, 45, 50, 55, 59, 64] },
      { name: "Drop E (E B E A D G B E)", notes: [28, 35, 40, 45, 50, 55, 59, 64] },
    ],
  },
  bass: {
    4: [
      { name: "Standard (E A D G)", notes: [28, 33, 38, 43] },
      { name: "Drop D (D A D G)", notes: [26, 33, 38, 43] },
      { name: "D standard (D G C F)", notes: [26, 31, 36, 41] },
    ],
    5: [{ name: "Standard (B E A D G)", notes: [23, 28, 33, 38, 43] }],
    6: [{ name: "Standard (B E A D G C)", notes: [23, 28, 33, 38, 43, 48] }],
  },
};

const GRIDS: { value: TabGrid; name: string }[] = [
  { value: "1/16", name: "Sixteenths" },
  { value: "1/8", name: "Eighths" },
  { value: "1/16t", name: "Sixteenth triplets" },
  { value: "1/8t", name: "Eighth triplets" },
];

/** The tuning a tab already has, by name, or the first for its count. */
function tuningIndex(instrument: Instrument, tuning: number[] | undefined): number {
  const list = TUNINGS[instrument][tuning?.length ?? 0] ?? [];
  const i = list.findIndex((t) => t.notes.join() === tuning?.join());
  return Math.max(0, i);
}

/**
 * Transcribe to tab: choose the instrument, tuning, capo and grid, then
 * the model listens to the clip off the audio thread, with progress and
 * Stop. A clip with a transcribed tab can also be re-quantised to
 * another grid without listening again.
 */
export function TranscribeSheet({ clip, onStatus, onClose }: Props) {
  const [instrument, setInstrument] = useState<Instrument>(clip.tab?.instrument ?? "guitar");
  const [strings, setStrings] = useState(
    clip.tab?.tuning.length ?? (instrument === "bass" ? 4 : 6),
  );
  const [tuning, setTuning] = useState(() => tuningIndex(instrument, clip.tab?.tuning));
  const [capo, setCapo] = useState(clip.tab?.capo ?? 0);
  const [grid, setGrid] = useState<TabGrid>("1/16");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const progress = useRenderProgress("transcribe");
  const tunings = TUNINGS[instrument][strings] ?? [];

  const chooseInstrument = (next: Instrument) => {
    setInstrument(next);
    const count = next === "bass" ? 4 : 6;
    setStrings(count);
    setTuning(0);
  };

  const transcribe = async () => {
    setError(null);
    setRunning(true);
    try {
      onStatus(
        await clipTranscribe(clip.id, {
          instrument,
          tuning: tunings[tuning]?.notes ?? [],
          capo,
          grid,
        }),
      );
      onClose();
    } catch (e) {
      const message = String(e);
      if (!message.includes("cancelled")) setError(message);
    } finally {
      setRunning(false);
    }
  };

  const requantise = () => {
    setError(null);
    clipTabRequantise(clip.id, grid).then(
      (status) => {
        onStatus(status);
        onClose();
      },
      (e) => setError(String(e)),
    );
  };

  return (
    <Sheet title={`Transcribe ${clip.name} to tab`} onClose={running ? () => {} : onClose}>
      <section aria-labelledby="tab-instrument" className="grid gap-2">
        <h3 id="tab-instrument" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Instrument
        </h3>
        <div role="radiogroup" aria-labelledby="tab-instrument" className="flex gap-1">
          {(["guitar", "bass"] as const).map((i) => (
            <Button
              key={i}
              role="radio"
              aria-checked={instrument === i}
              variant={instrument === i ? "transport" : "panel"}
              disabled={running}
              onClick={() => chooseInstrument(i)}
            >
              {i === "guitar" ? "Guitar" : "Bass"}
            </Button>
          ))}
        </div>
        <Select
          id="tab-strings"
          label="Strings"
          value={strings}
          disabled={running}
          onChange={(e) => {
            setStrings(Number(e.target.value));
            setTuning(0);
          }}
        >
          {Object.keys(TUNINGS[instrument]).map((n) => (
            <option key={n} value={n}>
              {n} strings
            </option>
          ))}
        </Select>
        <Select
          id="tab-tuning"
          label="Tuning"
          value={tuning}
          disabled={running}
          onChange={(e) => setTuning(Number(e.target.value))}
        >
          {tunings.map((t, i) => (
            <option key={t.name} value={i}>
              {t.name}
            </option>
          ))}
        </Select>
        <Select
          id="tab-capo"
          label="Capo"
          hint="Frets in the tab count from the capo"
          value={capo}
          disabled={running}
          onChange={(e) => setCapo(Number(e.target.value))}
        >
          {Array.from({ length: 13 }, (_, fret) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: the fret is the value
            <option key={fret} value={fret}>
              {fret === 0 ? "None" : `Fret ${fret}`}
            </option>
          ))}
        </Select>
      </section>
      <section aria-labelledby="tab-timing" className="grid gap-2">
        <h3 id="tab-timing" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Timing
        </h3>
        <Select
          id="tab-grid"
          label="Quantise to"
          hint="Notes snap to this grid at the project's tempo"
          value={grid}
          disabled={running}
          onChange={(e) => setGrid(e.target.value as TabGrid)}
        >
          {GRIDS.map((g) => (
            <option key={g.value} value={g.value}>
              {g.name}
            </option>
          ))}
        </Select>
      </section>
      <div className="flex items-center gap-2">
        {running ? (
          <>
            <RenderBar label="Transcribing" fraction={progress ?? 0} />
            <Button onClick={() => renderCancel()}>Stop</Button>
          </>
        ) : (
          <>
            <span className="flex-1" />
            {clip.tab?.transcribed ? (
              <Button onClick={requantise} title="Snap the notes it heard to the grid again">
                Re-quantise
              </Button>
            ) : null}
            <Button variant="transport" onClick={transcribe}>
              {clip.tab ? "Transcribe again" : "Transcribe"}
            </Button>
          </>
        )}
      </div>
      {clip.tab ? (
        <Lcd role="status" className="text-base">
          Has a {clip.tab.instrument} tab · {clip.tab.notes} notes
          {clip.tab.transcribed ? " · re-quantising keeps what it heard" : ""}
        </Lcd>
      ) : null}
      {error ? (
        <Lcd role="alert" error>
          {error}
        </Lcd>
      ) : null}
    </Sheet>
  );
}
