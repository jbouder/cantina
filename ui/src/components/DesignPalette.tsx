import { type DragEvent, useId, useState } from "react";
import type { GrooveInfo, LibraryPattern, StockGroove } from "../lib/engine";
import { FEELS } from "../lib/grooves";
import { Button } from "./Button";
import { Select } from "./Select";

/** The drag type a pattern file carries from the palette onto a section. */
export const PATTERN_DRAG = "application/x-cantina-pattern";

type Props = {
  /** The stock grooves, by genre then section. */
  grooves: StockGroove[];
  /** The user's whole parts in Grooves. */
  mine: GrooveInfo[];
  /** The user's patterns in Patterns. */
  patterns: LibraryPattern[];
  /** The project's time signature: grooves in another are marked. */
  meter: { numerator: number; denominator: number };
  /** The section a click puts a pattern in. */
  editing: string;
  onUsePattern: (path: string) => void;
  onUseGenre: (genre: string) => void;
  onOpenGroove: (path: string) => void;
};

const SECTION_LABELS: Record<StockGroove["section"], string> = {
  intro: "Intro",
  verse: "Verse",
  chorus: "Chorus",
  bridge: "Bridge",
  fill: "Fill",
  ending: "Ending",
};

function dragPattern(e: DragEvent<HTMLElement>, path: string) {
  e.dataTransfer.setData(PATTERN_DRAG, path);
  e.dataTransfer.effectAllowed = "copy";
}

/**
 * The drum designer's palette: the stock grooves by genre, each section
 * of one to drag onto a section or put in the one being edited, or the
 * whole genre at once; your grooves, to open whole; and your patterns.
 */
export function DesignPalette({
  grooves,
  mine,
  patterns,
  meter,
  editing,
  onUsePattern,
  onUseGenre,
  onOpenGroove,
}: Props) {
  const ids = useId();
  const genres = [...new Set(grooves.map((g) => g.genre))];
  const [genre, setGenre] = useState(genres[0] ?? "");
  const shown = grooves.filter((g) => g.genre === genre);
  const sameMeter = (m: { numerator: number; denominator: number }) =>
    m.numerator === meter.numerator && m.denominator === meter.denominator;

  return (
    <div className="grid content-start gap-3">
      <p className="m-0 text-ink-dim text-label">
        Drag a pattern onto a section in the lane, or click to put it in {editing}.
      </p>
      <h3 className="caption m-0">Stock grooves</h3>
      <Select
        id={`${ids}-genre`}
        label="Genre"
        value={genre}
        onChange={(e) => setGenre(e.target.value)}
      >
        {genres.map((g) => (
          <option key={g} value={g}>
            {g}
          </option>
        ))}
      </Select>
      <ul className="m-0 grid list-none gap-1 p-0">
        {shown.map((g) => (
          <li key={g.path}>
            <button
              type="button"
              draggable
              onDragStart={(e) => dragPattern(e, g.path)}
              onClick={() => onUsePattern(g.path)}
              title={`Put the ${g.genre} ${SECTION_LABELS[g.section].toLowerCase()} in ${editing}, or drag it onto a section`}
              className="surface-lcd flex w-full cursor-grab items-center gap-2 border-0 px-2 py-1 text-left font-lcd text-lcd-fg"
            >
              <span className="lcd-text min-w-0 flex-1 truncate text-readout">
                {SECTION_LABELS[g.section]}
              </span>
              <span className="lcd-text shrink-0 text-label">
                {sameMeter(g.timeSignature)
                  ? ""
                  : `${g.timeSignature.numerator}/${g.timeSignature.denominator} · `}
                {FEELS[g.feel]} · {g.tempo.min}–{g.tempo.max}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {shown.length > 0 ? (
        <Button
          onClick={() => onUseGenre(genre)}
          title={`Every ${genre} section into the part: each into the section of its kind, or a new one`}
        >
          Use the whole {genre} groove
        </Button>
      ) : null}

      <h3 className="caption m-0">Your grooves</h3>
      {mine.length > 0 ? (
        <ul className="m-0 grid list-none gap-1 p-0">
          {mine.map((g) => (
            <li key={g.path} className="flex items-center gap-2">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-body">{g.name}</span>
                <small className="block truncate text-ink-dim text-label">
                  {[
                    g.genre,
                    `${g.sections.length} sections`,
                    `${g.bars} bars`,
                    sameMeter(g.timeSignature)
                      ? ""
                      : `${g.timeSignature.numerator}/${g.timeSignature.denominator}`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </small>
              </span>
              <Button
                tier="quiet"
                onClick={() => onOpenGroove(g.path)}
                title={`Open ${g.name} in place of the part: its sections, order and kit`}
              >
                Open
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="m-0 text-ink-dim text-label">
          None yet: Save as groove keeps a part in Grooves in your Cantina folder.
        </p>
      )}

      {patterns.length > 0 ? (
        <>
          <h3 className="caption m-0">Your patterns</h3>
          <ul className="m-0 grid list-none gap-1 p-0">
            {patterns.map((p) => (
              <li key={p.path}>
                <button
                  type="button"
                  draggable
                  onDragStart={(e) => dragPattern(e, p.path)}
                  onClick={() => onUsePattern(p.path)}
                  title={`Put ${p.name} in ${editing}, or drag it onto a section`}
                  className="surface-lcd flex w-full cursor-grab items-center border-0 px-2 py-1 text-left font-lcd text-lcd-fg"
                >
                  <span className="lcd-text min-w-0 flex-1 truncate text-readout">{p.name}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </div>
  );
}
