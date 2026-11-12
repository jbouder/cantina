import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Tab } from "../lib/engine";
import { cursorAt, layoutTab } from "../lib/tabLayout";
import { BAR, GUTTER, rowHeight, TabRow } from "./TabView";

type Props = {
  tab: Tab;
  /** Ticks into the tab the cursor is at, or null for none. */
  tick: number | null;
  /** Keep the cursor's row in view. */
  follow: boolean;
  /** What it is, for a screen reader. */
  label: string;
};

/**
 * A tab to read, not edit: the bars wrapped into rows as wide as the
 * room, and a cursor that the view keeps in sight while it plays.
 */
export function TabStaff({ tab, tick, follow, label }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(720);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);

  const layout = useMemo(
    () => layoutTab(tab, Math.max(1, Math.floor((width - GUTTER - 8) / BAR)), BAR),
    [tab, width],
  );
  const height = rowHeight(layout.strings);
  const cursor = tick === null ? null : cursorAt(layout, tab, tick);

  useEffect(() => {
    const el = scroller.current;
    if (!el || !cursor || !follow) return;
    const top = cursor.row * height;
    if (top < el.scrollTop || top + height > el.scrollTop + el.clientHeight) el.scrollTop = top;
  }, [cursor, follow, height]);

  return (
    <div ref={scroller} className="surface-lcd scroll-area max-h-64 min-h-0 w-full p-0">
      <svg
        role="img"
        aria-label={`${label}: ${tab.notes.length} notes in ${layout.bars.length} bars`}
        width={GUTTER + layout.width + 8}
        height={layout.rows * height}
        className="block font-lcd"
      >
        {Array.from({ length: layout.rows }, (_, row) => (
          <TabRow
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are positions
            key={row}
            row={row}
            tab={tab}
            layout={layout}
            played={[0, Number.POSITIVE_INFINITY]}
            cursorX={cursor && cursor.row === row ? cursor.x : null}
            selected={[]}
            caret={null}
          />
        ))}
      </svg>
    </div>
  );
}
