import {
  type KeyboardEvent,
  type MouseEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { type ClipInfo, clipTab, type Tab, transportLocate } from "../lib/engine";
import {
  barTicks,
  bendLabel,
  cursorAt,
  layoutTab,
  playedTicks,
  sampleAt,
  stringNames,
  type TabLayout,
  tickAt,
  tickAtX,
} from "../lib/tabLayout";
import { Button } from "./Button";

type Props = {
  /** The track's clips that have a tab. */
  clips: ClipInfo[];
  sampleRate: number;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  onError: (message: string) => void;
};

/** Pixels between string lines. */
const LINE = 13;
/** Room above the staff for bends, palm mutes and bar numbers. */
const ABOVE = 28;
/** Room below the staff for the rhythm. */
const BELOW = 30;
/** Room left of each row for the string names. */
const GUTTER = 26;
/** A bar's width. */
const BAR = 240;

/** Room a fret number takes on its string. */
function labelWidth(label: string): number {
  return label.length * 7 + 4;
}

function rowHeight(strings: number): number {
  return ABOVE + (strings - 1) * LINE + BELOW;
}

/**
 * The tab of a clip, below the timeline: strings and fret numbers in
 * bars, technique marks, the rhythm under the staff and a cursor on the
 * playhead. **Follow** is one line that scrolls with playback; **Page**
 * wraps the bars into rows for reading along. Click a beat to move the
 * playhead there.
 */
export function TabView({ clips, sampleRate, position, playing, onError }: Props) {
  const [clipId, setClipId] = useState(clips[0]?.id ?? 0);
  const clip = clips.find((c) => c.id === clipId) ?? clips[0];
  const [tab, setTab] = useState<Tab | null>(null);
  // `?tabpage` opens on the page view in the browser, for screenshots.
  const [page, setPage] = useState(() =>
    new URLSearchParams(window.location.search).has("tabpage"),
  );
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(960);

  // The tab, again whenever the clip changes (a re-quantise, a trim).
  useEffect(() => {
    if (!clip) return;
    let live = true;
    clipTab(clip.id).then(
      (t) => live && setTab(t),
      (e) => live && onError(String(e)),
    );
    return () => {
      live = false;
    };
  }, [clip, onError]);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);

  const played = clip && tab ? playedTicks(clip, tab, sampleRate) : ([0, 0] as [number, number]);
  const layout: TabLayout | null = useMemo(() => {
    if (!tab) return null;
    const perRow = page
      ? Math.max(1, Math.floor((width - GUTTER) / BAR))
      : Number.POSITIVE_INFINITY;
    return layoutTab(tab, perRow, BAR, played[1]);
  }, [tab, page, width, played[1]]);

  const tick = clip && tab ? tickAt(clip, tab, sampleRate, position) : 0;
  const cursor = layout && tab ? cursorAt(layout, tab, tick) : null;
  const height = layout ? rowHeight(layout.strings) : 0;

  // Keep the cursor in view while playing: a third of the way in when
  // following, its row at the top on a page.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !cursor || !playing) return;
    if (page) {
      const top = cursor.row * height;
      if (top < el.scrollTop || top + height > el.scrollTop + el.clientHeight) el.scrollTop = top;
    } else {
      const x = GUTTER + cursor.x;
      if (x < el.scrollLeft + 40 || x > el.scrollLeft + el.clientWidth - 40) {
        el.scrollLeft = Math.max(0, x - el.clientWidth / 3);
      }
    }
  }, [cursor, playing, page, height]);

  if (!clip) return null;

  const seek = (e: MouseEvent<HTMLDivElement>) => {
    if (!layout || !tab) return;
    const box = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - box.left - GUTTER;
    const row = Math.max(0, Math.min(layout.rows - 1, Math.floor((e.clientY - box.top) / height)));
    locate(tickAtX(layout, tab, row, x));
  };

  const locate = (at: number) =>
    transportLocate(Math.max(0, sampleAt(clip, tab as Tab, sampleRate, at))).catch((err) =>
      onError(String(err)),
    );

  // The keys move the playhead a beat, a bar, or to the start.
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!tab) return;
    const beat = (960 * 4) / tab.tempo.denominator;
    const bar = barTicks(tab.tempo);
    const here = Math.round(tick / beat) * beat;
    const moves: Record<string, number> = {
      ArrowLeft: here - beat,
      ArrowRight: here + beat,
      PageUp: here - bar,
      PageDown: here + bar,
      Home: 0,
    };
    if (!(e.key in moves)) return;
    e.preventDefault();
    locate(Math.max(0, moves[e.key] ?? here));
  };
  const bars = tab ? barTicks(tab.tempo) : 1;
  const beatTicks = tab ? (960 * 4) / tab.tempo.denominator : 960;
  const where = Math.max(0, tick);
  const valueText = `bar ${Math.floor(where / bars) + 1}, beat ${Math.floor((where % bars) / beatTicks) + 1}`;

  return (
    <div className="grid grid-cols-[auto_1fr] items-start gap-2">
      <div className="grid w-28 gap-1">
        {clips.length > 1 ? (
          <span className="relative">
            <select
              aria-label="Clip"
              value={clip.id}
              onChange={(e) => setClipId(Number(e.target.value))}
              className="surface-lcd min-h-8 w-full appearance-none truncate py-0 pr-6 pl-2 font-lcd text-sm"
            >
              {clips.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </span>
        ) : (
          <span className="truncate text-sm" title={clip.name}>
            {clip.name}
          </span>
        )}
        {tab ? (
          <span className="text-ink-dim text-xs">
            {stringNames(tab).reverse().join(" ")}
            {tab.capo ? ` · capo ${tab.capo}` : ""}
          </span>
        ) : null}
        <div role="radiogroup" aria-label="Tab view" className="grid gap-1">
          <Button
            role="radio"
            aria-checked={!page}
            variant={page ? "panel" : "transport"}
            onClick={() => setPage(false)}
          >
            Follow
          </Button>
          <Button
            role="radio"
            aria-checked={page}
            variant={page ? "transport" : "panel"}
            onClick={() => setPage(true)}
          >
            Page
          </Button>
        </div>
      </div>
      <div
        ref={scroller}
        className={`surface-lcd overflow-auto p-0 ${page ? "max-h-[28rem]" : ""}`}
      >
        {tab && layout ? (
          <div
            role="slider"
            tabIndex={0}
            aria-label={`Tab of ${clip.name}: ${tab.notes.length} notes in ${layout.bars.length} bars`}
            aria-valuemin={1}
            aria-valuemax={layout.bars.length}
            aria-valuenow={Math.min(layout.bars.length, Math.floor(where / bars) + 1)}
            aria-valuetext={valueText}
            onKeyDown={key}
            onClick={seek}
            className="inline-block cursor-text"
          >
            <svg
              aria-hidden="true"
              width={GUTTER + layout.width + 8}
              height={layout.rows * height}
              className="block font-lcd"
            >
              {Array.from({ length: layout.rows }, (_, row) => (
                <Row
                  // biome-ignore lint/suspicious/noArrayIndexKey: rows are positions
                  key={row}
                  row={row}
                  tab={tab}
                  layout={layout}
                  played={played}
                  cursorX={cursor && cursor.row === row ? cursor.x : null}
                />
              ))}
            </svg>
          </div>
        ) : (
          <p className="m-0 p-3 text-sm">Reading the tab…</p>
        )}
      </div>
    </div>
  );
}

