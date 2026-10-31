import {
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type ClipInfo,
  clipMove,
  clipRemove,
  clipSplit,
  clipTrim,
  type EngineStatus,
  type Tempo,
  type TrackStatus,
  tempoSet,
  trackAdd,
  trackArm,
  transportLocate,
} from "../lib/engine";
import { loadPeaks, peaksFor, usePeaksVersion } from "../lib/peakStore";
import { columns } from "../lib/peaks";
import {
  canSplit,
  fitView,
  follow,
  type Geometry,
  gridFor,
  hitTest,
  moved,
  musicalLabel,
  RULER_HEIGHT,
  sampleAt,
  snap,
  TRACK_HEIGHT,
  trimmedLeft,
  trimmedRight,
  type View,
  xOf,
  zoomAround,
} from "../lib/timeline";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Led } from "./Led";

type Props = {
  tracks: TrackStatus[];
  sampleRate: number;
  tempo: Tempo;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  running: boolean;
  selectedTrack: number | null;
  onSelectTrack: (id: number) => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** A drag in progress: which edge, and the clip's geometry so far. */
interface Drag {
  kind: "clip" | "left" | "right";
  clip: ClipInfo;
  fromTrack: number;
  toTrack: number;
  startX: number;
  geometry: Geometry;
  moved: boolean;
}

/** The tokens the canvas draws with, read from the skin on each draw. */
const TOKENS = [
  "bg",
  "lane",
  "grid",
  "bar",
  "ruler-fg",
  "clip",
  "clip-edge",
  "clip-selected",
  "wave",
  "playhead",
] as const;
type Tokens = Record<(typeof TOKENS)[number], string>;

function readTokens(el: HTMLElement): Tokens {
  const style = getComputedStyle(el);
  const out = {} as Tokens;
  for (const t of TOKENS) out[t] = style.getPropertyValue(`--tl-${t}`).trim();
  out.wave ||= style.getPropertyValue("--lcd-fg").trim();
  return out;
}

const LABEL_WIDTH = 160;

/**
 * Tracks and clips on a canvas. Drag a clip to move it (across tracks
 * too), drag either edge to trim, Alt-click to split at the pointer, and
 * click the ruler to move the playhead. The keyboard does the same on
 * the selected clip: Delete, S to split at the playhead, the arrows to
 * nudge by a grid step. Waveforms come from peak caches built in a
 * worker; the canvas only ever reads one level of them per column.
 */
export function Timeline({
  tracks,
  sampleRate,
  tempo,
  position,
  playing,
  running,
  selectedTrack,
  onSelectTrack,
  onStatus,
  onError,
}: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [view, setView] = useState<View>(() => fitView(sampleRate * 30, 800));
  const [snapOn, setSnapOn] = useState(true);
  const [selectedClip, setSelectedClip] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const [bpm, setBpm] = useState(String(tempo.bpm));
  const peaksVersion = usePeaksVersion();

  const run = useCallback(
    (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e))),
    [onStatus, onError],
  );

  const grid = useMemo(() => gridFor(tempo, sampleRate, view.spp), [tempo, sampleRate, view.spp]);
  const height = RULER_HEIGHT + Math.max(1, tracks.length) * TRACK_HEIGHT;
  const playhead = position * sampleRate;
  const clipsByTrack = useMemo(() => tracks.map((t) => t.clips), [tracks]);
  const selected = useMemo(() => {
    for (const t of tracks) {
      const c = t.clips.find((c) => c.id === selectedClip);
      if (c) return { track: t, clip: c };
    }
    return null;
  }, [tracks, selectedClip]);

  useEffect(() => {
    setBpm(String(tempo.bpm));
  }, [tempo.bpm]);

  // Every clip's file gets a peak cache, built once.
  useEffect(() => {
    for (const t of tracks) for (const c of t.clips) loadPeaks(c.file);
  }, [tracks]);

  // The canvas fills its column.
  useLayoutEffect(() => {
    const el = container.current;
    if (!el) return;
    const measure = () => setWidth(Math.max(100, Math.floor(el.clientWidth)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Keep the playhead on screen while rolling, a page at a time.
  useEffect(() => {
    if (!playing) return;
    setView((v) => {
      const scroll = follow(v, playhead, width);
      return scroll === v.scroll ? v : { ...v, scroll };
    });
  }, [playing, playhead, width]);

  // Zoom with a pinch or Ctrl-wheel, scroll sideways with a swipe. A
  // native listener, since React's wheel events cannot prevent the page
  // from scrolling.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const x = e.clientX - el.getBoundingClientRect().left;
        setView((v) => zoomAround(v, Math.exp(e.deltaY * 0.01), x));
      } else if (e.deltaX !== 0 || e.shiftKey) {
        e.preventDefault();
        const delta = (e.deltaX || e.deltaY) * 1;
        setView((v) => ({ ...v, scroll: Math.max(0, v.scroll + delta * v.spp) }));
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  // ----- drawing -----

  // biome-ignore lint/correctness/useExhaustiveDependencies: peaksVersion redraws when a cache lands
  useLayoutEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    if (el.width !== width * dpr || el.height !== height * dpr) {
      el.width = width * dpr;
      el.height = height * dpr;
    }
    const ctx = el.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const tk = readTokens(el);
    const font = getComputedStyle(el).getPropertyValue("--font-ui").trim() || "sans-serif";

    ctx.fillStyle = tk.bg;
    ctx.fillRect(0, 0, width, height);

    // Lanes.
    tracks.forEach((t, i) => {
      const y = RULER_HEIGHT + i * TRACK_HEIGHT;
      ctx.fillStyle = tk.lane;
      if (i % 2 === 1 || t.id === selectedTrack) ctx.fillRect(0, y, width, TRACK_HEIGHT);
      ctx.fillStyle = tk.grid;
      ctx.fillRect(0, y + TRACK_HEIGHT - 1, width, 1);
    });

    // Grid lines and the ruler.
    const first = Math.floor(view.scroll / grid.division) * grid.division;
    const last = view.scroll + width * view.spp;
    const barLabelEvery = Math.max(1, Math.ceil(48 / (grid.bar / view.spp)));
    ctx.font = `11px ${font}`;
    ctx.textBaseline = "middle";
    for (let s = first; s <= last; s += grid.division) {
      const x = Math.round(xOf(s, view)) + 0.5;
      const onBar = Math.abs(s / grid.bar - Math.round(s / grid.bar)) < 1e-6;
      ctx.strokeStyle = onBar ? tk.bar : tk.grid;
      ctx.beginPath();
      ctx.moveTo(x, onBar ? RULER_HEIGHT / 2 : RULER_HEIGHT - 6);
      ctx.lineTo(x, height);
      ctx.stroke();
      if (onBar) {
        const bar = Math.round(s / grid.bar);
        if (bar % barLabelEvery === 0) {
          ctx.fillStyle = tk["ruler-fg"];
          ctx.fillText(String(bar + 1), x + 4, RULER_HEIGHT / 2 - 2);
        }
      }
    }
    ctx.strokeStyle = tk.bar;
    ctx.beginPath();
    ctx.moveTo(0, RULER_HEIGHT - 0.5);
    ctx.lineTo(width, RULER_HEIGHT - 0.5);
    ctx.stroke();

    // Clips, with the one being dragged drawn where it is going.
    const drawClip = (clip: ClipInfo, geometry: Geometry, row: number, isSelected: boolean) => {
      const x0 = xOf(geometry.start, view);
      const x1 = xOf(geometry.start + geometry.length, view);
      if (x1 < 0 || x0 > width) return;
      const left = Math.max(0, Math.floor(x0));
      const right = Math.min(width, Math.ceil(x1));
      const y = RULER_HEIGHT + row * TRACK_HEIGHT + 4;
      const h = TRACK_HEIGHT - 8;
      ctx.fillStyle = isSelected ? tk["clip-selected"] : tk.clip;
      ctx.fillRect(left, y, right - left, h);
      // Waveform: one column per pixel from the cache's level for this zoom.
      const cache = peaksFor(clip.file);
      if (cache && right > left) {
        const fromFrame = geometry.offset + (left - x0) * view.spp;
        const toFrame = fromFrame + (right - left) * view.spp;
        const cols = columns(cache, fromFrame, toFrame, right - left);
        const mid = y + h / 2;
        const amp = (h / 2 - 2) * 0.95;
        ctx.fillStyle = tk.wave;
        for (let c = 0; c < right - left; c++) {
          const lo = cols[c * 2] ?? 0;
          const hi = cols[c * 2 + 1] ?? 0;
          const top = mid - hi * amp;
          const bottom = mid - lo * amp;
          ctx.fillRect(left + c, top, 1, Math.max(1, bottom - top));
        }
      }
      ctx.strokeStyle = tk["clip-edge"];
      ctx.lineWidth = isSelected ? 2 : 1;
      ctx.strokeRect(left + 0.5, y + 0.5, right - left - 1, h - 1);
      ctx.lineWidth = 1;
      // Name, clipped to the clip.
      ctx.save();
      ctx.beginPath();
      ctx.rect(left, y, right - left, h);
      ctx.clip();
      ctx.fillStyle = tk["clip-edge"];
      ctx.font = `bold 10px ${font}`;
      ctx.fillText(clip.name.toUpperCase(), Math.max(left, x0) + 4, y + 9);
      ctx.restore();
    };
    tracks.forEach((t, i) => {
      for (const clip of t.clips) {
        if (drag && drag.clip.id === clip.id) continue;
        drawClip(clip, clip, i, clip.id === selectedClip);
      }
    });
    if (drag) drawClip(drag.clip, drag.geometry, drag.toTrack, true);

    // Playhead.
    const px = Math.round(xOf(playhead, view)) + 0.5;
    if (px >= 0 && px <= width) {
      ctx.strokeStyle = tk.playhead;
      ctx.beginPath();
      ctx.moveTo(px, 0);
      ctx.lineTo(px, height);
      ctx.stroke();
      ctx.fillStyle = tk.playhead;
      ctx.beginPath();
      ctx.moveTo(px - 5, 0);
      ctx.lineTo(px + 5, 0);
      ctx.lineTo(px, 7);
      ctx.fill();
    }
  }, [
    tracks,
    view,
    grid,
    width,
    height,
    playhead,
    selectedClip,
    selectedTrack,
    drag,
    peaksVersion,
  ]);

  // ----- the pointer -----

  const local = (e: PointerEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    const { x, y } = local(e);
    const hit = hitTest(clipsByTrack, x, y, view);
    if (!hit) return;
    e.currentTarget.focus();
    if (hit.kind === "ruler") {
      run(transportLocate(snap(sampleAt(x, view), grid.division, snapOn)));
      return;
    }
    const track = tracks[hit.track];
    if (track) onSelectTrack(track.id);
    if (hit.kind === "empty") {
      setSelectedClip(null);
      return;
    }
    setSelectedClip(hit.clip.id);
    if (e.altKey && hit.kind === "clip") {
      const at = Math.round(sampleAt(x, view));
      if (canSplit(hit.clip, at)) run(clipSplit(hit.clip.id, at));
      return;
    }
    const d: Drag = {
      kind: hit.kind,
      clip: hit.clip,
      fromTrack: hit.track,
      toTrack: hit.track,
      startX: x,
      geometry: { start: hit.clip.start, offset: hit.clip.offset, length: hit.clip.length },
      moved: false,
    };
    dragRef.current = d;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const { x, y } = local(e);
    if (!d.moved && Math.abs(x - d.startX) < 3) return;
    let geometry: Geometry;
    let toTrack = d.fromTrack;
    if (d.kind === "clip") {
      geometry = moved(d.clip, (x - d.startX) * view.spp, grid.division, snapOn);
      toTrack = Math.min(
        tracks.length - 1,
        Math.max(0, Math.floor((y - RULER_HEIGHT) / TRACK_HEIGHT)),
      );
    } else if (d.kind === "left") {
      geometry = trimmedLeft(d.clip, sampleAt(x, view), grid.division, snapOn);
    } else {
      const frames = peaksFor(d.clip.file)?.frames;
      geometry = trimmedRight(d.clip, sampleAt(x, view), grid.division, snapOn, frames);
    }
    const next = { ...d, geometry, toTrack, moved: true };
    dragRef.current = next;
    setDrag(next);
  };

  const onPointerUp = (e: PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    dragRef.current = null;
    setDrag(null);
    if (!d || !d.moved) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    const g = d.geometry;
    const dest = tracks[d.toTrack];
    if (d.kind === "clip") {
      if (dest && (g.start !== d.clip.start || d.toTrack !== d.fromTrack)) {
        run(clipMove(d.clip.id, dest.id, g.start));
      }
    } else if (g.start !== d.clip.start || g.length !== d.clip.length) {
      run(clipTrim(d.clip.id, g.start, g.offset, g.length));
    }
  };

  // ----- the keyboard -----

  const onKeyDown = (e: KeyboardEvent<HTMLCanvasElement>) => {
    if (e.metaKey || e.ctrlKey) return;
    switch (e.key) {
      case "Delete":
      case "Backspace":
        if (selected) run(clipRemove(selected.clip.id));
        break;
      case "s":
      case "S":
        if (selected && canSplit(selected.clip, Math.round(playhead))) {
          run(clipSplit(selected.clip.id, Math.round(playhead)));
        }
        break;
      case "ArrowLeft":
      case "ArrowRight": {
        const step = (e.key === "ArrowLeft" ? -1 : 1) * grid.division;
        if (selected) {
          const g = moved(selected.clip, step, grid.division, snapOn);
          run(clipMove(selected.clip.id, selected.track.id, g.start));
        } else {
          setView((v) => ({ ...v, scroll: Math.max(0, v.scroll + step * 4) }));
        }
        break;
      }
      case "=":
      case "+":
        setView((v) => zoomAround(v, 0.5, width / 2));
        break;
      case "-":
        setView((v) => zoomAround(v, 2, width / 2));
        break;
      case "Escape":
        setSelectedClip(null);
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  const commitBpm = () => {
    const value = Number(bpm);
    if (Number.isFinite(value) && value > 0 && value !== tempo.bpm) {
      run(tempoSet({ ...tempo, bpm: value }));
    } else {
      setBpm(String(tempo.bpm));
    }
  };

  const divisionLabel =
    grid.division < grid.beat
      ? `1/${Math.round((grid.beat / grid.division) * (4 / 1))}`
      : grid.division === grid.beat
        ? "beat"
        : `${Math.round(grid.division / grid.bar)} bar${grid.division > grid.bar ? "s" : ""}`;

  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Button aria-label="Zoom out" onClick={() => setView((v) => zoomAround(v, 2, width / 2))}>
          −
        </Button>
        <Button aria-label="Zoom in" onClick={() => setView((v) => zoomAround(v, 0.5, width / 2))}>
          +
        </Button>
        <Button
          onClick={() => {
            const end = Math.max(
              sampleRate * 8,
              ...tracks.flatMap((t) => t.clips.map((c) => c.start + c.length)),
            );
            setView(fitView(end, width));
          }}
        >
          Fit
        </Button>
        <Button aria-pressed={snapOn} onClick={() => setSnapOn((s) => !s)}>
          Snap
        </Button>
        <Lcd aria-label="Playhead in bars" className="min-w-24 text-center text-base tabular-nums">
          {musicalLabel(playhead, grid)}
        </Lcd>
        <span className="flex items-center gap-1 text-sm">
          <label htmlFor="tempo-bpm" className="font-bold text-[11px] uppercase tracking-[0.08em]">
            Tempo
          </label>
          <input
            id="tempo-bpm"
            type="number"
            min={20}
            max={300}
            step={1}
            value={bpm}
            onChange={(e) => setBpm(e.target.value)}
            onBlur={commitBpm}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitBpm();
            }}
            className="surface-lcd min-h-11 w-20 px-2 text-center font-lcd text-lg tabular-nums"
          />
          <span className="text-ink-dim">
            bpm · {tempo.numerator}/{tempo.denominator} · grid {divisionLabel}
          </span>
        </span>
      </div>
      <div
        className="surface-lcd grid overflow-hidden"
        style={{ gridTemplateColumns: `${LABEL_WIDTH}px 1fr` }}
      >
        <div className="grid content-start" style={{ paddingTop: RULER_HEIGHT }}>
          {tracks.map((t) => (
            <div
              key={t.id}
              style={{ height: TRACK_HEIGHT }}
              className={`flex items-center gap-1 border-tl-grid border-b px-1 ${t.id === selectedTrack ? "bg-tl-lane" : ""}`}
            >
              <button
                type="button"
                aria-pressed={t.id === selectedTrack}
                onClick={() => onSelectTrack(t.id)}
                className="min-h-11 min-w-0 flex-1 truncate border-0 bg-transparent px-1 text-left font-bold text-[11px] text-lcd-fg uppercase tracking-[0.08em]"
              >
                {t.name}
              </button>
              <button
                type="button"
                aria-pressed={t.armed}
                aria-label={`Arm ${t.name}`}
                onClick={() => run(trackArm(t.id, !t.armed))}
                className="grid size-11 shrink-0 place-items-center border-0 bg-transparent"
              >
                <Led on={t.recording || t.armed} tone="record" label="Armed" />
              </button>
            </div>
          ))}
          <div className="p-1">
            <Button onClick={() => run(trackAdd())} className="w-full">
              Add track
            </Button>
          </div>
        </div>
        <div ref={container} className="min-w-0">
          <canvas
            ref={canvas}
            tabIndex={0}
            aria-label="Timeline. Drag clips to move them, drag an edge to trim, Alt-click to split, click the ruler to move the playhead. With a clip selected: Delete removes it, S splits it at the playhead, the arrow keys nudge it."
            aria-disabled={!running}
            style={{ width, height, touchAction: "none" }}
            className={`block ${drag ? (drag.kind === "clip" ? "cursor-grabbing" : "cursor-ew-resize") : ""}`}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onKeyDown={onKeyDown}
          />
        </div>
      </div>
    </div>
  );
}
