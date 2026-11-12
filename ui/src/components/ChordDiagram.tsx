import type { Diagram } from "../lib/diagram";

/** Pixels between strings and between frets, full size and compact. */
const SIZES = { full: { string: 17, fret: 21, dot: 7 }, compact: { string: 13, fret: 15, dot: 5 } };
/** Room above the box for the name and the open and muted marks. */
const TOP = 34;
/** Room left of the box for a fret number. */
const LEFT = 22;

type Props = {
  diagram: Diagram;
  /** Dimmer: the chord coming next. */
  coming?: boolean;
  /** Smaller, without finger numbers, for a narrow column. */
  compact?: boolean;
};

/**
 * A chord box: strings up and down, lowest on the left; frets across,
 * the nut drawn when the shape starts at the first fret and the fret it
 * starts at named when it does not. A dot is a fretted string with its
 * finger, a bar a finger laid across, `o` an open string and `×` one not
 * played. A capo is named under the chord.
 */
export function ChordDiagram({ diagram, coming = false, compact = false }: Props) {
  const { string: STRING, fret: FRET, dot } = SIZES[compact ? "compact" : "full"];
  const strings = diagram.frets.length;
  const width = LEFT + (strings - 1) * STRING + 12;
  const height = TOP + diagram.span * FRET + (diagram.capo ? 18 : 6);
  const x = (i: number) => LEFT + i * STRING;
  const y = (fret: number) => TOP + (fret - diagram.base + 0.5) * FRET;
  const bottom = TOP + diagram.span * FRET;
  const shown = diagram.frets
    .map((f, i) => (f ? `${f}` : f === 0 ? "o" : "x") + (i < strings - 1 ? " " : ""))
    .join("");
  return (
    <svg
      role="img"
      aria-label={`${diagram.name || "Chord"}: ${shown}${diagram.capo ? `, capo ${diagram.capo}` : ""}`}
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      className="block shrink-0 font-lcd"
      opacity={coming ? 0.55 : 1}
    >
      <text
        x={LEFT + ((strings - 1) * STRING) / 2}
        y={12}
        textAnchor="middle"
        className="fill-lcd-fg text-label"
      >
        {diagram.name}
      </text>
      {diagram.frets.map((f, i) =>
        f === null || f === 0 ? (
          <text
            // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
            key={i}
            x={x(i)}
            y={TOP - 6}
            textAnchor="middle"
            className="fill-lcd-fg text-label"
          >
            {f === 0 ? "o" : "×"}
          </text>
        ) : null,
      )}
      {Array.from({ length: diagram.span + 1 }, (_, k) => (
        <line
          // biome-ignore lint/suspicious/noArrayIndexKey: frets are positions
          key={k}
          x1={x(0)}
          x2={x(strings - 1)}
          y1={TOP + k * FRET}
          y2={TOP + k * FRET}
          className="stroke-lcd-fg"
          strokeWidth={k === 0 && diagram.base === 1 ? 4 : 1}
          opacity={k === 0 && diagram.base === 1 ? 1 : 0.5}
        />
      ))}
      {Array.from({ length: strings }, (_, i) => (
        <line
          // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
          key={i}
          x1={x(i)}
          x2={x(i)}
          y1={TOP}
          y2={bottom}
          className="stroke-lcd-fg"
          opacity={0.6}
        />
      ))}
      {diagram.base > 1 ? (
        <text x={2} y={y(diagram.base) + 4} className="fill-lcd-fg text-label">
          {diagram.base}
        </text>
      ) : null}
      {diagram.barres.map((b) => (
        <rect
          key={`${b.fret}-${b.from}`}
          x={x(b.from) - dot}
          y={y(b.fret) - dot}
          width={x(b.to) - x(b.from) + 2 * dot}
          height={2 * dot}
          rx={dot}
          className="fill-lcd-fg"
        />
      ))}
      {diagram.frets.map((f, i) =>
        f ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
          <g key={i}>
            <circle cx={x(i)} cy={y(f)} r={dot} className="fill-lcd-fg" />
            {diagram.fingers[i] && !compact ? (
              <text
                x={x(i)}
                y={y(f) + 4}
                textAnchor="middle"
                className="fill-lcd-bg font-bold text-label"
              >
                {diagram.fingers[i]}
              </text>
            ) : null}
          </g>
        ) : null,
      )}
      {diagram.capo ? (
        <text
          x={LEFT + ((strings - 1) * STRING) / 2}
          y={bottom + 14}
          textAnchor="middle"
          className="fill-lcd-fg text-label"
        >
          capo {diagram.capo}
        </text>
      ) : null}
    </svg>
  );
}
