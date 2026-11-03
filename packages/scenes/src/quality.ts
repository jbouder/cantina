/** Render scale levels, highest first. Auto mode only ever steps down. */
export const QUALITY_LEVELS = [1, 0.75, 0.5] as const;

export const SLOW_FRAME_MS = 22;
export const SLOW_WINDOW_S = 2;

/**
 * Tracks frame time and reports when Auto quality should step down: the
 * average frame time has stayed above SLOW_FRAME_MS for SLOW_WINDOW_S.
 */
export class QualityGovernor {
  private avg = 16.7;
  private slowFor = 0;

  reset(): void {
    this.avg = 16.7;
    this.slowFor = 0;
  }

  /** Returns true when the caller should step down one level. */
  sample(dtSeconds: number): boolean {
    const ms = dtSeconds * 1000;
    this.avg += (ms - this.avg) * 0.05;
    if (this.avg > SLOW_FRAME_MS) this.slowFor += dtSeconds;
    else this.slowFor = 0;
    if (this.slowFor >= SLOW_WINDOW_S) {
      this.reset();
      return true;
    }
    return false;
  }
}

export function nextLowerQuality(q: number): number | null {
  const i = QUALITY_LEVELS.findIndex((l) => l <= q + 1e-6);
  return i >= 0 && i < QUALITY_LEVELS.length - 1 ? QUALITY_LEVELS[i + 1] : null;
}
