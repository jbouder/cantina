import { type KeyboardEvent, type PointerEvent, useRef } from "react";

type Props = {
  /** The arrangement's height above the divider, in pixels. */
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
};

/** The draggable bar between the arrangement and the editor; the arrows move it too. */
export function Divider({ value, min, max, onChange }: Props) {
  const start = useRef<{ y: number; value: number } | null>(null);
  const clamp = (v: number) => Math.min(max, Math.max(min, v));

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    start.current = { y: e.clientY, value };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const s = start.current;
    if (s) onChange(clamp(s.value + e.clientY - s.y));
  };
  const onPointerUp = () => {
    start.current = null;
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 96 : 24;
    if (e.key === "ArrowUp") onChange(clamp(value - step));
    else if (e.key === "ArrowDown") onChange(clamp(value + step));
    else return;
    e.preventDefault();
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a separator with a value is the ARIA pattern for a split
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label="Arrangement height"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onKeyDown={onKeyDown}
      className="group grid h-3 cursor-row-resize touch-none place-items-center"
    >
      <span
        aria-hidden="true"
        className="block h-1 w-16 rounded-control bg-frame-hi opacity-60 group-hover:opacity-100 group-focus-visible:opacity-100"
      />
    </div>
  );
}
