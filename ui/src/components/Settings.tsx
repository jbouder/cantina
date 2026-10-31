import type { EngineStatus } from "../lib/engine";
import { SKINS, useSkin } from "../skins";
import { Button } from "./Button";
import { DevicePicker } from "./DevicePicker";
import { Sheet } from "./Sheet";

type Props = {
  onClose: () => void;
  onStatusChange: (status: EngineStatus) => void;
};

/** The settings sheet: audio devices and the skin picker. */
export function Settings({ onClose, onStatusChange }: Props) {
  const [skin, setSkin] = useSkin();
  return (
    <Sheet title="Settings" onClose={onClose}>
      <section aria-labelledby="audio-heading" className="grid gap-2">
        <h3 id="audio-heading" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
          Audio
        </h3>
        <DevicePicker onStatusChange={onStatusChange} />
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
