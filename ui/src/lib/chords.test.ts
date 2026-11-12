import { describe, expect, it } from "vitest";
import { spellChords } from "./chords";

describe("chords spelled in a key", () => {
  it("counts numerals from the tonic as a major scale does", () => {
    expect(spellChords("i5 | bVI5 | bVII5 | i5", "E")).toBe("E5 | C5 | D5 | E5");
    expect(spellChords("I | IV | V | IV", "G")).toBe("G | C | D | C");
    expect(spellChords("I5 | I5 | bVII5 | IV5", "A")).toBe("A5 | A5 | G5 | D5");
  });

  it("makes lower case minor, unless the suffix says otherwise", () => {
    expect(spellChords("ii7 V7 | Imaj7 -", "C")).toBe("Dm7 G7 | Cmaj7 -");
    expect(spellChords("vi | iiø7 | viidim", "C")).toBe("Am | Dø7 | Bdim");
  });

  it("spells a slash bass and leaves chord names alone", () => {
    expect(spellChords("I/iii | Am | F | C/E", "C")).toBe("C/E | Am | F | C/E");
  });

  it("uses the key list's spelling for black keys", () => {
    expect(spellChords("i | bIII | bVII", "A")).toBe("Am | C | G");
    expect(spellChords("I | bIII", "F")).toBe("F | Ab");
  });
});
