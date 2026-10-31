import { useCallback, useEffect, useState } from "react";
import {
  type AudioDevice,
  type AudioSettings,
  audioSettings,
  type EngineStatus,
  listDevices,
  onStatus,
  setAudioSettings,
} from "../lib/engine";
import { Select } from "./Select";

const DEFAULT = "";

type Props = {
  /** Called with the status the engine returns after a change. */
  onStatusChange: (status: EngineStatus) => void;
};

function resolve(devices: AudioDevice[], uid: string | null, input: boolean) {
  return uid
    ? devices.find((d) => d.uid === uid)
    : devices.find((d) => (input ? d.defaultInput : d.defaultOutput));
}

function formatRate(rate: number) {
  return rate >= 1000 ? `${(rate / 1000).toFixed(rate % 1000 ? 1 : 0)} kHz` : `${rate} Hz`;
}

/**
 * Input and output device, sample rate and buffer size. Every change is
 * saved by the shell and, if the stream is open, applied by rebuilding it.
 */
export function DevicePicker({ onStatusChange }: Props) {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [settings, setSettings] = useState<AudioSettings | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    Promise.all([listDevices(), audioSettings()]).then(
      ([d, s]) => {
        setDevices(d);
        setSettings(s);
      },
      (e) => setError(String(e)),
    );
  }, []);

  useEffect(() => {
    refresh();
    // A device coming or going changes both the list and what is open.
    return onStatus(refresh);
  }, [refresh]);

  const apply = useCallback(
    async (patch: Partial<AudioSettings>) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      setSettings(next);
      try {
        onStatusChange(await setAudioSettings(next));
      } catch (e) {
        setError(String(e));
      }
    },
    [settings, onStatusChange],
  );

  if (!settings) {
    return <p className="m-0 text-ink-dim text-sm">{error ?? "Looking for devices…"}</p>;
  }

  const inputs = devices.filter((d) => d.inputChannels > 0);
  const outputs = devices.filter((d) => d.outputChannels > 0);
  const input = resolve(devices, settings.inputUid, true);
  const output = resolve(devices, settings.outputUid, false);
  const chosen = [input, output].filter((d): d is AudioDevice => d !== undefined);

  // Offer what both devices accept; with nothing resolved, the usual rates.
  const rates =
    chosen.length > 0
      ? chosen
          .map((d) => d.sampleRates)
          .reduce((a, b) => a.filter((r) => b.includes(r)))
          .sort((a, b) => a - b)
      : [44100, 48000, 88200, 96000];
  const bufferMin = Math.max(16, ...chosen.map((d) => d.bufferMin));
  const bufferMax = Math.min(4096, ...chosen.map((d) => d.bufferMax));
  const buffers: number[] = [];
  for (let n = 16; n <= 4096; n *= 2) {
    if (n >= bufferMin && n <= bufferMax) buffers.push(n);
  }
  const rateOptions = rates.includes(settings.sampleRate)
    ? rates
    : [...rates, settings.sampleRate].sort((a, b) => a - b);
  const bufferOptions = buffers.includes(settings.bufferFrames)
    ? buffers
    : [...buffers, settings.bufferFrames].sort((a, b) => a - b);

  const defaultName = (list: AudioDevice[], input: boolean) =>
    list.find((d) => (input ? d.defaultInput : d.defaultOutput))?.name;

  return (
    <div className="grid gap-2">
      <Select
        id="audio-input"
        label="Input"
        hint="Where the instrument comes in."
        value={settings.inputUid ?? DEFAULT}
        onChange={(e) => apply({ inputUid: e.target.value || null })}
      >
        <option value={DEFAULT}>System default ({defaultName(inputs, true) ?? "none"})</option>
        {inputs.map((d) => (
          <option key={d.uid} value={d.uid}>
            {d.name} ({d.inputChannels} in)
          </option>
        ))}
      </Select>
      <Select
        id="audio-output"
        label="Output"
        value={settings.outputUid ?? DEFAULT}
        onChange={(e) => apply({ outputUid: e.target.value || null })}
      >
        <option value={DEFAULT}>System default ({defaultName(outputs, false) ?? "none"})</option>
        {outputs.map((d) => (
          <option key={d.uid} value={d.uid}>
            {d.name} ({d.outputChannels} out)
          </option>
        ))}
      </Select>
      <Select
        id="audio-rate"
        label="Sample rate"
        hint="Rates both devices support."
        value={settings.sampleRate}
        onChange={(e) => apply({ sampleRate: Number(e.target.value) })}
      >
        {rateOptions.map((r) => (
          <option key={r} value={r}>
            {formatRate(r)}
          </option>
        ))}
      </Select>
      <Select
        id="audio-buffer"
        label="Buffer size"
        hint={`Smaller is faster to respond; ${bufferMin} to ${bufferMax} frames here.`}
        value={settings.bufferFrames}
        onChange={(e) => apply({ bufferFrames: Number(e.target.value) })}
      >
        {bufferOptions.map((b) => (
          <option key={b} value={b}>
            {b} frames · {((b / settings.sampleRate) * 1000).toFixed(1)} ms
          </option>
        ))}
      </Select>
      {error ? (
        <p role="alert" className="m-0 font-lcd text-lcd-error">
          {error}
        </p>
      ) : null}
    </div>
  );
}
