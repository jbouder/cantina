import { describe, expect, it } from "vitest";
import type { Tab, TabNote } from "./engine";
import { tabPdf } from "./tabPdf";

function tab(bars: number): Tab {
  const notes: TabNote[] = [];
  for (let b = 0; b < bars; b++) {
    notes.push({ string: 6, fret: 0, start: b * 3840, length: 480, palmMute: true });
    notes.push({ string: 3, fret: 7, start: b * 3840 + 960, length: 480, bend: { semitones: 1 } });
    notes.push({ string: 1, fret: 12, start: b * 3840 + 1920, length: 960, harmonic: true });
  }
  return {
    name: "Riff",
    instrument: "guitar",
    tuning: [40, 45, 50, 55, 59, 64],
    capo: 2,
    tempo: { bpm: 120, numerator: 4, denominator: 4 },
    notes,
  };
}

const text = (bytes: Uint8Array) => String.fromCharCode(...bytes);

describe("the tab PDF", () => {
  it("is a well-formed PDF whose cross-reference points at every object", () => {
    const pdf = text(tabPdf(tab(2), "Riff"));
    expect(pdf.startsWith("%PDF-1.4\n")).toBe(true);
    expect(pdf.trimEnd().endsWith("%%EOF")).toBe(true);
    const xref = Number(pdf.match(/startxref\n(\d+)/)?.[1]);
    expect(pdf.slice(xref, xref + 4)).toBe("xref");
    const offsets = [...pdf.slice(xref).matchAll(/^(\d{10}) 00000 n $/gm)].map((m) => Number(m[1]));
    offsets.forEach((offset, i) => {
      expect(pdf.slice(offset).startsWith(`${i + 1} 0 obj`)).toBe(true);
    });
    // Stream lengths are right.
    for (const m of pdf.matchAll(/<< \/Length (\d+) >>\nstream\n/g)) {
      const start = (m.index ?? 0) + m[0].length;
      expect(pdf.slice(start + Number(m[1]), start + Number(m[1]) + 10)).toBe("\nendstream");
    }
  });

  it("draws the page view: title, tuning, frets, marks and bar numbers", () => {
    const pdf = text(tabPdf(tab(2), "Riff"));
    expect(pdf).toContain("(Riff) Tj");
    expect(pdf).toContain("Tuning F# B E A C# f#");
    expect(pdf).toContain("capo 2");
    expect(pdf).toContain("(<12>) Tj");
    expect(pdf).toContain("(P.M.) Tj");
    expect(pdf).toContain("(\\275) Tj");
    expect(pdf).toContain("(2) Tj");
    expect(pdf.match(/\/Count (\d+)/)?.[1]).toBe("1");
  });

  it("runs onto more pages for a long tab, four bars a row", () => {
    const pdf = text(tabPdf(tab(64), "Long"));
    const pages = Number(pdf.match(/\/Count (\d+)/)?.[1]);
    expect(pages).toBeGreaterThan(1);
    expect(pdf).toContain(`(1 / ${pages}) Tj`);
  });
});