function Row({
  row,
  tab,
  layout,
  played,
  cursorX,
}: {
  row: number;
  tab: Tab;
  layout: TabLayout;
  played: [number, number];
  cursorX: number | null;
}) {
  const top = row * rowHeight(layout.strings) + ABOVE;
  const bottom = top + (layout.strings - 1) * LINE;
  const bars = layout.bars.filter((b) => b.row === row);
  const last = bars[bars.length - 1];
  const end = last ? last.x + layout.barWidth : 0;
  const inRow = (tickValue: number) => {
    const bar = layout.bars.find((b, i) => {
      const next = layout.bars[i + 1];
      return tickValue >= b.startTick && (!next || tickValue < next.startTick);
    });
    return bar?.row === row;
  };
  const notes = layout.notes.filter((n) => inRow(n.note.start));
  const beats = layout.beats.filter((b) => inRow(b.tick));
  const names = stringNames(tab);
  // Palm mutes and let ring: marked once where a run starts, then dashed.
  const runs = (key: "palmMute" | "letRing") =>
    beats.map((b, i) => {
      const on = tab.notes.some((n) => n.start === b.tick && n[key]);
      const prev = beats[i - 1];
      const before = prev !== undefined && tab.notes.some((n) => n.start === prev.tick && n[key]);
      return { x: b.x, on, first: on && !before };
    });

  return (
    <g transform={`translate(${GUTTER} 0)`}>
      {names.map((name, i) => (
        <text
          // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
          key={i}
          x={-GUTTER + 6}
          y={top + i * LINE + 4}
          className="fill-lcd-fg text-[11px]"
          opacity={0.7}
        >
          {name}
        </text>
      ))}
      {Array.from({ length: layout.strings }, (_, i) => {
        // Each string is drawn in pieces, broken where a fret number sits.
        const gaps = notes
          .filter((n) => n.line === i)
          .map((n) => [n.x - labelWidth(n.label) / 2, n.x + labelWidth(n.label) / 2] as const)
          .sort((a, b) => a[0] - b[0]);
        const pieces: [number, number][] = [];
        let from = 0;
        for (const [a, b] of gaps) {
          if (a > from) pieces.push([from, a]);
          from = Math.max(from, b);
        }
        if (from < end) pieces.push([from, end]);
        return (
          // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
          <g key={i}>
            {pieces.map(([x1, x2]) => (
              <line
                key={x1}
                x1={x1}
                x2={x2}
                y1={top + i * LINE}
                y2={top + i * LINE}
                className="stroke-lcd-fg"
                opacity={0.4}
                strokeWidth={1}
              />
            ))}
          </g>
        );
      })}
      {bars.map((b) => (
        <g key={b.number}>
          <line x1={b.x} x2={b.x} y1={top} y2={bottom} className="stroke-lcd-fg" opacity={0.5} />
          <text x={b.x + 3} y={top - 19} className="fill-lcd-fg text-[10px]" opacity={0.6}>
            {b.number}
          </text>
        </g>
      ))}
      <line x1={end} x2={end} y1={top} y2={bottom} className="stroke-lcd-fg" opacity={0.5} />

      {runs("palmMute").map((r) =>
        r.on ? (
          <text
            key={`pm${r.x}`}
            x={r.x}
            y={top - 12}
            textAnchor="middle"
            className="fill-lcd-fg text-[10px]"
          >
            {r.first ? "P.M." : "-"}
          </text>
        ) : null,
      )}
      {runs("letRing").map((r) =>
        r.on ? (
          <text
            key={`lr${r.x}`}
            x={r.x}
            y={top - 12}
            textAnchor="middle"
            className="fill-lcd-fg text-[10px]"
          >
            {r.first ? "let ring" : "-"}
          </text>
        ) : null,
      )}

      {notes.map((n) => {
        const y = top + n.line * LINE;
        const outside = n.note.start < played[0] || n.note.start >= played[1];
        const above = [
          n.note.bend
            ? `↑${bendLabel(n.note.bend.semitones)}${n.note.bend.release ? "↓" : ""}`
            : "",
          n.note.vibrato ? "~~" : "",
          n.note.tap ? "T" : "",
        ]
          .filter(Boolean)
          .join(" ");
        return (
          <g key={n.index} opacity={outside ? 0.35 : 1}>
            {n.join && n.joinTo !== null ? (
              <text
                x={(n.x + n.joinTo) / 2}
                y={y - 4}
                textAnchor="middle"
                className="fill-lcd-fg text-[10px]"
              >
                {n.join}
              </text>
            ) : null}
            <text x={n.x} y={y + 4} textAnchor="middle" className="fill-lcd-fg text-[12px]">
              {n.label}
            </text>
            {above ? (
              <text x={n.x + 6} y={y - 8} className="fill-lcd-fg text-[9px]">
                {above}
              </text>
            ) : null}
          </g>
        );
      })}

      {beats.map((b) => {
        const y0 = bottom + 6;
        const r = b.rhythm;
        const length = r.stem ? (r.hollow ? 7 : 14) : 0;
        return (
          <g key={b.tick} className="stroke-lcd-fg" strokeWidth={1.5}>
            {length ? <line x1={b.x} x2={b.x} y1={y0} y2={y0 + length} /> : null}
            {Array.from({ length: r.flags }, (_, f) => (
              <line
                // biome-ignore lint/suspicious/noArrayIndexKey: flags are positions
                key={f}
                x1={b.x}
                x2={b.x + 6}
                y1={y0 + length - f * 4}
                y2={y0 + length - f * 4 - 3}
              />
            ))}
            {r.dotted ? (
              <circle cx={b.x + 5} cy={y0 + length} r={1.3} className="fill-lcd-fg" />
            ) : null}
            {r.triplet ? (
              <text
                x={b.x}
                y={y0 + length + 8}
                textAnchor="middle"
                strokeWidth={0}
                className="fill-lcd-fg text-[9px]"
              >
                3
              </text>
            ) : null}
          </g>
        );
      })}

      {cursorX !== null ? (
        <line
          x1={cursorX}
          x2={cursorX}
          y1={top - 24}
          y2={bottom + BELOW - 6}
          className="stroke-tl-playhead"
          strokeWidth={2}
        />
      ) : null}
    </g>
  );
}
