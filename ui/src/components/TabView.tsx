import {
  type KeyboardEvent,
  type MouseEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type ClipInfo,
  clipTab,
  clipTabEdit,
  clipTabRefit,
  clipTabRequantize,
  type EngineStatus,
  loopSet,
  type Practice,
  practiceSet,
  type Tab,
  type TabGrid,
  transportLocate,
} from "../lib/engine";
import {
  applyEdit,
  type Caret,
  caretStep,
  caretString,
  GRID_TICKS,
  moveNotes,
  moveString,
  type NotesEdit,
  removeNotes,
  resizeNotes,
  TECHNIQUE_KEYS,
  type Technique,
  target,
  toggleTechnique,
  typeFret,
} from "../lib/tabEdit";
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
import { TabExportSheet } from "./TabExportSheet";

type Props = {
  /** The track's clips that have a tab. */
  clips: ClipInfo[];
  sampleRate: number;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  practice: Practice;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The technique buttons of the edit bar, in the order tab players read them. */
const TECHNIQUES: { technique: Technique; label: string; key: string; name: string }[] = [
  { technique: "hammerOn", label: "h", key: "H", name: "Hammer-on" },
  { technique: "pullOff", label: "p", key: "P", name: "Pull-off" },
  { technique: "slide", label: "/", key: "S", name: "Slide" },
  { technique: "bend", label: "↑", key: "B", name: "Bend" },
  { technique: "vibrato", label: "~", key: "V", name: "Vibrato" },
  { technique: "palmMute", label: "PM", key: "M", name: "Palm mute" },
  { technique: "letRing", label: "LR", key: "L", name: "Let ring" },
  { technique: "harmonic", label: "<>", key: "N", name: "Harmonic" },
  { technique: "dead", label: "x", key: "X", name: "Dead note" },
  { technique: "tap", label: "T", key: "T", name: "Tap" },
];

/** A second digit within this long makes a two-digit fret. */
const TWO_DIGITS_MS = 900;

/** Pixels between string lines. */
const LINE = 13;
/** Room above the staff for bends, palm mutes and bar numbers. */
const ABOVE = 28;
/** Room below the staff for the rhythm. */
const BELOW = 30;
/** Room left of each row for the string names. */
export const GUTTER = 26;
/** A bar's width. */
export const BAR = 240;

/** Room a fret number takes on its string. */
function labelWidth(label: string): number {
  return label.length * 7 + 4;
}

/** The caret's place on a row, or null when it is on another. */
function caretOn(
  layout: TabLayout,
  tab: Tab,
  caret: Caret,
  row: number,
): { x: number; line: number } | null {
  const at = cursorAt(layout, tab, caret.tick);
  return at.row === row ? { x: at.x, line: caret.string - 1 } : null;
}

export function rowHeight(strings: number): number {
  return ABOVE + (strings - 1) * LINE + BELOW;
}

/**
 * The tab of a clip, below the timeline: strings and fret numbers in
 * bars, technique marks, the rhythm under the staff and a cursor on the
 * playhead. **Follow** is one line that scrolls with playback; **Page**
 * wraps the bars into rows for reading along. Click a beat to move the
 * playhead there.
 */
export function TabView({
  clips,
  sampleRate,
  position,
  playing,
  practice,
  onStatus,
  onError,
}: Props) {
  const [clipId, setClipId] = useState(clips[0]?.id ?? 0);
  const clip = clips.find((c) => c.id === clipId) ?? clips[0];
  const [tab, setTab] = useState<Tab | null>(null);
  // `?tabpage` opens on the page view in the browser, for screenshots.
  const [page, setPage] = useState(() =>
    new URLSearchParams(window.location.search).has("tabpage"),
  );
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(960);
  // Editing: `?tabedit` opens on it in the browser, for screenshots.
  const [editing, setEditing] = useState(() =>
    new URLSearchParams(window.location.search).has("tabedit"),
  );
  // `?tabselect=0,1` starts with notes selected, for screenshots.
  const [selection, setSelection] = useState<number[]>(() =>
    (new URLSearchParams(window.location.search).get("tabselect") ?? "")
      .split(",")
      .filter(Boolean)
      .map(Number),
  );
  const [caret, setCaret] = useState<Caret>({ string: 1, tick: 0 });
  const [grid, setGrid] = useState<TabGrid>("1/16");
  // `?tabexport` opens the export sheet, for screenshots.
  const [exporting, setExporting] = useState(() =>
    new URLSearchParams(window.location.search).has("tabexport"),
  );
  const step = GRID_TICKS[grid];
  const lastDigit = useRef<{ at: number; fret: number; caret: Caret } | null>(null);

  // A selection belongs to one clip's tab.
  const firstClip = useRef(clipId);
  useEffect(() => {
    if (clipId !== firstClip.current) setSelection([]);
    firstClip.current = clipId;
  }, [clipId]);

  // The tab, again whenever the clip changes (a re-quantize, a trim).
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

  const setPractice = (next: Practice) => {
    practiceSet(next).then(onStatus, (e) => onError(String(e)));
  };

  // The loop runs from the first selected note to the end of the last.
  const loopSelection = () => {
    if (!tab || selection.length === 0) return;
    const notes = selection.map((i) => tab.notes[i]).filter((n) => n !== undefined);
    const from = Math.min(...notes.map((n) => n.start));
    const to = Math.max(...notes.map((n) => n.start + n.length));
    const start = Math.max(0, Math.round(sampleAt(clip, tab, sampleRate, from)));
    const end = Math.round(sampleAt(clip, tab, sampleRate, to));
    loopSet({ start, end, on: true }).then(
      (status) => {
        onStatus(status);
        return transportLocate(start);
      },
      (e) => onError(String(e)),
    );
  };

  /** Sends an edit; the notes it added stay selected. */
  const send = (edit: NotesEdit | null) => {
    if (!edit || !tab) return;
    const after = applyEdit(tab.notes, edit);
    clipTabEdit(clip.id, edit.remove, edit.add).then(
      (status) => {
        setTab({ ...tab, notes: after.notes });
        setSelection(selection.length > 0 || edit.add.length > 1 ? after.added : []);
        onStatus(status);
      },
      (e) => onError(String(e)),
    );
  };
  const whole = (action: Promise<EngineStatus>) => {
    action.then(
      (status) => {
        setSelection([]);
        onStatus(status);
      },
      (e) => onError(String(e)),
    );
  };
  const chosen = tab ? target(tab, selection, caret) : [];
  const has = (technique: Technique) =>
    tab !== null &&
    chosen.length > 0 &&
    chosen.every((i) => {
      const n = tab.notes[i];
      return (
        n !== undefined && (technique === "bend" ? n.bend !== undefined : n[technique] === true)
      );
    });

  // Keyboard entry: the caret moves on strings and grid steps, digits
  // type frets, letters toggle techniques, ⌥ moves notes, ⇧ resizes.
  const editKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!tab) return false;
    const notes = target(tab, selection, caret);
    const arrows: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -1,
      ArrowDown: 1,
    };
    const by = arrows[e.key];
    const across = e.key === "ArrowLeft" || e.key === "ArrowRight";
    if (e.metaKey || e.ctrlKey) {
      if (e.key.toLowerCase() !== "a") return false;
      setSelection(tab.notes.map((_, i) => i));
      return true;
    }
    if (by !== undefined && e.altKey) {
      send(across ? moveNotes(tab, notes, by * step) : moveString(tab, notes, by as -1 | 1));
      if (across) setCaret(caretStep(caret, step, by));
      else setCaret(caretString(tab, caret, by));
      return true;
    }
    if (by !== undefined && e.shiftKey && across) {
      send(resizeNotes(tab, notes, by * step, step));
      return true;
    }
    if (by !== undefined) {
      setSelection([]);
      setCaret(across ? caretStep(caret, step, by) : caretString(tab, caret, by));
      return true;
    }
    if (/^[0-9]$/.test(e.key) && !e.altKey) {
      const digit = Number(e.key);
      const now = performance.now();
      const last = lastDigit.current;
      const same =
        last !== null &&
        now - last.at < TWO_DIGITS_MS &&
        last.caret.string === caret.string &&
        last.caret.tick === caret.tick &&
        last.fret * 10 + digit <= 24;
      const fret = same && last ? last.fret * 10 + digit : digit;
      lastDigit.current = { at: now, fret, caret };
      setSelection([]);
      send(typeFret(tab, caret, fret, step));
      return true;
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      send(removeNotes(notes));
      setSelection([]);
      return true;
    }
    if (e.key === "Escape") {
      setSelection([]);
      return true;
    }
    if (e.key === "Home") {
      setCaret({ ...caret, tick: 0 });
      return true;
    }
    const technique = e.altKey || e.shiftKey ? undefined : TECHNIQUE_KEYS[e.key.toLowerCase()];
    if (technique) {
      send(toggleTechnique(tab, notes, technique));
      return true;
    }
    return false;
  };

  // A click in the editor puts the caret on the nearest string and grid
  // step, and selects a note there; ⇧ adds to the selection.
  const pick = (e: MouseEvent<HTMLDivElement>) => {
    if (!layout || !tab) return;
    const box = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - box.left - GUTTER;
    const y = e.clientY - box.top;
    const row = Math.max(0, Math.min(layout.rows - 1, Math.floor(y / height)));
    const line = Math.round((y - row * height - ABOVE) / LINE);
    const string = Math.max(1, Math.min(layout.strings, line + 1));
    const at = tickAtX(layout, tab, row, x);
    const hit = layout.notes.find((n) => {
      if (n.line !== string - 1) return false;
      const c = cursorAt(layout, tab, n.note.start);
      return c.row === row && Math.abs(c.x - x) <= labelWidth(n.label) / 2 + 3;
    });
    if (hit) {
      setCaret({ string, tick: hit.note.start });
      setSelection((was) =>
        e.shiftKey
          ? was.includes(hit.index)
            ? was.filter((i) => i !== hit.index)
            : [...was, hit.index]
          : [hit.index],
      );
    } else {
      setCaret({ string, tick: Math.max(0, Math.floor(at / step) * step) });
      if (!e.shiftKey) setSelection([]);
    }
  };

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
    if (editing) {
      // The editor's keys are its own: R, L, Space and the rest wait.
      if (editKey(e)) {
        e.preventDefault();
        e.stopPropagation();
      }
      return;
    }
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
    e.stopPropagation();
    locate(Math.max(0, moves[e.key] ?? here));
  };
  const bars = tab ? barTicks(tab.tempo) : 1;
  const beatTicks = tab ? (960 * 4) / tab.tempo.denominator : 960;
  const where = Math.max(0, tick);
  const placeOf = (t: number) =>
    `bar ${Math.floor(t / bars) + 1}, beat ${Math.floor((t % bars) / beatTicks) + 1}`;
  const valueText = editing
    ? `caret on string ${caret.string}, ${placeOf(caret.tick)}`
    : placeOf(where);

  return (
    <div className="grid grid-cols-[auto_1fr] items-start gap-2">
      <div className="grid w-28 gap-1">
        {clips.length > 1 ? (
          <span className="relative">
            <select
              aria-label="Clip"
              value={clip.id}
              onChange={(e) => setClipId(Number(e.target.value))}
              className="surface-field min-h-8 w-full appearance-none truncate py-0 pr-6 pl-2"
            >
              {clips.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </span>
        ) : (
          <span className="truncate text-body" title={clip.name}>
            {clip.name}
          </span>
        )}
        {tab ? (
          <span className="text-ink-dim text-label">
            {stringNames(tab).reverse().join(" ")}
            {tab.capo ? ` · capo ${tab.capo}` : ""}
          </span>
        ) : null}
        <div role="radiogroup" aria-label="Tab view" className="grid gap-1">
          <Button role="radio" aria-checked={!page} onClick={() => setPage(false)}>
            Follow
          </Button>
          <Button role="radio" aria-checked={page} onClick={() => setPage(true)}>
            Page
          </Button>
        </div>
        <Button aria-pressed={editing} onClick={() => setEditing(!editing)}>
          Edit
        </Button>
        <Button disabled={!tab} onClick={() => setExporting(true)}>
          Export…
        </Button>
        {exporting && tab ? (
          <TabExportSheet clip={clip} tab={tab} onClose={() => setExporting(false)} />
        ) : null}
      </div>
      <div className="grid min-w-0 gap-2">
        <div role="toolbar" aria-label="Practice" className="flex flex-wrap items-center gap-1">
          <Button
            aria-pressed={practice.voice}
            title="Play the tabs through a plucked-string voice, into the track's pedals"
            onClick={() => setPractice({ ...practice, voice: !practice.voice })}
          >
            Hear tab
          </Button>
          <Button
            aria-pressed={practice.muteRecordings}
            title="Silence the recordings under the tabs, to play the part yourself"
            onClick={() => setPractice({ ...practice, muteRecordings: !practice.muteRecordings })}
          >
            Mute recording
          </Button>
          <label className="caption flex items-center gap-1">
            Speed
            <input
              type="range"
              className="fader w-28"
              aria-label="Practice speed"
              aria-valuetext={`${Math.round(practice.speed * 100)}%`}
              min={50}
              max={100}
              step={5}
              value={Math.round(practice.speed * 100)}
              onChange={(e) => setPractice({ ...practice, speed: Number(e.target.value) / 100 })}
              onDoubleClick={() => setPractice({ ...practice, speed: 1 })}
            />
            <output className="surface-lcd px-2 py-0.5 font-lcd text-body leading-none tabular-nums">
              <span className="lcd-text">{Math.round(practice.speed * 100)}%</span>
            </output>
          </label>
          <Button
            disabled={!tab || selection.length === 0}
            title="Loop playback over the selected notes"
            onClick={loopSelection}
          >
            Loop selection
          </Button>
          {practice.speed < 1 ? (
            <span className="text-ink-dim text-label">Recordings are silent below 100%</span>
          ) : null}
        </div>
        {editing && tab ? (
          <div
            role="toolbar"
            aria-label="Edit the tab"
            className="flex flex-wrap items-center gap-1"
          >
            <label className="flex items-center gap-1 text-label">
              Grid
              <select
                aria-label="Grid"
                value={grid}
                onChange={(e) => setGrid(e.target.value as TabGrid)}
                className="surface-field min-h-8 appearance-none py-0 pr-2 pl-2"
              >
                <option value="1/8">1/8</option>
                <option value="1/16">1/16</option>
                <option value="1/8t">1/8 triplet</option>
                <option value="1/16t">1/16 triplet</option>
              </select>
            </label>
            {TECHNIQUES.map((t) => (
              <Button
                key={t.technique}
                aria-pressed={has(t.technique)}
                aria-label={t.name}
                title={`${t.name} (${t.key})`}
                disabled={chosen.length === 0}
                onClick={() => send(toggleTechnique(tab, chosen, t.technique))}
              >
                {t.label}
              </Button>
            ))}
            <Button
              disabled={chosen.length === 0}
              title="Delete (⌫)"
              onClick={() => {
                send(removeNotes(chosen));
                setSelection([]);
              }}
            >
              Delete
            </Button>
            <Button
              title={selection.length > 0 ? "Fit the selected notes again" : "Fit every note again"}
              onClick={() => whole(clipTabRefit(clip.id, selection))}
            >
              Re-fit
            </Button>
            {clip.tab?.transcribed ? (
              <Button
                title={
                  selection.length > 0
                    ? `Quantize the selected notes to ${grid} again from what was heard`
                    : `Quantize every note to ${grid} again from what was heard`
                }
                onClick={() => whole(clipTabRequantize(clip.id, grid, selection))}
              >
                Re-quantize
              </Button>
            ) : null}
            <span className="text-ink-dim text-label">
              {selection.length > 0 ? `${selection.length} selected` : "Type frets at the caret"}
            </span>
          </div>
        ) : null}
        <div
          ref={scroller}
          className={`surface-lcd overflow-auto p-0 ${page ? "max-h-[28rem]" : ""}`}
        >
          {tab && layout ? (
            <div
              role="slider"
              tabIndex={0}
              aria-label={`Tab of ${clip.name}: ${tab.notes.length} notes in ${layout.bars.length} bars${editing ? ", editing" : ""}`}
              aria-valuemin={1}
              aria-valuemax={layout.bars.length}
              aria-valuenow={Math.min(layout.bars.length, Math.floor(where / bars) + 1)}
              aria-valuetext={valueText}
              onKeyDown={key}
              onClick={editing ? pick : seek}
              className="inline-block cursor-text"
            >
              <svg
                aria-hidden="true"
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
                    played={played}
                    cursorX={cursor && cursor.row === row ? cursor.x : null}
                    selected={selection}
                    caret={editing ? caretOn(layout, tab, caret, row) : null}
                  />
                ))}
              </svg>
            </div>
          ) : (
            <p className="m-0 p-3 text-body">Reading the tab…</p>
          )}
        </div>
      </div>
    </div>
  );
}

