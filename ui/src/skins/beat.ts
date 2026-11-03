/*
 * Drives `--beat` on the document from the engine's analysis frames, for
 * skins whose CSS pulses with it (Neon's edge). The beat comes from the
 * tempo map while the transport rolls, so the pulse lands on the grid,
 * and from onsets heard in the input while it is stopped; the scenes'
 * EngineFrames already does exactly that. The property is set only while
 * a skin that reads it is on, and held at 0 under reduced motion or
 * Reduce flashing.
 */

import { type EngineAnalysis, EngineFrames } from "@cantina/scenes";

export interface BeatTarget {
  setProperty(name: string, value: string): void;
  removeProperty(name: string): void;
}

export interface SkinBeat {
  /** Takes one analysis frame and updates the property. */
  push(analysis: EngineAnalysis): void;
}

/** `active` says whether the property may move right now. */
export function createSkinBeat(target: BeatTarget, active: () => boolean): SkinBeat {
  const frames = new EngineFrames();
  let set = false;
  return {
    push(analysis) {
      // The onset detector keeps listening even while the property rests,
      // so it is ready when the skin or the setting changes.
      frames.push(analysis);
      const beat = frames.readFrame().beat;
      if (!active()) {
        if (set) target.removeProperty("--beat");
        set = false;
        return;
      }
      target.setProperty("--beat", beat.toFixed(3));
      set = true;
    },
  };
}
