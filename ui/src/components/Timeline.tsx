import {
  type DragEvent,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
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
  type Level,
  type PatternInfo,
  patternClipAdd,
  patternClipMove,
  patternClipRemove,
  patternClipSplit,
  patternClipTrim,
  SILENT,
  type Strip,
  type Tempo,
  type TrackStatus,
  trackAdd,
  trackAddDrums,
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
  laneToTicks,
  moved,
  musicalLabel,
  patternLane,
  previewHits,
  RULER_HEIGHT,
  sampleAt,
  samplesPerTick,
  snap,
  TRACK_HEIGHT,
  trimmedLeft,
  trimmedRight,
  type View,
  xOf,
  zoomAround,
} from "../lib/timeline";
import { Button } from "./Button";
import { MasterHeader, TrackHeader } from "./TrackHeader";

/** The drag type a pattern chip carries from the drum grid. */
export const PATTERN_DRAG = "application/x-cantina-pattern";

/** What the transport strip above the lanes can do to the view. */
export interface TimelineControls {
  snapOn: boolean;
  toggleSnap: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  /** Zooms to show every clip. */
  fit: () => void;
  /** The playhead as `bar.beat.sixteenth`. */
  bars: string;
  /** What edits snap to, as a label. */
  divisionLabel: string;
}

