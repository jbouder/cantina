/*
 * Tapster's beeps and chirps, synthesised in the window with Web Audio:
 * a few quick rising blips when a reply lands, a falling warble on an
 * error. They play through the system's output from the web view, never
 * through the engine, so they are never in a recording, a mixdown or the
 * stage. Off unless Settings ▸ Assistant ▸ Droid sounds is on.
 */

let context: AudioContext | null = null;

function audio(): AudioContext | null {
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  context ??= new window.AudioContext();
  if (context.state === "suspended") void context.resume();
  return context;
}

/** One blip: a tone gliding from `from` to `to` Hz, with a quick fade. */
function blip(
  ctx: AudioContext,
  at: number,
  from: number,
  to: number,
  length: number,
  level: number,
) {
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.type = "triangle";
  osc.frequency.setValueAtTime(from, at);
  osc.frequency.exponentialRampToValueAtTime(to, at + length);
  gain.gain.setValueAtTime(0, at);
  gain.gain.linearRampToValueAtTime(level, at + 0.005);
  gain.gain.exponentialRampToValueAtTime(0.0001, at + length);
  osc.connect(gain).connect(ctx.destination);
  osc.start(at);
  osc.stop(at + length + 0.02);
}

/** The pitches of one chirp: a short phrase that differs each time. */
export function phrase(kind: "reply" | "error", random: () => number = Math.random): number[][] {
  if (kind === "error") {
    return [
      [520, 380, 0.12],
      [420, 240, 0.12],
      [300, 140, 0.22],
    ];
  }
  const notes = 3 + Math.floor(random() * 2);
  return Array.from({ length: notes }, () => {
    const from = 900 + random() * 1300;
    return [from, from * (1.2 + random() * 0.6), 0.045 + random() * 0.035];
  });
}

/** Plays a chirp: `reply` when an answer lands, `error` when it fails. */
export function chirp(kind: "reply" | "error") {
  const ctx = audio();
  if (!ctx) return;
  let at = ctx.currentTime + 0.01;
  for (const [from = 1000, to = 1200, length = 0.05] of phrase(kind)) {
    blip(ctx, at, from, to, length, kind === "error" ? 0.12 : 0.08);
    at += length + 0.025;
  }
}
