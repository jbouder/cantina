import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown } from "./markdown";

describe("markdown", () => {
  it("splits paragraphs, headings, lists, code, quotes and rules", () => {
    const blocks = parseMarkdown(
      [
        "## Your chain",
        "A gate goes **before** the drive.",
        "Keep it tight.",
        "",
        "- Gate at `-50 dB`",
        "- Overdrive",
        "  with the tone at noon",
        "1. One",
        "2) Two",
        "> Signal chain recalibrated.",
        "---",
        "```json",
        '{ "bpm": 90 }',
        "```",
      ].join("\n"),
    );
    expect(blocks.map((b) => b.type)).toEqual([
      "heading",
      "paragraph",
      "list",
      "list",
      "quote",
      "rule",
      "code",
    ]);
    expect(blocks[0]).toMatchObject({ level: 2 });
    expect(blocks[1]).toMatchObject({
      inline: [
        { type: "text", text: "A gate goes " },
        { type: "strong", children: [{ type: "text", text: "before" }] },
        { type: "text", text: " the drive. Keep it tight." },
      ],
    });
    expect(blocks[2]).toMatchObject({ ordered: false });
    const bullets = blocks[2];
    if (bullets?.type === "list") {
      expect(bullets.items).toHaveLength(2);
      expect(bullets.items[1]).toEqual([{ type: "text", text: "Overdrive with the tone at noon" }]);
    }
    expect(blocks[3]).toMatchObject({ ordered: true });
    expect(blocks[6]).toEqual({ type: "code", lang: "json", text: '{ "bpm": 90 }' });
  });

  it("runs an unclosed fence to the end, as a reply does while it streams", () => {
    expect(parseMarkdown("Here:\n```\nline one\nline two")).toEqual([
      { type: "paragraph", inline: [{ type: "text", text: "Here:" }] },
      { type: "code", lang: "", text: "line one\nline two" },
    ]);
  });

  it("finds code, emphasis and links inline, and leaves HTML as text", () => {
    expect(parseInline("see [the guide](https://x.dev/g) and *this* `<b>`")).toEqual([
      { type: "text", text: "see " },
      { type: "link", href: "https://x.dev/g", children: [{ type: "text", text: "the guide" }] },
      { type: "text", text: " and " },
      { type: "em", children: [{ type: "text", text: "this" }] },
      { type: "text", text: " " },
      { type: "code", text: "<b>" },
    ]);
    expect(parseInline("<script>alert(1)</script>")).toEqual([
      { type: "text", text: "<script>alert(1)</script>" },
    ]);
    expect(parseInline("2 * 3 * 4")).toEqual([{ type: "text", text: "2 * 3 * 4" }]);
  });
});
