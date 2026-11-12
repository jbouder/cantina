import { useMemo } from "react";
import { fretboardDots, neckFrets } from "../lib/diagram";
import type { Tab } from "../lib/engine";
import { useLeftHanded } from "../lib/handedness";
import { stringNames } from "../lib/tabLayout";

/** Pixels between strings, the width of a fret, and the open strings' room. */
const STRING = 13;
const FRET = 38;
const OPEN = 30;
/** Room for the string names and the fret numbers. */
const NAMES = 18;
const NUMBERS = 16;

const INLAYS = [3, 5, 7, 9, 15, 17, 19, 21];
const DOUBLES = [12, 24];

type Props = {
  tab: Tab;
  /** Ticks into the tab the playhead is at, or null when stopped. */
  tick: number | null;
};

/**
 * The neck in the tab's tuning: strings across, the highest on top as tab
 * reads, frets counted from the nut with the capo drawn on its fret. The
 * phrase's notes are faint dots, the shape to learn; the beat sounding is
 * lit and the one coming is a ring. Left-handed, the nut is on the right.
 */
export function Fretboard({ tab, tick }: Props) {
  const [lefty] = useLeftHanded();
  const frets = neckFrets(tab);
  const strings = tab.tuning.length;
  const dots = useMemo(() => fretboardDots(tab, tick), [tab, tick]);
  const names = stringNames(tab);
  const capo = tab.capo ?? 0;
  const board = OPEN + frets * FRET;
  const width = NAMES + board + 4;
  const height = (strings - 1) * STRING + 12 + NUMBERS;
  const top = 6;
  // x of a fret's wire (0 the nut), and of the middle of a fret, from the names' side.
  const wire = (f: number) => NAMES + OPEN + f * FRET;
  const middle = (f: number) => (f === 0 ? NAMES + OPEN / 2 : wire(f) - FRET / 2);
  const flip = (x: number) => (lefty ? width - x : x);
  const y = (string: number) => top + (string - 1) * STRING;
  const lit = dots.filter((d) => d.kind !== "shape").length;
  return (
    <div className="surface-lcd scroll-area min-w-0 p-1">
      <svg
        role="img"
        aria-label={`Fretboard${lefty ? ", left-handed" : ""}: ${dots.length} notes in the phrase, ${lit} lit`}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        style={{ minWidth: Math.min(width, NAMES + frets * 24) }}
        className="block font-lcd"
      >
        {INLAYS.filter((f) => f <= frets).map((f) => (
          <circle
            key={f}
            cx={flip(middle(f))}
            cy={top + ((strings - 1) * STRING) / 2}
            r={3}
            className="fill-lcd-dim"
          />
        ))}
        {DOUBLES.filter((f) => f <= frets).flatMap((f) =>
          [1, 3].map((k) => (
            <circle
              key={`${f}-${k}`}
              cx={flip(middle(f))}
              cy={top + ((strings - 1) * STRING * k) / 4}
              r={3}
              className="fill-lcd-dim"
            />
          )),
        )}
        {Array.from({ length: frets + 1 }, (_, f) => (
          <line
            // biome-ignore lint/suspicious/noArrayIndexKey: frets are positions
            key={f}
            x1={flip(wire(f))}
            x2={flip(wire(f))}
            y1={top}
            y2={y(strings)}
            className="stroke-lcd-fg"
            strokeWidth={f === 0 ? 4 : 1}
            opacity={f === 0 ? 0.9 : 0.4}
          />
        ))}
        {capo > 0 ? (
          <rect
            x={flip(wire(capo) - FRET / 2) - 3}
            y={top - 4}
            width={6}
            height={(strings - 1) * STRING + 8}
            rx={3}
            className="fill-lcd-muted"
          >
            <title>Capo at fret {capo}</title>
          </rect>
        ) : null}
        {names.map((name, i) => (
          <g
            // biome-ignore lint/suspicious/noArrayIndexKey: strings are positions
            key={i}
          >
            <line
              x1={flip(NAMES)}
              x2={flip(NAMES + board)}
              y1={y(i + 1)}
              y2={y(i + 1)}
              className="stroke-lcd-fg"
              opacity={0.55}
            />
            <text
              x={flip(NAMES / 2)}
              y={y(i + 1) + 4}
              textAnchor="middle"
              className="fill-lcd-fg text-label"
            >
              {name}
            </text>
          </g>
        ))}
        {[...INLAYS, ...DOUBLES]
          .filter((f) => f <= frets)
          .map((f) => (
            <text
              key={f}
              x={flip(middle(f))}
              y={height - 3}
              textAnchor="middle"
              className="fill-lcd-fg text-label"
              opacity={0.7}
            >
              {f}
            </text>
          ))}
        {dots.map((d) => {
          const cx = flip(middle(d.fret));
          const cy = y(d.string);
          return d.kind === "next" ? (
            <circle
              key={`${d.string}-${d.fret}`}
              cx={cx}
              cy={cy}
              r={5.5}
              className="fill-none stroke-lcd-fg"
              strokeWidth={2}
            />
          ) : (
            <circle
              key={`${d.string}-${d.fret}`}
              cx={cx}
              cy={cy}
              r={d.kind === "now" ? 6 : 4}
              className="fill-lcd-fg"
              opacity={d.kind === "now" ? 1 : 0.35}
            />
          );
        })}
      </svg>
    </div>
  );
}
