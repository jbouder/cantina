import { type KeyboardEvent, type PointerEvent, useState } from "react";
import type { PadInfo } from "../lib/engine";
import { KEY_VELOCITY, PAD_KEYS, padForKey } from "../lib/liveRecord";

type Props = {
  pads: PadInfo[];
  /** A pad played, how hard. */
  onHit: (pad: string, velocity: number) => void;
};

/**
 * The kit as pads to play in the drum designer: click one (harder toward
 * its top), or with the strip focused press A S D F… for the kit's pads in
 * order, ⇧ for an accent.
 */
export function DesignPads({ pads, onHit }: Props) {
  const [lit, setLit] = useState<string | null>(null);
  const hit = (index: number, velocity: number) => {
    const pad = pads[index];
    if (!pad) return;
    setLit(pad.id);
    window.setTimeout(() => setLit((l) => (l === pad.id ? null : l)), 120);
    onHit(pad.id, velocity);
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const index = padForKey(e.key);
    if (index === null || index >= pads.length) return;
    e.preventDefault();
    e.stopPropagation();
    if (!e.repeat) hit(index, e.shiftKey ? 1 : KEY_VELOCITY);
  };
  const press = (e: PointerEvent<HTMLButtonElement>, index: number) => {
    const box = e.currentTarget.getBoundingClientRect();
    const velocity =
      1 - 0.6 * Math.min(1, Math.max(0, (e.clientY - box.top) / Math.max(1, box.height)));
    hit(index, velocity);
  };
  return (
    <div
      role="application"
      aria-roledescription="drum pads"
      aria-label="Pads: the keys A S D F G H J K L ; Z X C V B N play them in order, Shift for an accent"
      onKeyDown={onKey}
      className="surface-lcd scroll-area flex gap-1 p-1"
    >
      {pads.map((pad, i) => (
        <button
          key={pad.id}
          type="button"
          tabIndex={i === 0 ? 0 : -1}
          aria-label={`${pad.name}${PAD_KEYS[i] ? ` (${PAD_KEYS[i].toUpperCase()})` : ""}`}
          onPointerDown={(e) => press(e, i)}
          className={`grid h-12 w-20 shrink-0 content-between border-0 p-1 text-left font-lcd ${
            lit === pad.id ? "bg-lcd-fg text-lcd-bg" : "bg-tl-lane text-lcd-fg"
          }`}
        >
          <span className="truncate text-label">{pad.name}</span>
          <span className="text-right text-label opacity-70">
            {PAD_KEYS[i]?.toUpperCase() ?? ""}
          </span>
        </button>
      ))}
    </div>
  );
}
