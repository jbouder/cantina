import { Fragment, type ReactNode } from "react";
import { assistantOpenUrl } from "../lib/assistant";
import { type Block, type Inline, parseMarkdown } from "../lib/markdown";

/** A reply's Markdown as elements. Nothing in it is ever treated as HTML. */
export function Markdown({ text }: { text: string }) {
  return <div className="grid gap-2 text-sm leading-snug">{parseMarkdown(text).map(block)}</div>;
}

function block(b: Block, key: number): ReactNode {
  switch (b.type) {
    case "paragraph":
      return (
        <p key={key} className="m-0">
          {spans(b.inline)}
        </p>
      );
    case "heading":
      return (
        <p key={key} className="m-0 font-bold">
          {spans(b.inline)}
        </p>
      );
    case "list": {
      const List = b.ordered ? "ol" : "ul";
      return (
        <List
          key={key}
          className={`m-0 grid gap-0.5 pl-5 ${b.ordered ? "list-decimal" : "list-disc"}`}
        >
          {b.items.map((item, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: list items have no identity but their place
            <li key={i}>{spans(item)}</li>
          ))}
        </List>
      );
    }
    case "code":
      return (
        <pre
          key={key}
          className="surface-lcd m-0 overflow-x-auto whitespace-pre px-2 py-1 font-lcd text-sm"
        >
          <code className="lcd-text">{b.text}</code>
        </pre>
      );
    case "quote":
      return (
        <blockquote key={key} className="m-0 border-ink-dim border-l-2 pl-2 text-ink-dim">
          {spans(b.inline)}
        </blockquote>
      );
    case "rule":
      return <hr key={key} className="m-0 w-full border-0 border-ink-dim border-t" />;
  }
}

function spans(inline: Inline[]): ReactNode {
  return inline.map((s, i) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: spans have no identity but their place
    <Fragment key={i}>{span(s)}</Fragment>
  ));
}

function span(s: Inline): ReactNode {
  switch (s.type) {
    case "text":
      return s.text;
    case "code":
      return <code className="rounded-lcd bg-lcd-bg px-1 font-lcd text-lcd-fg">{s.text}</code>;
    case "strong":
      return <strong>{spans(s.children)}</strong>;
    case "em":
      return <em>{spans(s.children)}</em>;
    case "link":
      return /^https?:\/\//.test(s.href) ? (
        <button
          type="button"
          title={s.href}
          onClick={() => void assistantOpenUrl(s.href)}
          className="inline cursor-pointer border-0 bg-transparent p-0 text-inherit underline"
        >
          {spans(s.children)}
        </button>
      ) : (
        spans(s.children)
      );
  }
}
