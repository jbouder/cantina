/** A card's horizontal extent on screen, as `getBoundingClientRect` gives it. */
export type Span = { left: number; right: number };

/** How far the pointer must travel before a press becomes a drag, in px. */
export const DRAG_THRESHOLD = 4;

/**
 * Where a card dragged to `x` lands: the index of the card whose center
 * is nearest, so the gaps between cards and the space past either end
 * still pick a place. -1 for an empty row.
 */
export function dropIndex(x: number, spans: Span[]): number {
  let best = -1;
  let bestDistance = Number.POSITIVE_INFINITY;
  spans.forEach((s, i) => {
    const distance = Math.abs(x - (s.left + s.right) / 2);
    if (distance < bestDistance) {
      best = i;
      bestDistance = distance;
    }
  });
  return best;
}
