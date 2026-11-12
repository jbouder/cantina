import type { TrackStatus, TransportStatus } from "./engine";

/**
 * What the status line says, and the one thing it offers to do about it:
 * start the engine, or arm a track (`track`), on the way to recording.
 */
export type NextStep = {
  text: string;
  action: { kind: "start" } | { kind: "arm"; track: TrackStatus } | null;
};

/**
 * The track Record arms when none is: the selected one if it takes
 * audio, otherwise the first that does; null with no audio track.
 */
export function trackToArm(tracks: TrackStatus[], selected: number | null): TrackStatus | null {
  const audio = tracks.filter((t) => !t.kit);
  return audio.find((t) => t.id === selected) ?? audio[0] ?? null;
}

/**
 * The status line: what the transport is doing, or the next step to
 * recording, in order: the engine on, a guitar track, a track armed.
 */
export function nextStep(
  transport: TransportStatus,
  tracks: TrackStatus[],
  countIn: number,
  running: boolean,
  selected: number | null,
): NextStep {
  const armed = tracks.filter((t) => t.armed && !t.kit);
  const names = armed.map((t) => t.name).join(", ");
  if (countIn > 0) return { text: "Counting in…", action: null };
  if (transport.recording) return { text: `Recording ${names}…`, action: null };
  if (!running) {
    return {
      text: "The engine is off: start it to hear your instrument, or press Record",
      action: { kind: "start" },
    };
  }
  if (armed.length > 0) return { text: `${names} armed · press Record or R`, action: null };
  const target = trackToArm(tracks, selected);
  if (!target) return { text: "Add a guitar track to record", action: null };
  return {
    text: `No track armed: arm ${target.name}, or press Record to arm it and go`,
    action: { kind: "arm", track: target },
  };
}
