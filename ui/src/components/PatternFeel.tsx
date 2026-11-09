import { useEffect, useId, useState } from "react";
import type { Humanize, PatternInfo } from "../lib/engine";
import { withPatternFeel, withRowFeel } from "../lib/gridEdit";
import { Button } from "./Button";

type Props = {
  pattern: PatternInfo;
  /** The row the grid's focus is on, by pad id, and its name. */
  pad: { id: string; name: string } | null;
  /** Sends the pattern as it should be: one undoable edit. */
  onReplace: (next: PatternInfo) => void;
};

const LABEL = "font-bold text-[11px] uppercase tracking-[0.08em]";

/**
 * A 0-to-1 amount as a percentage fader. It moves freely while dragged
 * and sends one edit when let go, so a drag is one step of undo.
 */
function Amount({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: number;
  onCommit: (value: number) => void;
}) {
  const id = useId();
  const [shown, setShown] = useState(value);
  useEffect(() => setShown(value), [value]);
  const commit = () => {
    if (shown !== value) onCommit(shown);
  };
  return (
    <span className="grid w-36 gap-0.5">
      <span className="flex items-baseline justify-between">
        <label htmlFor={id} className={LABEL}>
          {label}
        </label>
        <output
          htmlFor={id}
          className="surface-lcd px-1.5 py-0.5 font-lcd text-sm leading-none tabular-nums"
        >
          <span className="lcd-text">{Math.round(shown * 100)}%</span>
        </output>
      </span>
      <input
        id={id}
        type="range"
        className="fader"
        min={0}
        max={1}
        step={0.01}
        value={shown}
        aria-valuetext={`${Math.round(shown * 100)}%`}
        onChange={(e) => setShown(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
        onDoubleClick={() => onCommit(0)}
      />
    </span>
  );
}

const NONE: Humanize = { timing: 0, velocity: 0 };

/**
 * How a pattern feels: humanize for the whole pattern and a new take of
 * it, and for the focused row its own swing and humanize.
 */
export function PatternFeel({ pattern, pad, onReplace }: Props) {
  const human = pattern.humanize ?? NONE;
  const row = pad ? pattern.rows.find((r) => r.pad === pad.id) : undefined;
  const setPattern = (next: Humanize) => onReplace(withPatternFeel(pattern, next));
  return (
    <section aria-label="Feel" className="flex flex-wrap items-end gap-x-4 gap-y-2">
      <span className={`${LABEL} self-center`}>Humanize</span>
      <Amount
        label="Timing"
        value={human.timing}
        onCommit={(timing) => setPattern({ ...human, timing })}
      />
      <Amount
        label="Velocity"
        value={human.velocity}
        onCommit={(velocity) => setPattern({ ...human, velocity })}
      />
      <Button
        onClick={() =>
          onReplace(
            withPatternFeel(pattern, pattern.humanize ?? null, 1 + Math.floor(Math.random() * 1e9)),
          )
        }
        title="Roll the humanize and chances again: another take, the same every time it plays"
      >
        New take
      </Button>
      {pad ? (
        <>
          <span className={`${LABEL} self-center`}>{pad.name}</span>
          <Button
            aria-pressed={row?.swing !== undefined}
            variant={row?.swing !== undefined ? "transport" : "panel"}
            onClick={() =>
              onReplace(
                withRowFeel(pattern, pad.id, {
                  swing: row?.swing !== undefined ? null : pattern.swing,
                }),
              )
            }
            title={`Give ${pad.name} its own swing instead of the pattern's`}
          >
            Own swing
          </Button>
          {row?.swing !== undefined ? (
            <Amount
              label="Swing"
              value={row.swing}
              onCommit={(swing) => onReplace(withRowFeel(pattern, pad.id, { swing }))}
            />
          ) : null}
          <Button
            aria-pressed={row?.humanize !== undefined}
            variant={row?.humanize !== undefined ? "transport" : "panel"}
            onClick={() =>
              onReplace(
                withRowFeel(pattern, pad.id, {
                  humanize: row?.humanize !== undefined ? null : { ...human },
                }),
              )
            }
            title={`Give ${pad.name} its own humanize instead of the pattern's`}
          >
            Own humanize
          </Button>
          {row?.humanize ? (
            <>
              <Amount
                label="Timing"
                value={row.humanize.timing}
                onCommit={(timing) =>
                  onReplace(
                    withRowFeel(pattern, pad.id, {
                      humanize: { ...(row.humanize ?? NONE), timing },
                    }),
                  )
                }
              />
              <Amount
                label="Velocity"
                value={row.humanize.velocity}
                onCommit={(velocity) =>
                  onReplace(
                    withRowFeel(pattern, pad.id, {
                      humanize: { ...(row.humanize ?? NONE), velocity },
                    }),
                  )
                }
              />
            </>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