type Props = {
  tracks: TrackStatus[];
  /** The project's patterns, for drawing pattern clips. */
  patterns: PatternInfo[];
  sampleRate: number;
  tempo: Tempo;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  running: boolean;
  selectedTrack: number | null;
  /** Whether a drum track can be added: at least one kit is installed. */
  hasKits: boolean;
  /** Post-fader levels per track, in track order, and the master. */
  levels: Level[];
  master: Strip;
  masterLevel: Level;
  /** The transport strip, drawn above the lanes with the view's controls. */
  strip: (controls: TimelineControls) => ReactNode;
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

const LABEL_WIDTH = 288;

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
  patterns,
  sampleRate,
  tempo,
  position,
  playing,
  running,
  selectedTrack,
  hasKits,
  levels,
  master,
  masterLevel,
  strip,
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
  const peaksVersion = usePeaksVersion();

  const run = useCallback(
    (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e))),
    [onStatus, onError],
  );

  const grid = useMemo(() => gridFor(tempo, sampleRate, view.spp), [tempo, sampleRate, view.spp]);
  const height = RULER_HEIGHT + Math.max(1, tracks.length) * TRACK_HEIGHT;
  const anySolo = tracks.some((t) => t.strip.solo);
  const playhead = position * sampleRate;
  // What each lane holds: audio clips in samples, or a drum track's
  // pattern clips converted from ticks so the same hit testing and drag
  // arithmetic serve both. A pattern clip has no file.
  const spt = samplesPerTick(tempo, sampleRate);
  const lanes = useMemo(
    () =>
      tracks.map((t) =>
        t.kit
          ? t.patternClips.map((c) =>
              patternLane(c, patterns.find((p) => p.id === c.pattern)?.name ?? "?", spt),
            )
          : t.clips,
      ),
    [tracks, patterns, spt],
  );
  const isPattern = (clip: ClipInfo) => clip.file === "";
  const selected = useMemo(() => {
    for (const [i, t] of tracks.entries()) {
      const c = lanes[i]?.find((c) => c.id === selectedClip);
      if (c) return { track: t, clip: c };
    }
    return null;
  }, [tracks, lanes, selectedClip]);

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
      const track = tracks[row];
      const x0 = xOf(geometry.start, view);
      const x1 = xOf(geometry.start + geometry.length, view);
      if (x1 < 0 || x0 > width) return;
      const left = Math.max(0, Math.floor(x0));
      const right = Math.min(width, Math.ceil(x1));
      const y = RULER_HEIGHT + row * TRACK_HEIGHT + 4;
      const h = TRACK_HEIGHT - 8;
      ctx.fillStyle = isSelected ? tk["clip-selected"] : tk.clip;
      ctx.fillRect(left, y, right - left, h);
      // A pattern clip shows its steps: one band per pad, a block per
      // hit, the pattern looping from the clip's offset.
      const original = track?.patternClips.find((c) => c.id === clip.id);
      const pattern = original && patterns.find((p) => p.id === original.pattern);
      if (original && pattern && track?.kit && right > left) {
        const pads = track.kit.pads.map((p) => p.id);
        const shown = {
          ...original,
          offset: Math.round(geometry.offset / spt),
          length: Math.round(geometry.length / spt),
        };
        const fromRel = (left - x0) * view.spp;
        const toRel = (right - x0) * view.spp;
        const band = (h - 14) / Math.max(1, pads.length);
        const stepPx = Math.max(2, ((960 / pattern.stepsPerBeat) * spt) / view.spp - 1);
        for (const hit of previewHits(shown, pattern, pads, spt, fromRel, toRel)) {
          const hx = x0 + hit.at / view.spp;
          ctx.globalAlpha = 0.35 + hit.velocity * 0.65;
          ctx.fillStyle = tk.wave;
          ctx.fillRect(
            hx,
            y + 12 + hit.row * band,
            Math.min(stepPx, right - hx),
            Math.max(1, band - 1),
          );
        }
        ctx.globalAlpha = 1;
      }
      // Waveform: one column per pixel from the cache's level for this zoom.
      const cache = clip.file ? peaksFor(clip.file) : null;
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
    lanes.forEach((clips, i) => {
      for (const clip of clips) {
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
    lanes,
    patterns,
    spt,
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
    const hit = hitTest(lanes, x, y, view);
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
      if (canSplit(hit.clip, at)) run(split(hit.clip, at));
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
      // A pattern clip can grow without limit: the pattern loops.
      const frames = isPattern(d.clip) ? Number.POSITIVE_INFINITY : peaksFor(d.clip.file)?.frames;
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
    // A clip only lands on a track of its own kind; otherwise it stays
    // on its track at the new position.
    const wanted = tracks[d.toTrack];
    const dest = wanted && Boolean(wanted.kit) === isPattern(d.clip) ? wanted : tracks[d.fromTrack];
    if (d.kind === "clip") {
      if (dest && (g.start !== d.clip.start || dest.id !== tracks[d.fromTrack]?.id)) {
        run(
          isPattern(d.clip)
            ? patternClipMove(d.clip.id, dest.id, Math.round(g.start / spt))
            : clipMove(d.clip.id, dest.id, g.start),
        );
      }
    } else if (g.start !== d.clip.start || g.length !== d.clip.length) {
      if (isPattern(d.clip)) {
        const t = laneToTicks(g, spt);
        run(patternClipTrim(d.clip.id, t.start, t.offset, t.length));
      } else {
        run(clipTrim(d.clip.id, g.start, g.offset, g.length));
      }
    }
  };

  const split = (clip: ClipInfo, at: number) =>
    isPattern(clip) ? patternClipSplit(clip.id, Math.round(at / spt)) : clipSplit(clip.id, at);

  const remove = (clip: ClipInfo) =>
    isPattern(clip) ? patternClipRemove(clip.id) : clipRemove(clip.id);

  // ----- dropping a pattern from the drum grid -----

  const onDragOver = (e: DragEvent<HTMLCanvasElement>) => {
    if (e.dataTransfer.types.includes(PATTERN_DRAG)) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    }
  };

  const onDrop = (e: DragEvent<HTMLCanvasElement>) => {
    const id = Number(e.dataTransfer.getData(PATTERN_DRAG));
    if (!id) return;
    e.preventDefault();
    const r = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    const row = Math.floor((y - RULER_HEIGHT) / TRACK_HEIGHT);
    const track = tracks[row];
    if (!track) return;
    if (!track.kit) {
      onError(`${track.name} is an audio track; drop the pattern on a drum track`);
      return;
    }
    const start = snap(sampleAt(x, view), grid.division, snapOn);
    run(patternClipAdd(track.id, id, Math.round(start / spt)));
  };

  // ----- the keyboard -----

  const onKeyDown = (e: KeyboardEvent<HTMLCanvasElement>) => {
    if (e.metaKey || e.ctrlKey) return;
    switch (e.key) {
      case "Delete":
      case "Backspace":
        if (selected) run(remove(selected.clip));
        break;
      case "s":
      case "S":
        if (selected && canSplit(selected.clip, Math.round(playhead))) {
          run(split(selected.clip, Math.round(playhead)));
        }
        break;
      case "ArrowLeft":
      case "ArrowRight": {
        const step = (e.key === "ArrowLeft" ? -1 : 1) * grid.division;
        if (selected) {
          const g = moved(selected.clip, step, grid.division, snapOn);
          run(
            isPattern(selected.clip)
              ? patternClipMove(selected.clip.id, selected.track.id, Math.round(g.start / spt))
              : clipMove(selected.clip.id, selected.track.id, g.start),
          );
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

  const divisionLabel =
    grid.division < grid.beat
      ? `1/${Math.round((grid.beat / grid.division) * (4 / 1))}`
      : grid.division === grid.beat
        ? "beat"
        : `${Math.round(grid.division / grid.bar)} bar${grid.division > grid.bar ? "s" : ""}`;

  const controls: TimelineControls = {
    snapOn,
    toggleSnap: () => setSnapOn((s) => !s),
    zoomIn: () => setView((v) => zoomAround(v, 0.5, width / 2)),
    zoomOut: () => setView((v) => zoomAround(v, 2, width / 2)),
    fit: () => {
      const end = Math.max(
        sampleRate * 8,
        ...lanes.flatMap((clips) => clips.map((c) => c.start + c.length)),
      );
      setView(fitView(end, width));
    },
    bars: musicalLabel(playhead, grid),
    divisionLabel,
  };

  return (
    <div className="grid gap-2">
      {strip(controls)}
      <div
        className="surface-lcd grid overflow-hidden"
        style={{ gridTemplateColumns: `${LABEL_WIDTH}px 1fr` }}
      >
        <div className="grid content-start" style={{ paddingTop: RULER_HEIGHT }}>
          {tracks.map((t, i) => (
            <TrackHeader
              key={t.id}
              track={t}
              selected={t.id === selectedTrack}
              silenced={t.strip.mute || (anySolo && !t.strip.solo)}
              level={levels[i] ?? SILENT}
              onSelect={() => onSelectTrack(t.id)}
              onStatus={onStatus}
              onError={onError}
            />
          ))}
          <MasterHeader master={master} level={masterLevel} onStatus={onStatus} onError={onError} />
          <div className="grid grid-cols-2 gap-1 p-1">
            <Button onClick={() => run(trackAdd())} className="min-w-0 px-1">
              Add track
            </Button>
            <Button
              onClick={() => run(trackAddDrums())}
              disabled={!hasKits}
              title={hasKits ? "A track that plays drum patterns" : "No drum kits are installed"}
              className="min-w-0 px-1"
            >
              Add drums
            </Button>
          </div>
        </div>
        <div ref={container} className="min-w-0">
          <canvas
            ref={canvas}
            tabIndex={0}
            aria-label="Timeline. Drag clips to move them, drag an edge to trim, Alt-click to split, click the ruler to move the playhead. With a clip selected: Delete removes it, S splits it at the playhead, the arrow keys nudge it. Drop a pattern from the drum grid on a drum track to place it."
            aria-disabled={!running}
            style={{ width, height, touchAction: "none" }}
            className={`block ${drag ? (drag.kind === "clip" ? "cursor-grabbing" : "cursor-ew-resize") : ""}`}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onKeyDown={onKeyDown}
            onDragOver={onDragOver}
            onDrop={onDrop}
          />
        </div>
      </div>
    </div>
  );
}
