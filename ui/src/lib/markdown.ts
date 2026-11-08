/**
 * The Markdown a model writes, parsed into blocks and inline spans for
 * React to render as elements: no HTML is ever injected, so nothing a
 * reply contains can run. Covers what chat replies use: paragraphs,
 * headings, lists, fenced code, quotes, rules, and inline code, bold,
 * italic and links. Anything else reads as plain text.
 */

export type Inline =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong"; children: Inline[] }
  | { type: "em"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] };

export type Block =
  | { type: "paragraph"; inline: Inline[] }
  | { type: "heading"; level: number; inline: Inline[] }
  | { type: "list"; ordered: boolean; items: Inline[][] }
  | { type: "code"; lang: string; text: string }
  | { type: "quote"; inline: Inline[] }
  | { type: "rule" };

const FENCE = /^\s*(```|~~~)\s*([\w+-]*)\s*$/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s*>\s?(.*)$/;

/** Splits Markdown into blocks. An unclosed code fence runs to the end, as while streaming. */
export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const at = (n: number): string => lines[n] ?? "";
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = at(i);
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (
        i < lines.length &&
        !at(i)
          .trim()
          .startsWith(fence[1] ?? "```")
      )
        body.push(at(i++));
      i++;
      blocks.push({ type: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }
    if (!line.trim()) {
      i++;
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({
        type: "heading",
        level: (heading[1] ?? "#").length,
        inline: parseInline(heading[2] ?? ""),
      });
      i++;
      continue;
    }
    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(at(i))) body.push(QUOTE.exec(at(i++))?.[1] ?? "");
      blocks.push({ type: "quote", inline: parseInline(body.join(" ")) });
      continue;
    }
    const ordered = NUMBERED.test(line);
    if (ordered || BULLET.test(line)) {
      const marker = ordered ? NUMBERED : BULLET;
      const items: string[] = [];
      while (i < lines.length) {
        const item = marker.exec(at(i));
        if (item) {
          items.push(item[1] ?? "");
        } else if (at(i).trim() && /^\s{2,}/.test(at(i)) && items.length > 0) {
          items.push(`${items.pop() ?? ""} ${at(i).trim()}`);
        } else {
          break;
        }
        i++;
      }
      blocks.push({ type: "list", ordered, items: items.map(parseInline) });
      continue;
    }
    const body: string[] = [];
    while (
      i < lines.length &&
      at(i).trim() &&
      !FENCE.test(at(i)) &&
      !HEADING.test(at(i)) &&
      !BULLET.test(at(i)) &&
      !NUMBERED.test(at(i)) &&
      !QUOTE.test(at(i))
    ) {
      body.push(at(i++).trim());
    }
    blocks.push({ type: "paragraph", inline: parseInline(body.join(" ")) });
  }
  return blocks;
}

const INLINE =
  /`([^`]+)`|\*\*(.+?)\*\*|__(.+?)__|\*([^*\s][^*]*?)\*|_([^_\s][^_]*?)_|\[([^\]]+)\]\(([^)\s]+)\)/;

/** Splits a line into inline spans. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let rest = text;
  while (rest) {
    const m = INLINE.exec(rest);
    if (!m) {
      out.push({ type: "text", text: rest });
      break;
    }
    if (m.index > 0) out.push({ type: "text", text: rest.slice(0, m.index) });
    if (m[1] !== undefined) out.push({ type: "code", text: m[1] });
    else if (m[2] !== undefined || m[3] !== undefined)
      out.push({ type: "strong", children: parseInline(m[2] ?? m[3] ?? "") });
    else if (m[4] !== undefined || m[5] !== undefined)
      out.push({ type: "em", children: parseInline(m[4] ?? m[5] ?? "") });
    else out.push({ type: "link", href: m[7] ?? "", children: parseInline(m[6] ?? "") });
    rest = rest.slice(m.index + m[0].length);
  }
  return out;
}
