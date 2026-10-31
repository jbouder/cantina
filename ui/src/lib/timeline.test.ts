import { describe, expect, it } from "vitest";
import type { ClipInfo, PatternInfo } from "./engine";
import {
  canSplit,
  fitView,
  follow,
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
} from "./timeline";

const SR = 48_000;
const tempo = { bpm: 120, numerator: 4, denominator: 4 };

function clip(id: number, start: number, length: number, offset = 0): ClipInfo {
  return { id, name: `c${id}`, file: `c${id}.wav`, start, offset, length };
}

describe("view", () => {
  const view: View = { scroll: 1000, spp: 10 };
  it("maps samples to pixels and back", () => {
    expect(xOf(1000, view)).toBe(0);
    expect(xOf(1500, view)).toBe(50);
    expect(sampleAt(50, view)).toBe(1500);
    expect(sampleAt(-500, view)).toBe(0);
  });
  it("zooms around the pointer and clamps", () => {
    const z = zoomAround(view, 2, 50);
    expect(z.spp).toBe(20);
    expect(sampleAt(50, z)).toBe(1500);
    expect(zoomAround(view, 1e-9, 0).spp).toBe(8);
    expect(zoomAround(view, 1e9, 0).spp).toBe(65536);
    expect(zoomAround({ scroll: 0, spp: 10 }, 4, 500).scroll).toBe(0);
  });
  it("fits a length into a width", () => {
    const v = fitView(SR * 10, 1000);
    expect(v.scroll).toBe(0);
    expect(xOf(SR * 10, v)).toBeLessThanOrEqual(1000);
  });
  it("follows the playhead by paging", () => {
    expect(follow(view, 1200, 100)).toBe(1000);
    expect(follow(view, 5000, 100)).toBe(4900);
    expect(follow(view, 0, 100)).toBe(0);
  });
});

describe("grid", () => {
  it("derives beats and bars from the tempo", () => {
    const g = gridFor(tempo, SR, 100);
    expect(g.beat).toBe(24_000);
    expect(g.bar).toBe(96_000);
    expect(g.beatsPerBar).toBe(4);
  });
  it("coarsens the division as the zoom goes out", () => {
    expect(gridFor(tempo, SR, 100).division).toBe(6_000);
    expect(gridFor(tempo, SR, 1000).division).toBe(24_000);
    expect(gridFor(tempo, SR, 10_000).division).toBe(192_000);
    expect(gridFor(tempo, SR, 1e6).division).toBe(96_000 * 32);
  });
  it("snaps to the division when on", () => {
    expect(snap(6_100, 6_000, true)).toBe(6_000);
    expect(snap(8_999, 6_000, true)).toBe(6_000);
    expect(snap(9_001, 6_000, true)).toBe(12_000);
    expect(snap(6_100, 6_000, false)).toBe(6_100);
    expect(snap(-50, 6_000, true)).toBe(0);
  });
  it("labels bars, beats and sixteenths one-based", () => {
    const g = gridFor(tempo, SR, 100);
    expect(musicalLabel(0, g)).toBe("1.1.1");
    expect(musicalLabel(96_000 + 24_000 + 6_000, g)).toBe("2.2.2");
  });
});

describe("hitTest", () => {
  const view: View = { scroll: 0, spp: 100 };
  const tracks = [[clip(1, 1000, 10_000)], []];
  it("finds the ruler, clips, edges and empty lanes", () => {
    expect(hitTest(tracks, 5, 10, view)).toEqual({ kind: "ruler" });
    const y = RULER_HEIGHT + 10;
    expect(hitTest(tracks, 60, y, view)?.kind).toBe("clip");
    expect(hitTest(tracks, 12, y, view)?.kind).toBe("left");
    expect(hitTest(tracks, 108, y, view)?.kind).toBe("right");
    expect(hitTest(tracks, 200, y, view)).toEqual({ kind: "empty", track: 0 });
    expect(hitTest(tracks, 60, y + TRACK_HEIGHT, view)).toEqual({ kind: "empty", track: 1 });
    expect(hitTest(tracks, 60, y + 2 * TRACK_HEIGHT, view)).toBeNull();
  });
  it("treats a tiny clip as all body", () => {
    const tiny = [[clip(1, 0, 1000)]];
    expect(hitTest(tiny, 1, RULER_HEIGHT + 1, view)?.kind).toBe("clip");
  });
});

