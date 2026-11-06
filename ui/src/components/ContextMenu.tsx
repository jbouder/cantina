import { type KeyboardEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** One line of a context menu. */
export interface MenuItem {
  label: string;
  /** The key that does the same, shown at the right. */
  hint?: string;
  disabled?: boolean;
  /** A rule above this item, to set a group apart. */
  separated?: boolean;
  onSelect: () => void;
}

type Props = {
  /** What the menu acts on, for assistive technology. */
  label: string;
  /** Where to open, in viewport pixels. */
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
};

/** Keeps a box of `size` inside `limit`, opening away from the edge. */
function fit(at: number, size: number, limit: number): number {
  return Math.max(0, at + size > limit ? at - size : at);
}

/** The items that can take focus, in order. */
function enabled(panel: HTMLElement | null): HTMLElement[] {
  return Array.from(panel?.querySelectorAll<HTMLElement>("[role=menuitem]:enabled") ?? []);
}

/**
 * A menu at the pointer. It goes on `document.body`, since the window's
 * filter would otherwise be what `fixed` positions against. The arrow
 * keys, Home and End move through it; Escape, Tab or a press outside
 * close it, and focus goes back where it was.
 */
export function ContextMenu({ label, x, y, items, onClose }: Props) {
  const panel = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState({ left: x, top: y });

  useLayoutEffect(() => {
    const r = panel.current?.getBoundingClientRect();
    if (!r) return;
    setPlace({
      left: fit(x, r.width, window.innerWidth),
      top: fit(y, r.height, window.innerHeight),
    });
  }, [x, y]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    enabled(panel.current)[0]?.focus();
    const onPress = (e: PointerEvent) => {
      if (!panel.current?.contains(e.target as Node)) onClose();
    };
    // In capture, so a press that starts a drag elsewhere closes it too.
    window.addEventListener("pointerdown", onPress, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPress, true);
      window.removeEventListener("blur", onClose);
      opener?.focus();
    };
  }, [onClose]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const list = enabled(panel.current);
    const at = list.indexOf(document.activeElement as HTMLElement);
    let next: HTMLElement | undefined;
    switch (e.key) {
      case "ArrowDown":
        next = list[(at + 1) % list.length];
        break;
      case "ArrowUp":
        next = list[(at - 1 + list.length) % list.length];
        break;
      case "Home":
        next = list[0];
        break;
      case "End":
        next = list[list.length - 1];
        break;
      case "Escape":
      case "Tab":
        onClose();
        break;
      default:
        return;
    }
    e.preventDefault();
    next?.focus();
  };

  return createPortal(
    <div
      ref={panel}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={place}
      className="surface-panel fixed z-20 grid min-w-52 bg-sheet-bg py-1 text-sheet-ink text-sm"
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          className={`flex items-center justify-between gap-6 border-0 bg-transparent px-3 py-1 text-left text-inherit enabled:hover:bg-sheet-ink enabled:hover:text-sheet-bg enabled:focus:bg-sheet-ink enabled:focus:text-sheet-bg disabled:opacity-50 ${item.separated ? "border-frame-mid border-t border-solid" : ""}`}
          onClick={() => {
            onClose();
            item.onSelect();
          }}
        >
          {item.label}
          {item.hint ? <kbd className="font-ui opacity-70">{item.hint}</kbd> : null}
        </button>
      ))}
    </div>,
    document.body,
  );
}
