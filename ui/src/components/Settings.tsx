import { SKINS, useSkin } from "../skins";
import { Button } from "./Button";
import { Sheet } from "./Sheet";

type Props = { onClose: () => void };

/** The settings sheet. For now it holds the skin picker. */
export function Settings({ onClose }: Props) {
  const [skin, setSkin] = useSkin();
  return (
    <Sheet title="Settings" onClose={onClose}>
      <div role="radiogroup" aria-labelledby="skin-label" className="grid gap-2">
        <span id="skin-label" className="text-sm">
          Skin
          <small className="block text-ink-dim text-xs">
            Changes the whole studio's look. Remembered for next time.
          </small>
        </span>
        <div className="flex flex-wrap gap-1">
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
      </div>
    </Sheet>
  );
}
