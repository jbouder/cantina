import { useEffect, useState } from "react";
import {
  chooseMixdownPath,
  type ExportOptions,
  type ExportResult,
  mixdownExport,
  onRenderProgress,
  type RenderProgress,
  renderCancel,
} from "../lib/engine";
import { Button } from "./Button";
import { Toggle } from "./Field";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  projectName: string;
  /** The project's rate, offered first. */
  sampleRate: number;
  onClose: () => void;
};

/** Fraction done, 0 to 1, of the render this sheet started, or null when none runs. */
export function useRenderProgress(what: RenderProgress["what"]): number | null {
  const [fraction, setFraction] = useState<number | null>(null);
  useEffect(() => onRenderProgress((p) => p.what === what && setFraction(p.fraction)), [what]);
  return fraction;
}

/** A bar for a render's progress, in the display material. */
export function RenderBar({ label, fraction }: { label: string; fraction: number }) {
  const percent = Math.round(fraction * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      className="surface-lcd h-3 min-w-24 flex-1 overflow-hidden p-0.5"
    >
      <div className="h-full bg-lcd-fg" style={{ width: `${percent}%` }} />
    </div>
  );
}

const clock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

/**
 * The export sheet: the whole song or a range of bars, the rate and bit
 * depth, whether the tail rings out, then a save dialog and a render
 * faster than real time with its progress and a way to stop it.
 */
export function ExportSheet({ projectName, sampleRate, onClose }: Props) {
  const [rate, setRate] = useState<ExportOptions["sampleRate"]>(
    sampleRate === 44100 ? 44100 : 48000,
  );
  const [bits, setBits] = useState<ExportOptions["bits"]>(24);
  const [whole, setWhole] = useState(true);
  const [fromBar, setFromBar] = useState("1");
  const [toBar, setToBar] = useState("8");
  const [tail, setTail] = useState(true);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<ExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const progress = useRenderProgress("export");

  const from = Number.parseInt(fromBar, 10);
  const to = Number.parseInt(toBar, 10);
  const rangeValid = whole || (from >= 1 && to >= from);

  const start = async () => {
    setError(null);
    setResult(null);
    const path = await chooseMixdownPath(projectName).catch((e) => {
      setError(String(e));
      return null;
    });
    if (!path) return;
    setRunning(true);
    try {
      setResult(
        await mixdownExport({
          path,
          sampleRate: rate,
          bits,
          fromBar: whole ? null : from,
          toBar: whole ? null : to,
          tail,
        }),
      );
    } catch (e) {
      const message = String(e);
      if (!message.includes("canceled")) setError(message);
    } finally {
      setRunning(false);
    }
  };

  return (
    <Sheet
      title="Export mixdown"
      onClose={running ? () => {} : onClose}
      footer={
        running ? (
          <>
            <RenderBar label="Exporting" fraction={progress ?? 0} />
            <Button onClick={() => renderCancel()}>Stop</Button>
          </>
        ) : (
          <>
            <Button onClick={onClose}>{result ? "Done" : "Cancel"}</Button>
            <Button tier="primary" disabled={!rangeValid} onClick={start}>
              Export…
            </Button>
          </>
        )
      }
    >
      <section aria-labelledby="export-range" className="grid gap-2">
        <h3 id="export-range" className="caption m-0">
          Range
        </h3>
        <div role="radiogroup" aria-labelledby="export-range" className="flex gap-1">
          <Button role="radio" aria-checked={whole} onClick={() => setWhole(true)}>
            Whole song
          </Button>
          <Button role="radio" aria-checked={!whole} onClick={() => setWhole(false)}>
            Bars
          </Button>
        </div>
        {whole ? null : (
          <>
            <TextInput
              id="export-from"
              label="From bar"
              inputMode="numeric"
              value={fromBar}
              onChange={(e) => setFromBar(e.target.value)}
            />
            <TextInput
              id="export-to"
              label="To bar"
              hint="Included"
              inputMode="numeric"
              value={toBar}
              onChange={(e) => setToBar(e.target.value)}
            />
          </>
        )}
      </section>
      <section aria-labelledby="export-format" className="grid gap-2">
        <h3 id="export-format" className="caption m-0">
          File
        </h3>
        <Select
          id="export-rate"
          label="Sample rate"
          hint={
            rate === sampleRate ? "The project's rate" : `Converted from ${sampleRate / 1000} kHz`
          }
          value={rate}
          onChange={(e) => setRate(Number(e.target.value) as ExportOptions["sampleRate"])}
        >
          <option value={48000}>48 kHz</option>
          <option value={44100}>44.1 kHz</option>
        </Select>
        <Select
          id="export-bits"
          label="Bit depth"
          hint={bits === 32 ? "Float: no dither, nothing clips" : "Dithered"}
          value={bits}
          onChange={(e) => setBits(Number(e.target.value) as ExportOptions["bits"])}
        >
          <option value={24}>24-bit</option>
          <option value={16}>16-bit</option>
          <option value={32}>32-bit float</option>
        </Select>
        <Toggle
          id="export-tail"
          label="Let the tail ring out"
          hint="Keeps rendering past the end until reverbs and delays have died away."
          checked={tail}
          onChange={setTail}
        />
      </section>
      {result ? (
        <Lcd role="status" className="text-readout">
          Wrote {result.path.split("/").pop()} · {clock(result.seconds)} in{" "}
          {result.renderSeconds.toFixed(1)} s (
          {Math.max(1, Math.round(result.seconds / Math.max(result.renderSeconds, 0.001)))}× real
          time) · peak {result.peakDb.toFixed(1)} dBFS
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
