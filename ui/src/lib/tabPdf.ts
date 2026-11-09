import type { Tab } from "./engine";
import { bendLabel, layoutTab, stringNames, type TabLayout } from "./tabLayout";

/**
 * A tab as a printable PDF, drawn from the tab view's page layout
 * (`layoutTab`): the same bars, note positions, technique marks and
 * rhythm, in black on US Letter pages. Plain PDF 1.4 with the standard
 * Helvetica font, so it opens anywhere and needs nothing embedded.
 */

/** US Letter, in points. */
const PAGE_W = 612;
const PAGE_H = 792;
const MARGIN = 42;
/** Room left of a row for the string names. */
const GUTTER = 18;
const BARS_PER_ROW = 4;
/** Between string lines. */
const LINE = 9;
/** Above the staff for bar numbers and marks; below it for the rhythm. */
const ABOVE = 26;
const BELOW = 24;
const ROW_GAP = 8;
/** The header's height on the first page. */
const HEADER = 44;

/** Text for a PDF string in WinAnsi: escaped, with ½ as its byte. */
function pdfText(text: string): string {
  let out = "";
  for (const ch of text) {
    if (ch === "\\" || ch === "(" || ch === ")") out += `\\${ch}`;
    else if (ch === "½") out += "\\275";
    else if (ch.charCodeAt(0) < 128) out += ch;
    else out += "?";
  }
  return out;
}

/** Width of Helvetica text, roughly: enough to center fret numbers. */
function textWidth(text: string, size: number): number {
  return text.length * size * 0.55;
}

class Page {
  ops: string[] = ["0 0 0 RG 0 0 0 rg 0.6 w"];

  line(x1: number, y1: number, x2: number, y2: number, width = 0.6) {
    this.ops.push(
      `${width} w ${x1.toFixed(2)} ${y1.toFixed(2)} m ${x2.toFixed(2)} ${y2.toFixed(2)} l S`,
    );
  }

  text(x: number, y: number, text: string, size: number, align: "left" | "center" = "left") {
    const at = align === "center" ? x - textWidth(text, size) / 2 : x;
    this.ops.push(`BT /F1 ${size} Tf ${at.toFixed(2)} ${y.toFixed(2)} Td (${pdfText(text)}) Tj ET`);
  }

  dot(x: number, y: number, r: number) {
    // A small square reads as a dot at this size.
    this.ops.push(
      `${(x - r).toFixed(2)} ${(y - r).toFixed(2)} ${(2 * r).toFixed(2)} ${(2 * r).toFixed(2)} re f`,
    );
  }

  /** An arrow up (or down) `size` tall from (x, y), for bends. */
  arrow(x: number, y: number, size: number, up: boolean) {
    const tip = up ? y + size : y - size;
    const back = up ? tip - 2 : tip + 2;
    this.line(x, y, x, tip, 0.5);
    this.line(x - 1.5, back, x, tip, 0.5);
    this.line(x + 1.5, back, x, tip, 0.5);
  }
}

function rowHeight(strings: number): number {
  return ABOVE + (strings - 1) * LINE + BELOW + ROW_GAP;
}

/** Draws row `row` of the layout with its top at `top` (PDF y, from the bottom). */
function drawRow(page: Page, tab: Tab, layout: TabLayout, row: number, top: number) {
  const left = MARGIN + GUTTER;
  const staffTop = top - ABOVE;
  const y = (line: number) => staffTop - line * LINE;
  const bottom = y(layout.strings - 1);
  const bars = layout.bars.filter((b) => b.row === row);
  const last = bars[bars.length - 1];
  const end = last ? last.x + layout.barWidth : 0;
  const inRow = (tick: number) => {
    const bar = layout.bars.find((b, i) => {
      const next = layout.bars[i + 1];
      return tick >= b.startTick && (!next || tick < next.startTick);
    });
    return bar?.row === row;
  };
  const notes = layout.notes.filter((n) => inRow(n.note.start));
  const beats = layout.beats.filter((b) => inRow(b.tick));

  stringNames(tab).forEach((name, i) => {
    page.text(MARGIN, y(i) - 2.5, name, 7);
  });
  // Strings, broken where a fret number sits.
  for (let line = 0; line < layout.strings; line++) {
    const gaps = notes
      .filter((n) => n.line === line)
      .map((n) => {
        const half = textWidth(n.label, 7.5) / 2 + 1.5;
        return [n.x * scale(layout) - half, n.x * scale(layout) + half] as const;
      })
      .sort((a, b) => a[0] - b[0]);
    let from = 0;
    for (const [a, b] of gaps) {
      if (a > from) page.line(left + from, y(line), left + a, y(line), 0.4);
      from = Math.max(from, b);
    }
    const right = end * scale(layout);
    if (from < right) page.line(left + from, y(line), left + right, y(line), 0.4);
  }
  for (const bar of bars) {
    const x = left + bar.x * scale(layout);
    page.line(x, staffTop, x, bottom, 0.6);
    page.text(x + 1.5, staffTop + 15, String(bar.number), 6);
  }
  page.line(left + end * scale(layout), staffTop, left + end * scale(layout), bottom, 0.6);

  // Palm mute and let ring: named where a run starts, then dashes.
  for (const [key, name, at] of [
    ["palmMute", "P.M.", 7],
    ["letRing", "let ring", 7],
  ] as const) {
    beats.forEach((b, i) => {
      const on = tab.notes.some((n) => n.start === b.tick && n[key]);
      if (!on) return;
      const prev = beats[i - 1];
      const before = prev !== undefined && tab.notes.some((n) => n.start === prev.tick && n[key]);
      page.text(left + b.x * scale(layout), staffTop + at, before ? "-" : name, 6, "center");
    });
  }

  for (const n of notes) {
    const x = left + n.x * scale(layout);
    const ny = y(n.line);
    page.text(x, ny - 2.6, n.label, 7.5, "center");
    if (n.join && n.joinTo !== null) {
      page.text(left + ((n.x + n.joinTo) / 2) * scale(layout), ny + 2, n.join, 6, "center");
    }
    // Marks start just right of the fret number, clear of the string above.
    let mark = x + textWidth(n.label, 7.5) / 2 + 2;
    if (n.note.bend) {
      page.arrow(mark, ny - 1.5, 5, true);
      const amount = bendLabel(n.note.bend.semitones);
      page.text(mark + 2, ny, amount, 5.5);
      mark += 3 + textWidth(amount, 5.5);
      if (n.note.bend.release) {
        page.arrow(mark, ny + 3.5, 5, false);
        mark += 4;
      }
    }
    if (n.note.vibrato) {
      page.text(mark, ny + 4, "~~", 6);
      mark += 8;
    }
    if (n.note.tap) page.text(mark, ny + 4, "T", 6);
  }

  // The rhythm under the staff.
  for (const b of beats) {
    const x = left + b.x * scale(layout);
    const y0 = bottom - 4;
    const r = b.rhythm;
    const length = r.stem ? (r.hollow ? 6 : 12) : 0;
    if (length) page.line(x, y0, x, y0 - length, 0.7);
    for (let f = 0; f < r.flags; f++) {
      const fy = y0 - length + f * 3;
      page.line(x, fy, x + 4.5, fy + 2.5, 0.7);
    }
    if (r.dotted) page.dot(x + 3.5, y0 - length, 0.8);
    if (r.triplet) page.text(x, y0 - length - 7, "3", 5.5, "center");
  }
}

