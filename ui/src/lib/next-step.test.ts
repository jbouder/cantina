import { describe, expect, it } from "vitest";
import type { TrackStatus } from "./engine";
import { nextStep, trackToArm } from "./next-step";

function track(id: number, name: string, extra: Partial<TrackStatus> = {}): TrackStatus {
  return {
    id,
    name,
    armed: false,
    recording: false,
    kit: null,
    chain: [],
    clips: [],
    patternClips: [],
    strip: { gainDb: 0, pan: 0, mute: false, solo: false },
    latencyFrames: 0,
    pads: {},
    ...extra,
  };
}

const drums = track(9, "Drums", { kit: { id: "k", name: "Kit" } as TrackStatus["kit"] });
const idle = { playing: false, recording: false };

describe("the next step to recording", () => {
  it("starts with the engine", () => {
    const step = nextStep(idle, [track(1, "Guitar")], 0, false, null);
    expect(step.action).toEqual({ kind: "start" });
  });

  it("then asks for a guitar track", () => {
    const step = nextStep(idle, [drums], 0, true, null);
    expect(step.text).toBe("Add a guitar track to record");
    expect(step.action).toBeNull();
  });

  it("then offers to arm the selected audio track", () => {
    const tracks = [track(1, "Guitar"), track(2, "Bass")];
    const step = nextStep(idle, tracks, 0, true, 2);
    expect(step.action).toEqual({ kind: "arm", track: tracks[1] });
  });

  it("is ready once a track is armed", () => {
    const step = nextStep(idle, [track(1, "Guitar", { armed: true })], 0, true, null);
    expect(step).toEqual({ text: "Guitar armed · press Record or R", action: null });
  });

  it("says what the transport is doing first", () => {
    const tracks = [track(1, "Guitar", { armed: true })];
    expect(nextStep(idle, tracks, 3, true, null).text).toBe("Counting in…");
    expect(nextStep({ playing: true, recording: true }, tracks, 0, true, null).text).toBe(
      "Recording Guitar…",
    );
  });
});

describe("the track Record arms", () => {
  it("is the selected one when it takes audio, else the first that does", () => {
    const tracks = [drums, track(1, "Guitar"), track(2, "Bass")];
    expect(trackToArm(tracks, 2)?.name).toBe("Bass");
    expect(trackToArm(tracks, 9)?.name).toBe("Guitar");
    expect(trackToArm(tracks, null)?.name).toBe("Guitar");
    expect(trackToArm([drums], 9)).toBeNull();
  });
});
