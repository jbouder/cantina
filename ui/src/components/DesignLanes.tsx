import { useRef, useState } from "react";
import { SECTION_KINDS } from "../lib/designEdit";
import { type DrumPart, fillBars, layout, orderBars, patternTicks } from "../lib/drumPart";
import type { GrooveSection } from "../lib/engine";
import { Button } from "./Button";
import { ContextMenu } from "./ContextMenu";
import { PATTERN_DRAG } from "./DesignPalette";
import { Led } from "./Led";

type Props = {
  part: DrumPart;
  /** The section the grid edits. */
  editing: string;
  /** The section and the step of the order under the playhead, while it rolls. */
  playingSection: string | null;
  playingStep: number | null;
  /** The step of the order the inspector shows, if any. */
  selectedStep: number | null;
  barTicks: number;
  onEdit: (section: string) => void;
  onAddSection: (kind: GrooveSection | null) => void;
  onSelectStep: (step: number) => void;
  onAddStep: () => void;
  /** A pattern file dropped from the palette onto a section. */
  onDropPattern: (section: string, path: string) => void;
};

/** `4 bars`, `1 bar`, `6 beats` for a length in ticks. */
function barsLabel(ticks: number, bar: number): string {
  const bars = ticks / Math.max(1, bar);
  if (Number.isInteger(bars)) return `${bars} bar${bars === 1 ? "" : "s"}`;
  return `${Math.round((ticks / 960) * 10) / 10} beats`;
}

/**
 * The two lanes across the top of the drum designer: the part's
 * sections, one of which the grid edits, and the order that arranges
 * them into a song, each step as wide as its bars.
 */
export function DesignLanes({
  part,
  editing,
  playingSection,
  playingStep,
  selectedStep,
  barTicks,
  onEdit,
  onAddSection,
  onSelectStep,
  onAddStep,
  onDropPattern,
}: Props) {
  const [over, setOver] = useState<string | null>(null);
  const addKey = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const spans = layout(part);
  const fills = new Set(fillBars(part, barTicks));
  const total = orderBars(part, barTicks);

  return (
    <div className="grid gap-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="caption w-16 shrink-0">Sections</span>
        <div
          role="tablist"
          aria-label="Sections: the grid edits the one chosen; 1 to 9 choose"
          className="scroll-area flex min-w-0 flex-1 gap-1"
        >
          {part.sections.map((s, i) => (
            <Button
              key={s.name}
              role="tab"
              aria-selected={s.name === editing}
              title={`${s.name}: ${barsLabel(patternTicks(s.pattern), barTicks)}${s.fill ? `, a fill from ${s.fill.section}` : ""}${i < 9 ? ` (${i + 1})` : ""}`}
              onClick={() => onEdit(s.name)}
              onDragOver={(e) => {
                if (!e.dataTransfer.types.includes(PATTERN_DRAG)) return;
                e.preventDefault();
                e.dataTransfer.dropEffect = "copy";
                setOver(s.name);
              }}
              onDragLeave={() => setOver(null)}
              onDrop={(e) => {
                const path = e.dataTransfer.getData(PATTERN_DRAG);
                setOver(null);
                if (!path) return;
                e.preventDefault();
                onDropPattern(s.name, path);
              }}
              className={over === s.name ? "outline-2 outline-focus outline-solid" : ""}
            >
              <Led on={s.name === playingSection} label="Playing" />
              {s.name}
            </Button>
          ))}
        </div>
        <Button
          ref={addKey}
          square
          aria-label="Add a section"
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          title="Add a section: intro, verse, chorus, bridge, fill, ending or your own"
          onClick={() => {
            const r = addKey.current?.getBoundingClientRect();
            setMenu(menu || !r ? null : { x: r.left, y: r.bottom + 2 });
          }}
        >
          +
        </Button>
        {menu ? (
          <ContextMenu
            label="Add a section"
            x={menu.x}
            y={menu.y}
            onClose={() => setMenu(null)}
            items={[
              ...SECTION_KINDS.map((k) => ({
                label: k.label,
                onSelect: () => onAddSection(k.kind),
              })),
              { label: "Your own", separated: true, onSelect: () => onAddSection(null) },
            ]}
          />
        ) : null}
      </div>

      <div className="flex min-w-0 items-center gap-2">
        <span className="caption w-16 shrink-0" title="The song: the sections in order">
          Order
        </span>
        <ol
          aria-label={`Order: ${part.order.length} steps, ${total} bars`}
          className="scroll-area m-0 flex min-w-0 flex-1 list-none gap-1 p-0"
        >
          {spans.map((span) => {
            const step = part.order[span.step];
            if (!step) return null;
            const bars = span.length / Math.max(1, barTicks);
            const first = Math.floor(span.start / Math.max(1, barTicks));
            const filled = Array.from({ length: Math.floor(bars) }, (_, b) => first + b).filter(
              (b) => fills.has(b),
            ).length;
            return (
              <li
                key={`${span.step}-${step.section}`}
                className="min-w-24 shrink-0"
                style={{ flexGrow: bars, flexBasis: `${Math.max(6, bars * 1.5)}rem` }}
              >
                <button
                  type="button"
                  aria-pressed={selectedStep === span.step}
                  title={`Step ${span.step + 1}: ${step.section} ×${step.times}, bars ${first + 1} to ${first + Math.ceil(bars)}${filled > 0 ? `, ${filled} with a fill` : ""}`}
                  onClick={() => onSelectStep(span.step)}
                  className="surface-lcd flex h-10 w-full min-w-0 cursor-pointer flex-col items-start justify-center border-0 px-2 text-left font-lcd text-lcd-fg aria-pressed:outline-2 aria-pressed:outline-focus aria-pressed:outline-solid aria-pressed:-outline-offset-2"
                >
                  <span className="lcd-text flex w-full min-w-0 items-center gap-1 text-readout">
                    <Led on={playingStep === span.step} label="Playing" />
                    <span className="min-w-0 truncate">
                      {step.section} ×{step.times}
                    </span>
                  </span>
                  <span className="lcd-text text-label">
                    {barsLabel(span.length, barTicks)}
                    {filled > 0 ? ` · ${filled} fill${filled === 1 ? "" : "s"}` : ""}
                  </span>
                </button>
              </li>
            );
          })}
          {spans.length === 0 ? (
            <li className="flex items-center text-ink-dim text-body">
              No steps yet: add the section you are editing.
            </li>
          ) : null}
        </ol>
        <span className="lcd-text shrink-0 font-lcd text-label" aria-hidden="true">
          {total} bars
        </span>
        <Button
          square
          aria-label={`Add ${editing} to the order`}
          title={`Add ${editing} to the end of the order`}
          onClick={onAddStep}
        >
          +
        </Button>
      </div>
    </div>
  );
}
