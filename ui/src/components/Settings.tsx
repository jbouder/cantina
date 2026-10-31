import { type EngineStatus, latencyWarningSet, ms } from "../lib/engine";
import { SKINS, useSkin } from "../skins";
import { Button } from "./Button";
import { DevicePicker } from "./DevicePicker";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  status: EngineStatus;
  onClose: () => void;
  onStatusChange: (status: EngineStatus) => void;
};

const WARN_CHOICES = [5, 10, 20, 50, 100];

/** The settings sheet: audio devices, the latency warning and the skin picker. */
export function Settings({ status, onClose, onStatusChange }: Props) {
  const [skin, setSkin] = useSkin();
  const { latency } = status;
  const choices = WARN_CHOICES.includes(latency.warnMs)
    ? WARN_CHOICES
    : [...WARN_CHOICES, latency.warnMs].sort((a, b) => a - b);
  return (
    <Sheet title="Settings" onClose={onClose}>
      <section aria-labelledby="audio-heading" className="grid gap-2">
        <h3 id="audio-heading" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Audio
        </h3>
        <DevicePicker onStatusChange={onStatusChange} />
        <Select
          id="latency-warn"
          label="Warn when plugins delay playback by more than"
          hint={
            status.running
              ? `Now ${ms(latency.compensationFrames, status.sampleRate)}; the devices add ${ms(latency.inputFrames, status.sampleRate)} in and ${ms(latency.outputFrames, status.sampleRate)} out.`
              : "Live monitoring is never delayed to match other tracks."
          }
          value={latency.warnMs}
          onChange={(e) => latencyWarningSet(Number(e.target.value)).then(onStatusChange)}
        >
          {choices.map((c) => (
            <option key={c} value={c}>
              {c} ms
            </option>
          ))}
        </Select>
      </section>
      <section aria-labelledby="skin-label" className="grid gap-2">
        <h3 id="skin-label" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Skin
          <small className="block font-normal text-ink-dim text-xs normal-case tracking-normal">
            Changes the whole studio's look. Remembered for next time.
          </small>
        </h3>
        <div role="radiogroup" aria-labelledby="skin-label" className="flex flex-wrap gap-1">
          {SKINS.map((s) => (
            <Button
              key={s.id}
              role="radio"
              aria-checked={skin === s.id}
              onClick={() => setSkin(s.id)}
              variant={skin === s.id ? "transport" : "panel"}
            >
              {s.name}
            </Button>
          ))}
        </div>
      </section>
    </Sheet>
  );
}
