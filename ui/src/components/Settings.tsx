import type { SkinPreview } from "@cantina/skins";
import { type EngineStatus, latencyWarningSet, ms, routeLabel } from "../lib/engine";
import { useReduceFlashing } from "../lib/motion";
import { SKINS, useSkin } from "../skins";
import { Button } from "./Button";
import { DevicePicker } from "./DevicePicker";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  status: EngineStatus;
  /** True while a start or stop is in flight. */
  busy: boolean;
  onToggleEngine: () => void;
  onClose: () => void;
  onStatusChange: (status: EngineStatus) => void;
};

const WARN_CHOICES = [5, 10, 20, 50, 100];

/**
 * A skin in miniature, in its own colors whatever skin is on: its frame
 * with the title stripe, a display with its ink, and a key.
 */
function SkinSwatch({ preview }: { preview: SkinPreview }) {
  return (
    <span
      aria-hidden="true"
      className="relative inline-grid h-4 w-7 shrink-0 grid-cols-[1fr_6px] gap-px overflow-hidden rounded-[2px] p-[2px] pt-[4px]"
      style={{ background: preview.frame }}
    >
      <span className="absolute inset-x-0 top-0 h-[2px]" style={{ background: preview.title }} />
      <span className="grid place-items-center" style={{ background: preview.lcd }}>
        <span className="h-px w-3/4" style={{ background: preview.lcdInk }} />
      </span>
      <span style={{ background: preview.key }} />
    </span>
  );
}

/**
 * The settings sheet: the engine, audio devices, the latency warning, the
 * skin picker and Reduce flashing.
 */
export function Settings({ status, busy, onToggleEngine, onClose, onStatusChange }: Props) {
  const [skin, setSkin] = useSkin();
  const [reduceFlashing, setReduceFlashing] = useReduceFlashing();
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
        <div className="flex items-center gap-2">
          <Lcd aria-live="polite" className="min-w-0 flex-1 text-base">
            {routeLabel(status)}
          </Lcd>
          <Button
            variant="transport"
            aria-pressed={status.running}
            disabled={busy}
            onClick={onToggleEngine}
            className="shrink-0"
          >
            {busy ? "…" : status.running ? "Stop engine" : "Start engine"}
          </Button>
        </div>
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
              <SkinSwatch preview={s.preview} />
              {s.name}
            </Button>
          ))}
        </div>
      </section>
      <section aria-labelledby="motion-label" className="grid gap-2">
        <h3 id="motion-label" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Motion
          <small className="block font-normal text-ink-dim text-xs normal-case tracking-normal">
            Stills the stage's flashes and the skin's beat pulse. Your system's reduce motion
            setting stills the pulse too.
          </small>
        </h3>
        <div className="flex">
          <Button
            aria-pressed={reduceFlashing}
            onClick={() => setReduceFlashing(!reduceFlashing)}
            variant={reduceFlashing ? "transport" : "panel"}
          >
            Reduce flashing
          </Button>
        </div>
      </section>
    </Sheet>
  );
}
