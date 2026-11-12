import {
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type ClipInfo,
  type CopiedClip,
  clipDuplicate,
  clipMove,
  clipPaste,
  clipRemove,
  clipSplit,
  clipTrim,
  type EngineStatus,
  type Level,
  type LoopRegion,
  loopSet,
  type PatternInfo,
  patternClipAdd,
  patternClipDuplicate,
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
  trackMove,
  transportLocate,
} from "../lib/engine";
import { setPatternDropTarget } from "../lib/pattern-drag";
import { loadPeaks, peaksFor, usePeaksVersion } from "../lib/peakStore";
import { columns } from "../lib/peaks";
import { DRAG_THRESHOLD, rowDropIndex } from "../lib/reorder";
import {
  ADD_ROW_HEIGHT,
  canSplit,
  fitView,
  follow,
  type Geometry,
  gridFor,
  gripWidth,
  type Hit,
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
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { MasterHeader, type RowGrip, TrackHeader } from "./TrackHeader";

/**
 * The dashed slot under the last track header, as Add a pedal is at the
 * end of a chain: where the next track goes. It opens a menu of the kinds.
 */
const ADD_SLOT =
  "flex w-full cursor-pointer items-center justify-center gap-1 rounded-control border-2 border-frame-hi border-dashed bg-transparent px-2 font-bold text-ink-dim text-label hover:border-ink hover:text-ink aria-expanded:border-ink aria-expanded:text-ink";

/** A track header on its way to a new place. */
type RowDrag = {
  id: number;
  from: number;
  /** Where the press began, and how far it has gone, in px. */
  y0: number;
  dy: number;
  /** Past the threshold: a drag, not a click. */
  moved: boolean;
  /** The index it would land at. */
  over: number;
};

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
  /** The loop region, drawn on the ruler; Shift-drag there sets it, its edges drag. */
  loopRegion?: LoopRegion | null;
  /** Filled with the clip edits the Edit menu runs on the selected clip. */
  editRef?: RefObject<TimelineEdit | null>;
  /** What the Edit menu can do now; called when that changes. */
  onEditState?: (state: TimelineEditState) => void;
};

/** The Edit menu's view of the timeline: which clip edits apply now. */
export interface TimelineEditState {
  /** A clip is selected: Duplicate, Delete, Cut and Copy apply. */
  selected: boolean;
  /** The playhead is inside the selected clip. */
  canSplit: boolean;
  /** The selected clip's span on the timeline, in samples. */
  span: { start: number; end: number } | null;
}

/** The clip edits the Edit menu runs on the selected clip, and the View menu's zoom. */
export interface TimelineEdit {
  duplicate: () => void;
  remove: () => void;
  splitAtPlayhead: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  fit: () => void;
}

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