/** Layout units to points: bars are laid out at the view's width. */
function scale(layout: TabLayout): number {
  return (PAGE_W - 2 * MARGIN - GUTTER) / (layout.barWidth * BARS_PER_ROW);
}

/** The tab as the bytes of a PDF file. `title` heads the first page. */
export function tabPdf(tab: Tab, title: string): Uint8Array {
  const layout = layoutTab(tab, BARS_PER_ROW, 240);
  const height = rowHeight(layout.strings);
  const pages: Page[] = [];
  let page = new Page();
  pages.push(page);
  page.text(MARGIN, PAGE_H - MARGIN - 14, title || tab.name || "Tab", 16);
  const tuning = stringNames(tab).reverse().join(" ");
  const details = [
    `Tuning ${tuning}`,
    tab.capo ? `capo ${tab.capo}` : "",
    `${Math.round(tab.tempo.bpm)} bpm`,
    `${tab.tempo.numerator}/${tab.tempo.denominator}`,
  ].filter(Boolean);
  page.text(MARGIN, PAGE_H - MARGIN - 30, details.join("  ·  ").replace(/·/g, "-"), 8);
  let top = PAGE_H - MARGIN - HEADER;
  for (let row = 0; row < layout.rows; row++) {
    if (top - height < MARGIN) {
      page = new Page();
      pages.push(page);
      top = PAGE_H - MARGIN;
    }
    drawRow(page, tab, layout, row, top);
    top -= height;
  }
  pages.forEach((p, i) => {
    p.text(PAGE_W / 2, MARGIN / 2, `${i + 1} / ${pages.length}`, 7, "center");
  });
  return writePdf(pages.map((p) => p.ops.join("\n")));
}

/** A PDF of pages, each a content stream, with Helvetica as /F1. */
function writePdf(contents: string[]): Uint8Array {
  const objects: string[] = [];
  const add = (body: string) => {
    objects.push(body);
    return objects.length;
  };
  const catalog = add("");
  const pagesId = add("");
  const font = add(
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
  );
  const kids: number[] = [];
  for (const content of contents) {
    const stream = add(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`);
    kids.push(
      add(
        `<< /Type /Page /Parent ${pagesId} 0 R /MediaBox [0 0 ${PAGE_W} ${PAGE_H}] ` +
          `/Resources << /Font << /F1 ${font} 0 R >> >> /Contents ${stream} 0 R >>`,
      ),
    );
  }
  objects[catalog - 1] = `<< /Type /Catalog /Pages ${pagesId} 0 R >>`;
  objects[pagesId - 1] =
    `<< /Type /Pages /Kids [${kids.map((k) => `${k} 0 R`).join(" ")}] /Count ${kids.length} >>`;

  let out = "%PDF-1.4\n";
  const offsets: number[] = [];
  objects.forEach((body, i) => {
    offsets.push(out.length);
    out += `${i + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) out += `${String(offset).padStart(10, "0")} 00000 n \n`;
  out += `trailer\n<< /Size ${objects.length + 1} /Root ${catalog} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  // Every character is below 128, so a character is a byte.
  const bytes = new Uint8Array(out.length);
  for (let i = 0; i < out.length; i++) bytes[i] = out.charCodeAt(i);
  return bytes;
}
