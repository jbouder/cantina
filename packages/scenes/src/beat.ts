/**
 * Bass-onset beat detection. Pure: the same inputs always give the same state,
 * so it can be tested with synthetic signals and no AudioContext.
 *
 * An onset fires when bass energy exceeds THRESHOLD times its average over the
 * last HISTORY frames, outside a refractory window. The output decays linearly
 * from 1 to 0 over DECAY seconds.
 */
export const BEAT_HISTORY = 43;
export const BEAT_THRESHOLD = 1.4;
export const BEAT_REFRACTORY = 0.25;
export const BEAT_DECAY = 0.3;
/** Ignore near-silence so noise floors do not trigger onsets. */
export const BEAT_MIN_LEVEL = 0.08;
/** Frames of history required before onsets can fire. */
export const BEAT_WARMUP = 8;

export interface BeatState {
  readonly history: readonly number[];
  readonly lastOnset: number;
  readonly lastTime: number;
  readonly beat: number;
}

export const initialBeatState: BeatState = {
  history: [],
  lastOnset: Number.NEGATIVE_INFINITY,
  lastTime: Number.NaN,
  beat: 0,
};

export interface BeatResult {
  state: BeatState;
  onset: boolean;
  beat: number;
}

export function stepBeat(state: BeatState, bass: number, time: number): BeatResult {
  const { history } = state;
  const dt = Number.isNaN(state.lastTime) ? 0 : Math.max(0, time - state.lastTime);

  let avg = 0;
  for (const v of history) avg += v;
  avg = history.length ? avg / history.length : 0;

  const onset =
    history.length >= BEAT_WARMUP &&
    bass >= BEAT_MIN_LEVEL &&
    bass > avg * BEAT_THRESHOLD &&
    time - state.lastOnset >= BEAT_REFRACTORY;

  const beat = onset ? 1 : Math.max(0, state.beat - dt / BEAT_DECAY);

  const next = history.length >= BEAT_HISTORY ? history.slice(1) : history.slice();
  next.push(bass);

  return {
    onset,
    beat,
    state: {
      history: next,
      lastOnset: onset ? time : state.lastOnset,
      lastTime: time,
      beat,
    },
  };
}