/** The open context menu: where, and the timeline sample under the pointer. */
interface Menu {
  x: number;
  y: number;
  at: number;
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
 * too), drag either edge or a selected clip's handles to trim, Alt-click
 * to split at the pointer, right-click for the clip's menu, and click the
 * ruler to move the playhead. The keyboard does the same on the selected
 * clip: Delete, Cmd-D to duplicate, S to split at the playhead, the arrows
 * to nudge by a grid step, and Cut, Copy and Paste from the Edit menu.
 * Shift-drag the ruler to set the loop, and drag its ends to move them. Waveforms come from peak caches built in a
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
  editRef,
  onEditState,
  loopRegion = null,
}: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  // A track header being dragged to a new place: which, from where, how
  // far, and where it would land. The handlers read the ref, which is
  // never a render behind.
  const [rowDrag, setRowDragState] = useState<RowDrag | null>(null);
  const rowDragRef = useRef<RowDrag | null>(null);
  const setRowDrag = useCallback((d: RowDrag | null) => {
    rowDragRef.current = d;
    setRowDragState(d);
  }, []);
  // Escape puts a dragged header back.
  const rowDragging = rowDrag?.moved ?? false;
  useEffect(() => {
    if (!rowDragging) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") setRowDrag(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rowDragging, setRowDrag]);
  // Where the Add track menu opens, under its slot; null when closed.
  const [addMenu, setAddMenu] = useState<{ x: number; y: number } | null>(null);
  // The lanes' scrolling viewport: the canvas is at least as tall, so the
  // grid runs on through the empty room under the last track.
  const lanesView = useRef<HTMLDivElement>(null);
  const [viewHeight, setViewHeight] = useState(0);
  const [view, setView] = useState<View>(() => fitView(sampleRate * 30, 800));
  const [snapOn, setSnapOn] = useState(true);
  const [selectedClip, setSelectedClip] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  // A loop being set or resized on the ruler: the edge that stays put and
  // the one under the pointer, in samples.
  const [loopDraft, setLoopDraft] = useState<{ anchor: number; moving: number } | null>(null);
  const loopDraftRef = useRef<{ anchor: number; moving: number } | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const [hover, setHover] = useState<Hit["kind"] | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  const peaksVersion = usePeaksVersion();

  const run = useCallback(
    (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e))),
    [onStatus, onError],
  );

  const grid = useMemo(() => gridFor(tempo, sampleRate, view.spp), [tempo, sampleRate, view.spp]);
  const height = Math.max(RULER_HEIGHT + Math.max(1, tracks.length) * TRACK_HEIGHT, viewHeight);
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
    const lanes = lanesView.current;
    const measure = () => {
      setWidth(Math.max(100, Math.floor(el.clientWidth)));
      if (lanes) setViewHeight(Math.floor(lanes.clientHeight));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    if (lanes) observer.observe(lanes);
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
    const style = getComputedStyle(el);
    const font = style.getPropertyValue("--font-ui").trim() || "sans-serif";
    // The label step of the type scale: the canvas draws no size of its own.
    const labelSize = style.getPropertyValue("--type-label").trim() || "11px";

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
    ctx.font = `${labelSize} ${font}`;
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
      // The trim handles, on the edges that are in view.
      if (isSelected) {
        const grip = gripWidth(x1 - x0, true);
        ctx.fillStyle = tk["clip-edge"];
        if (x0 >= 0) ctx.fillRect(x0, y, grip, h);
        if (x1 <= width) ctx.fillRect(x1 - grip, y, grip, h);
        ctx.fillStyle = tk["clip-selected"];
        const ridge = Math.floor(h / 3);
        if (x0 >= 0 && grip >= 3) ctx.fillRect(x0 + grip / 2 - 0.5, y + ridge, 1, ridge);
        if (x1 <= width && grip >= 3) ctx.fillRect(x1 - grip / 2 - 0.5, y + ridge, 1, ridge);
      }
      // Name, clipped to the clip.
      ctx.save();
      ctx.beginPath();
      ctx.rect(left, y, right - left, h);
      ctx.clip();
      ctx.fillStyle = tk["clip-edge"];
      ctx.font = `bold ${labelSize} ${font}`;
      ctx.fillText(clip.name, Math.max(left, x0) + 4, y + 9);
      ctx.restore();
    };
    lanes.forEach((clips, i) => {
      for (const clip of clips) {
        if (drag && drag.clip.id === clip.id) continue;
        drawClip(clip, clip, i, clip.id === selectedClip);
      }
    });
    if (drag) drawClip(drag.clip, drag.geometry, drag.toTrack, true);

    // The loop: a band on the ruler, faint while off, and lines down the
    // lanes while on.
    const shownLoop = loopDraft
      ? {
          start: Math.min(loopDraft.anchor, loopDraft.moving),
          end: Math.max(loopDraft.anchor, loopDraft.moving),
          on: loopRegion?.on ?? true,
        }
      : loopRegion;
    if (shownLoop && shownLoop.end > shownLoop.start) {
      const lx0 = xOf(shownLoop.start, view);
      const lx1 = xOf(shownLoop.end, view);
      ctx.save();
      ctx.fillStyle = tk["clip-selected"];
      ctx.globalAlpha = shownLoop.on ? 0.45 : 0.18;
      ctx.fillRect(lx0, 0, lx1 - lx0, RULER_HEIGHT - 1);
      if (shownLoop.on) {
        ctx.globalAlpha = 0.08;
        ctx.fillRect(lx0, RULER_HEIGHT, lx1 - lx0, height - RULER_HEIGHT);
        ctx.globalAlpha = 0.9;
        ctx.fillRect(Math.round(lx0), 0, 1, height);
        ctx.fillRect(Math.round(lx1) - 1, 0, 1, height);
      }
      ctx.restore();
    }

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
    loopRegion,
    loopDraft,
  ]);

  // ----- the pointer -----

  const local = (e: MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: PointerEvent<HTMLCanvasElement>) => {
    // A Ctrl-click is a right-click on a Mac; the menu handles it.
    if (e.button !== 0 || e.ctrlKey) return;
    const { x, y } = local(e);
    const hit = hitTest(lanes, x, y, view, selectedClip);
    if (!hit) return;
    e.currentTarget.focus();
    if (hit.kind === "ruler") {
      const at = snap(sampleAt(x, view), grid.division, snapOn);
      // Grab a loop edge, or Shift-drag a new loop; a plain click locates.
      const edge = loopEdgeAt(x);
      if (edge !== null && loopRegion) {
        const draft = {
          anchor: edge === "start" ? loopRegion.end : loopRegion.start,
          moving: edge === "start" ? loopRegion.start : loopRegion.end,
        };
        loopDraftRef.current = draft;
        setLoopDraft(draft);
        e.currentTarget.setPointerCapture(e.pointerId);
        return;
      }
      if (e.shiftKey) {
        const draft = { anchor: at, moving: at };
        loopDraftRef.current = draft;
        setLoopDraft(draft);
        e.currentTarget.setPointerCapture(e.pointerId);
        return;
      }
      run(transportLocate(at));
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

  /** The loop edge within a few pixels of `x` on the ruler, if any. */
  const loopEdgeAt = (x: number): "start" | "end" | null => {
    if (!loopRegion) return null;
    if (Math.abs(x - xOf(loopRegion.start, view)) <= 5) return "start";
    if (Math.abs(x - xOf(loopRegion.end, view)) <= 5) return "end";
    return null;
  };

  const onPointerMove = (e: PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    const { x, y } = local(e);
    const loop = loopDraftRef.current;
    if (loop) {
      const next = { ...loop, moving: Math.max(0, snap(sampleAt(x, view), grid.division, snapOn)) };
      loopDraftRef.current = next;
      setLoopDraft(next);
      return;
    }
    if (!d) {
      setHover(hitTest(lanes, x, y, view, selectedClip)?.kind ?? null);
      return;
    }
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
    const loop = loopDraftRef.current;
    if (loop) {
      loopDraftRef.current = null;
      setLoopDraft(null);
      const start = Math.round(Math.min(loop.anchor, loop.moving));
      const end = Math.round(Math.max(loop.anchor, loop.moving));
      if (end > start) run(loopSet({ start, end, on: loopRegion?.on ?? true }));
      return;
    }
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
      run(trim(d.clip, g));
    }
  };

  /** The clip's menu, on the clip under the pointer, which it selects. */
  const onContextMenu = (e: MouseEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const { x, y } = local(e);
    const hit = hitTest(lanes, x, y, view, selectedClip);
    if (!hit || hit.kind === "ruler" || hit.kind === "empty") {
      setMenu(null);
      return;
    }
    e.currentTarget.focus();
    const track = tracks[hit.track];
    if (track) onSelectTrack(track.id);
    setSelectedClip(hit.clip.id);
    setMenu({ x: e.clientX, y: e.clientY, at: Math.round(sampleAt(x, view)) });
  };

  const trim = (clip: ClipInfo, g: Geometry) => {
    if (!isPattern(clip)) return clipTrim(clip.id, g.start, g.offset, g.length);
    const t = laneToTicks(g, spt);
    return patternClipTrim(clip.id, t.start, t.offset, t.length);
  };

  const split = (clip: ClipInfo, at: number) =>
    isPattern(clip) ? patternClipSplit(clip.id, Math.round(at / spt)) : clipSplit(clip.id, at);

  const remove = (clip: ClipInfo) =>
    isPattern(clip) ? patternClipRemove(clip.id) : clipRemove(clip.id);

  /** Copies a clip into the next space that holds it and selects the copy, so a second one follows it. */
  const duplicate = (track: TrackStatus, clip: ClipInfo) =>
    (isPattern(clip) ? patternClipDuplicate(clip.id) : clipDuplicate(clip.id)).then(
      (status) => {
        onStatus(status);
        const now = status.project.tracks.find((t) => t.id === track.id);
        const clips = isPattern(clip) ? now?.patternClips : now?.clips;
        setSelectedClip(Math.max(...(clips ?? []).map((c) => c.id)));
      },
      (e) => onError(String(e)),
    );

  // ----- the Edit menu -----

  const here = Math.round(playhead);
  const canSplitHere = selected !== null && canSplit(selected.clip, here);
  const editState = selected !== null;
  const spanStart = selected?.clip.start ?? -1;
  const spanEnd = selected ? selected.clip.start + selected.clip.length : -1;
  useEffect(() => {
    onEditState?.({
      selected: editState,
      canSplit: canSplitHere,
      span: spanStart >= 0 ? { start: spanStart, end: spanEnd } : null,
    });
  }, [onEditState, editState, canSplitHere, spanStart, spanEnd]);

  // Cut, Copy and Paste are the system's own, so text fields keep them.
  // With the timeline focused they act on clips: the web view raises the
  // clipboard events, and canceling `before…` is how WebKit enables the
  // menu item. The copied clip stays in the timeline, not the system
  // clipboard, and pastes onto the selected track at the playhead.
  const clipboard = useRef<CopiedClip | null>(null);
  const selectedRef = useRef(false);
  selectedRef.current = selected !== null;
  const clipEdits = useRef<Record<"copy" | "cut" | "paste", () => boolean>>({
    copy: () => false,
    cut: () => false,
    paste: () => false,
  });
  clipEdits.current = {
    copy: () => {
      if (!selected) return false;
      clipboard.current = copyOf(selected.track, selected.clip);
      return true;
    },
    cut: () => {
      if (!selected) return false;
      clipboard.current = copyOf(selected.track, selected.clip);
      run(remove(selected.clip));
      return true;
    },
    paste: () => {
      const copied = clipboard.current;
      const track = tracks.find((t) => t.id === selectedTrack);
      if (!copied || !track) return false;
      const start = copied.kind === "pattern" ? Math.round(playhead / spt) : here;
      clipPaste(copied, track.id, start).then(
        (status) => {
          onStatus(status);
          const now = status.project.tracks.find((t) => t.id === track.id);
          const clips = copied.kind === "pattern" ? now?.patternClips : now?.clips;
          setSelectedClip(Math.max(...(clips ?? []).map((c) => c.id)));
        },
        (e) => onError(String(e)),
      );
      return true;
    },
  };
  useEffect(() => {
    const focused = () => document.activeElement === canvas.current;
    const enable = (e: Event, ok: () => boolean) => {
      if (focused() && ok()) e.preventDefault();
    };
    const act = (e: Event, edit: () => boolean) => {
      if (focused() && edit()) e.preventDefault();
    };
    const handlers: [string, (e: Event) => void][] = [
      ["beforecopy", (e) => enable(e, () => selectedRef.current)],
      ["beforecut", (e) => enable(e, () => selectedRef.current)],
      ["beforepaste", (e) => enable(e, () => clipboard.current !== null)],
      ["copy", (e) => act(e, () => clipEdits.current.copy())],
      ["cut", (e) => act(e, () => clipEdits.current.cut())],
      ["paste", (e) => act(e, () => clipEdits.current.paste())],
    ];
    for (const [type, handler] of handlers) document.addEventListener(type, handler);
    return () => {
      for (const [type, handler] of handlers) document.removeEventListener(type, handler);
    };
  }, []);
  /** What Copy keeps of a clip. */
  const copyOf = (track: TrackStatus, clip: ClipInfo): CopiedClip => {
    if (!isPattern(clip)) {
      return {
        kind: "audio",
        name: clip.name,
        file: clip.file,
        offset: clip.offset,
        length: clip.length,
      };
    }
    const original = track.patternClips.find((c) => c.id === clip.id);
    return {
      kind: "pattern",
      pattern: original?.pattern ?? 0,
      offset: original?.offset ?? 0,
      length: original?.length ?? 0,
    };
  };

  const menuItems = (target: NonNullable<typeof selected>, at: number): MenuItem[] => {
    const { track, clip } = target;
    const here = Math.round(playhead);
    const inside = canSplit(clip, here);
    return [
      { label: "Split here", disabled: !canSplit(clip, at), onSelect: () => run(split(clip, at)) },
      {
        label: "Split at playhead",
        hint: "S",
        disabled: !inside,
        onSelect: () => run(split(clip, here)),
      },
      {
        label: "Trim start to playhead",
        disabled: !inside,
        separated: true,
        onSelect: () => run(trim(clip, trimmedLeft(clip, here, 0, false))),
      },
      {
        label: "Trim end to playhead",
        disabled: !inside,
        onSelect: () => run(trim(clip, trimmedRight(clip, here, 0, false))),
      },
      {
        label: "Duplicate",
        hint: "⌘D",
        separated: true,
        onSelect: () => duplicate(track, clip),
      },
      { label: "Delete", hint: "⌫", onSelect: () => run(remove(clip)) },
    ];
  };

  // ----- dropping a pattern from the drum grid -----

  // The grid drags with pointer events (see lib/pattern-drag.ts); the
  // canvas is where a pattern lands. The ref keeps the latest view.
  const dropPattern = useRef<(id: number, clientX: number, clientY: number) => void>(() => {});
  dropPattern.current = (id, clientX, clientY) => {
    const el = canvas.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const x = clientX - r.left;
    const y = clientY - r.top;
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
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    return setPatternDropTarget(el, (id, x, y) => dropPattern.current(id, x, y));
  }, []);

  // ----- the keyboard -----

  const onKeyDown = (e: KeyboardEvent<HTMLCanvasElement>) => {
    // ⌘D, like the rest of the Edit menu, comes from the menu bar.
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

  // The View and Edit menus reach the timeline through this.
  if (editRef) {
    editRef.current = {
      duplicate: () => {
        if (selected) duplicate(selected.track, selected.clip);
      },
      remove: () => {
        if (selected) run(remove(selected.clip));
      },
      splitAtPlayhead: () => {
        if (selected && canSplit(selected.clip, here)) run(split(selected.clip, here));
      },
      zoomIn: controls.zoomIn,
      zoomOut: controls.zoomOut,
      fit: controls.fit,
    };
  }

  const pointing = drag?.kind ?? hover;
  const cursor =
    pointing === "left" || pointing === "right"
      ? "cursor-ew-resize"
      : drag
        ? "cursor-grabbing"
        : pointing === "clip"
          ? "cursor-grab"
          : "";

  // A header's name row is its grip: a press that travels becomes a drag,
  // dropped on the nearest row. Its keys are clicks, not grips.
  const rowGrip = (id: number, index: number): RowGrip => ({
    onPointerDown: (e) => {
      if (e.button !== 0 || (e.target as Element).closest("[data-no-grip]")) return;
      setRowDrag({ id, from: index, y0: e.clientY, dy: 0, moved: false, over: index });
    },
    onPointerMove: (e) => {
      const d = rowDragRef.current;
      if (d?.id !== id) return;
      const dy = e.clientY - d.y0;
      if (!d.moved && Math.abs(dy) <= DRAG_THRESHOLD) return;
      if (!d.moved) e.currentTarget.setPointerCapture(e.pointerId);
      const over = rowDropIndex(d.from, dy, TRACK_HEIGHT, tracks.length);
      setRowDrag({ ...d, dy, moved: true, over });
    },
    onPointerUp: () => {
      const d = rowDragRef.current;
      if (d?.id !== id) return;
      setRowDrag(null);
      if (d.moved && d.over !== d.from) run(trackMove(d.id, d.over));
    },
    onPointerCancel: () => setRowDrag(null),
  });

  return (
    <div className="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-2">
      {strip(controls)}
      <div className="surface-lcd grid min-h-0 grid-rows-[minmax(0,1fr)_auto]">
        <div
          ref={lanesView}
          className="grid min-h-0 content-start overflow-x-hidden overflow-y-auto"
          style={{ gridTemplateColumns: `${LABEL_WIDTH}px 1fr` }}
        >
          <div className="grid content-start">
            <div style={{ height: RULER_HEIGHT }} />
            {tracks.map((t, i) => (
              <TrackHeader
                key={t.id}
                track={t}
                index={i}
                count={tracks.length}
                offset={rowDrag?.moved && rowDrag.id === t.id ? rowDrag.dy : null}
                dropTarget={rowDrag?.moved === true && rowDrag.over === i && rowDrag.id !== t.id}
                grip={rowGrip(t.id, i)}
                selected={t.id === selectedTrack}
                silenced={t.strip.mute || (anySolo && !t.strip.solo)}
                level={levels[i] ?? SILENT}
                onSelect={() => onSelectTrack(t.id)}
                onStatus={onStatus}
                onError={onError}
              />
            ))}
            <div className="flex p-1" style={{ height: ADD_ROW_HEIGHT }}>
              <button
                type="button"
                aria-haspopup="menu"
                aria-expanded={addMenu !== null}
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  setAddMenu(addMenu ? null : { x: r.left, y: r.bottom + 2 });
                }}
                title="Add a guitar track or a drum track"
                className={ADD_SLOT}
              >
                <span aria-hidden="true" className="font-lcd">
                  +
                </span>
                Add track
                <svg aria-hidden="true" viewBox="0 0 16 16" className="size-2.5 fill-current">
                  <path d="M3 6l5 5 5-5z" />
                </svg>
              </button>
              {addMenu ? (
                <ContextMenu
                  label="Add a track"
                  x={addMenu.x}
                  y={addMenu.y}
                  onClose={() => setAddMenu(null)}
                  items={[
                    {
                      label: "Guitar track",
                      onSelect: () => run(trackAdd()),
                    },
                    {
                      label: hasKits ? "Drum track" : "Drum track (no kits installed)",
                      disabled: !hasKits,
                      onSelect: () => run(trackAddDrums()),
                    },
                  ]}
                />
              ) : null}
            </div>
          </div>
          <div ref={container} className="min-w-0">
            <canvas
              ref={canvas}
              tabIndex={0}
              aria-label="Timeline. Drag clips to move them, drag an edge to trim, Alt-click to split, right-click a clip for its menu, click the ruler to move the playhead, Shift-drag the ruler to set the loop. With a clip selected: Delete removes it, Command-D duplicates it, Command-X and Command-C cut and copy it, S splits it at the playhead, the arrow keys nudge it. Command-V pastes a copied clip on the selected track at the playhead. Drop a pattern from the drum grid on a drum track to place it."
              aria-disabled={!running}
              style={{ width, height, touchAction: "none" }}
              className={`block ${cursor}`}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
              onKeyDown={onKeyDown}
              onContextMenu={onContextMenu}
            />
            {menu && selected ? (
              <ContextMenu
                label={`${selected.clip.name} clip`}
                x={menu.x}
                y={menu.y}
                items={menuItems(selected, menu.at)}
                onClose={closeMenu}
              />
            ) : null}
          </div>
        </div>
        <MasterHeader master={master} level={masterLevel} onStatus={onStatus} onError={onError} />
      </div>
    </div>
  );
}
