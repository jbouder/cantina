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

/**
 * Where a row of `count` rows, each `height` tall, lands when dragged
 * `dy` pixels from index `from`: the nearest row, kept in the list.
 */
export function rowDropIndex(from: number, dy: number, height: number, count: number): number {
  if (count <= 0) return -1;
  return Math.max(0, Math.min(count - 1, from + Math.round(dy / height)));
}
