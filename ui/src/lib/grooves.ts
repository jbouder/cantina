import type { StockGroove } from "./engine";

/** How each feel is written in the browser. */
export const FEELS: Record<StockGroove["feel"], string> = {
  straight: "straight",
  shuffle: "shuffle",
  swing: "swing",
  "half-time": "half time",
  "double-time": "double time",
};

export const meterOf = (g: StockGroove) =>
  `${g.timeSignature.numerator}/${g.timeSignature.denominator}`;

/** The grooves that pass the filters and the search, in library order. */
export function filterGrooves(
  grooves: StockGroove[],
  genre: string,
  section: string,
  meter: string,
  query: string,
): StockGroove[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return grooves.filter((g) => {
    if (genre && g.genre !== genre) return false;
    if (section && g.section !== section) return false;
    if (meter && meterOf(g) !== meter) return false;
    const text = `${g.name} ${g.genre} ${g.section} ${FEELS[g.feel]} ${meterOf(g)}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}