describe("edits", () => {
  const c = clip(1, 10_000, 20_000, 500);
  it("moves with snapping", () => {
    expect(moved(c, 2_100, 6_000, true)).toEqual({ start: 12_000, offset: 500, length: 20_000 });
    expect(moved(c, -20_000, 6_000, false)).toEqual({ start: 0, offset: 500, length: 20_000 });
  });
  it("trims the left edge keeping the audio in place", () => {
    expect(trimmedLeft(c, 16_000, 6_000, false)).toEqual({
      start: 16_000,
      offset: 6_500,
      length: 14_000,
    });
    // Not before the file's first frame.
    expect(trimmedLeft(c, 0, 6_000, false)).toEqual({ start: 9_500, offset: 0, length: 20_500 });
    // Not past the clip's end.
    expect(trimmedLeft(c, 99_000, 6_000, false).length).toBe(1);
  });
  it("trims the right edge within the file", () => {
    expect(trimmedRight(c, 20_000, 6_000, false)).toEqual({
      start: 10_000,
      offset: 500,
      length: 10_000,
    });
    // Cannot grow without knowing the file length.
    expect(trimmedRight(c, 99_000, 6_000, false).length).toBe(20_000);
    // Can grow up to the file's end when it is known.
    expect(trimmedRight(c, 99_000, 6_000, false, 25_000).length).toBe(24_500);
    expect(trimmedRight(c, 0, 6_000, false).length).toBe(1);
  });
  it("splits only strictly inside", () => {
    expect(canSplit(c, 10_000)).toBe(false);
    expect(canSplit(c, 20_000)).toBe(true);
    expect(canSplit(c, 30_000)).toBe(false);
  });
});

describe("pattern clips", () => {
  const spt = samplesPerTick(tempo, SR);
  const pattern: PatternInfo = {
    id: 11,
    name: "Beat",
    stepsPerBeat: 4,
    length: 16,
    swing: 0,
    rows: [
      { pad: "kick", steps: [1, ...new Array(7).fill(0), 0.5, ...new Array(7).fill(0)] },
      { pad: "snare", steps: [0, 0, 0, 0, 1, ...new Array(11).fill(0)] },
    ],
  };
  it("convert between ticks and samples", () => {
    expect(spt).toBe(25);
    const lane = patternLane(
      { id: 7, pattern: 11, start: 960, offset: 240, length: 3840 },
      "Beat",
      spt,
    );
    expect(lane).toEqual({
      id: 7,
      name: "Beat",
      file: "",
      start: 24_000,
      offset: 6_000,
      length: 96_000,
    });
    expect(laneToTicks({ start: 24_010, offset: 6_000, length: 96_000 }, spt)).toEqual({
      start: 960,
      offset: 240,
      length: 3840,
    });
    expect(laneToTicks({ start: 0, offset: 0, length: 3 }, spt).length).toBe(1);
  });
  it("preview the hits of a looping, offset clip within a window", () => {
    const clip = { id: 7, pattern: 11, start: 0, offset: 960, length: 7680 };
    const hits = previewHits(clip, pattern, ["kick", "snare"], spt, 0, 7680 * spt);
    // Offset a beat: the snare (step 4) lands at tick 0, the half kick
    // (step 8) a beat later, the kick (step 0) at the next pass.
    expect(hits).toEqual([
      { row: 1, at: 0, velocity: 1 },
      { row: 0, at: 960 * spt, velocity: 0.5 },
      { row: 0, at: 2880 * spt, velocity: 1 },
      { row: 1, at: 3840 * spt, velocity: 1 },
      { row: 0, at: 4800 * spt, velocity: 0.5 },
      { row: 0, at: 6720 * spt, velocity: 1 },
    ]);
    // A window sees only its hits.
    expect(previewHits(clip, pattern, ["kick"], spt, 1000 * spt, 3000 * spt)).toEqual([
      { row: 0, at: 2880 * spt, velocity: 1 },
    ]);
    expect(previewHits(clip, pattern, ["ride"], spt, 0, 1e9)).toEqual([]);
  });
});