/** One row of a tab: strings, fret numbers, marks, rhythm and the cursor. */
export function TabRow({
  row,
  tab,
  layout,
  played,
  cursorX,
  selected,
  caret,
}: {
  row: number;
  tab: Tab;
  layout: TabLayout;
  played: [number, number];
  cursorX: number | null;
  selected: number[];
  /** The caret's x and string line on this row, when editing. */
  caret: { x: number; line: number } | null;
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
          className="fill-lcd-fg text-label"
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
          <text x={b.x + 3} y={top - 19} className="fill-lcd-fg text-label" opacity={0.6}>
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
            className="fill-lcd-fg text-label"
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
            className="fill-lcd-fg text-label"
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
            {selected.includes(n.index) ? (
              <rect
                x={n.x - labelWidth(n.label) / 2}
                y={y - 7}
                width={labelWidth(n.label)}
                height={14}
                rx={2}
                className="fill-select"
              />
            ) : null}
            {n.join && n.joinTo !== null ? (
              <text
                x={(n.x + n.joinTo) / 2}
                y={y - 4}
                textAnchor="middle"
                className="fill-lcd-fg text-label"
              >
                {n.join}
              </text>
            ) : null}
            <text x={n.x} y={y + 4} textAnchor="middle" className="fill-lcd-fg text-body">
              {n.label}
            </text>
            {above ? (
              <text x={n.x + 6} y={y - 8} className="fill-lcd-fg text-label">
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
                className="fill-lcd-fg text-label"
              >
                3
              </text>
            ) : null}
          </g>
        );
      })}

      {caret ? (
        <rect
          x={caret.x - 7}
          y={top + caret.line * LINE - 7}
          width={14}
          height={14}
          rx={2}
          fill="none"
          className="stroke-focus"
          strokeWidth={1.5}
        />
      ) : null}
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
