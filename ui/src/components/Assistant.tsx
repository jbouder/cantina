import { useCallback, useEffect, useRef, useState } from "react";
import {
  assistantClear,
  assistantConfirm,
  assistantRetry,
  assistantSend,
  assistantStop,
  assistantThread,
  assistantUndo,
  type ChatEvent,
  type ChatItem,
} from "../lib/assistant";
import { chirp } from "../lib/chirps";
import { type AssistantStatus, asProblem, type EngineStatus } from "../lib/engine";
import { Button } from "./Button";
import { Droid, type DroidState } from "./Droid";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { Markdown } from "./Markdown";

type Props = {
  assistant: AssistantStatus;
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
};

type Pending = { id: string; question: string };

/**
 * The assistant's chat panel, docked at the right of the window and
 * dressed as Tapster's console: the droid with its readout and status
 * lights, the thread, the reply being written, tool cards with Undo,
 * questions a tool asks first, and the message field with Send and Stop.
 */
export function Assistant({ assistant, onClose, onStatus }: Props) {
  const { persona, sounds } = assistant;
  const beeps = useRef(sounds);
  beeps.current = sounds;
  const failed = useRef(false);
  const [items, setItems] = useState<ChatItem[]>([]);
  const [writing, setWriting] = useState("");
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    assistantThread().then(setItems, () => setItems([]));
    field.current?.focus();
  }, []);

  // Keep the newest at the bottom in view as the reply streams.
  const length = items.length + writing.length + (pending ? 1 : 0);
  useEffect(() => {
    void length;
    const s = scroller.current;
    if (s) s.scrollTop = s.scrollHeight;
  }, [length]);

  const onEvent = useCallback((event: ChatEvent) => {
    switch (event.type) {
      case "text":
        setWriting((w) => w + event.text);
        break;
      case "item":
        setWriting("");
        setPending(null);
        setItems((list) => [...list, event.item]);
        if (event.item.type === "error") {
          failed.current = true;
          if (beeps.current) chirp("error");
        }
        break;
      case "confirm":
        setPending({ id: event.id, question: event.question });
        break;
      case "done":
        setWriting("");
        setPending(null);
        setBusy(false);
        if (beeps.current && !failed.current) chirp("reply");
        break;
    }
  }, []);

  const run = (request: Promise<void>) => {
    setBusy(true);
    setNote(null);
    failed.current = false;
    request.catch((e) => {
      if (beeps.current) chirp("error");
      setBusy(false);
      setWriting("");
      setItems((list) => [...list, { type: "error", problem: asProblem(e) }]);
    });
  };
  const send = () => {
    const text = draft.trim();
    if (!text || busy) return;
    setDraft("");
    run(assistantSend(text, onEvent));
  };
  const retry = () => {
    // Drop what the shell drops: the failures and the last reply.
    setItems((list) => {
      const kept = [...list];
      while (kept.length && ["error", "reply"].includes(kept.at(-1)?.type ?? "")) kept.pop();
      return kept;
    });
    run(assistantRetry(onEvent));
  };
  const clear = () => {
    assistantClear().then(
      () => setItems([]),
      (e) => setNote(String(e)),
    );
  };
  const undo = (id: string) => {
    assistantUndo(id).then(
      (status) => {
        onStatus(status);
        setItems((list) =>
          list.map((i) => (i.type === "call" && i.id === id ? { ...i, undone: true } : i)),
        );
      },
      (e) => setNote(String(e)),
    );
  };
  const answer = (yes: boolean) => {
    if (!pending) return;
    assistantConfirm(pending.id, yes);
    setPending(null);
  };
  const lastIsError = items.at(-1)?.type === "error";
  const state: DroidState = busy
    ? writing
      ? "speaking"
      : "thinking"
    : lastIsError
      ? "error"
      : "idle";
  const name = persona ? "Tapster" : "Assistant";
  const readout = persona
    ? { idle: "On duty", thinking: "Processing…", speaking: "Transmitting", error: "Fault logged" }
    : { idle: "Ready", thinking: "Thinking…", speaking: "Writing…", error: "Error" };

  return (
    <aside
      aria-label={name}
      className="surface-panel mr-window mb-window flex w-[22rem] shrink-0 flex-col overflow-hidden"
    >
      <header className="surface-titlebar flex min-h-[30px] items-center gap-1 py-1 pr-1 pl-3">
        <h2 className="title-text m-0 flex-1 font-normal text-[1.05rem]">{name}</h2>
        <Button onClick={clear} disabled={busy || items.length === 0}>
          Clear
        </Button>
        <Button onClick={onClose} aria-label="Close the assistant" className="px-2">
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </Button>
      </header>
      <div className="flex items-center gap-2 bg-sheet-bg px-3 pt-3">
        <Droid state={state} size={44} className="text-sheet-ink" />
        <Lcd aria-live="polite" error={state === "error"} className="min-w-0 flex-1 text-base">
          {name.toUpperCase()} · {readout[state]}
        </Lcd>
        <fieldset className="m-0 grid gap-1 border-0 p-0" aria-label="Status lights">
          <Led on label="Link" />
          <Led on={busy} label="Busy" />
          <Led on={state === "error"} tone="record" label="Fault" />
        </fieldset>
      </div>
      <div
        ref={scroller}
        role="log"
        aria-live="polite"
        aria-busy={busy}
        className="grid min-h-0 flex-1 content-start gap-3 overflow-y-auto bg-sheet-bg p-3 text-sheet-ink"
      >
        {items.length === 0 && !busy ? (
          <div className="grid justify-items-center gap-2 py-4 text-center">
            <Droid size={96} className="text-sheet-ink" />
            <p className="m-0 text-sm">
              {persona
                ? "Tapster, house droid, on duty. What can I pour you?"
                : "What can I help with?"}
            </p>
            <p className="m-0 text-ink-dim text-sm">
              Ask about a setting, a sound or a problem, or ask for a change: "add a gate before the
              drive", "set the tempo to 96", "write a 6/8 blues shuffle".
            </p>
          </div>
        ) : null}
        {items.map((item, i) => (
          <Entry
            // biome-ignore lint/suspicious/noArrayIndexKey: the thread only grows at the end
            key={i}
            item={item}
            onUndo={undo}
            onRetry={lastIsError && i === items.length - 1 && !busy ? retry : undefined}
          />
        ))}
        {writing ? (
          <div className="grid gap-1">
            <Markdown text={writing} />
          </div>
        ) : null}
        {busy && !writing && !pending ? (
          <p className="m-0 text-ink-dim text-sm" role="status" aria-label="Thinking">
            …
          </p>
        ) : null}
        {pending ? (
          <div role="alertdialog" aria-label="Confirm" className="surface-lcd grid gap-2 p-2">
            <span className="lcd-text font-lcd text-base">{pending.question}</span>
            <div className="flex justify-end gap-1">
              <Button onClick={() => answer(false)}>No</Button>
              <Button variant="transport" onClick={() => answer(true)}>
                Yes
              </Button>
            </div>
          </div>
        ) : null}
        {note ? (
          <Lcd role="status" error className="text-base">
            {note}
          </Lcd>
        ) : null}
      </div>
      <form
        className="flex items-end gap-1 bg-sheet-bg p-2"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <label htmlFor="assistant-message" className="sr-only">
          Message
        </label>
        <textarea
          id="assistant-message"
          ref={field}
          rows={2}
          value={draft}
          placeholder={persona ? "Ask Tapster…" : "Ask the assistant…"}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
          className="surface-lcd min-h-8 min-w-0 flex-1 resize-none px-2 py-1 font-ui text-sm"
        />
        {busy ? (
          <Button variant="transport" onClick={() => void assistantStop()}>
            Stop
          </Button>
        ) : (
          <Button type="submit" variant="transport" disabled={!draft.trim()}>
            Send
          </Button>
        )}
      </form>
    </aside>
  );
}

function Entry({
  item,
  onUndo,
  onRetry,
}: {
  item: ChatItem;
  onUndo: (id: string) => void;
  onRetry?: (() => void) | undefined;
}) {
  switch (item.type) {
    case "user":
      return (
        <p className="m-0 ml-8 whitespace-pre-wrap rounded-control bg-list-bg px-3 py-2 text-list-fg text-sm">
          {item.text}
        </p>
      );
    case "reply":
      return (
        <div className="grid gap-1">
          <Markdown text={item.text} />
          {item.stopped ? <small className="text-ink-dim text-xs">Stopped</small> : null}
        </div>
      );
    case "call":
      return (
        <div className="surface-lcd flex items-center gap-2 px-2 py-1">
          <Led on={item.ok && !item.undone} label={item.ok ? "Done" : "Not done"} />
          <span
            className={`lcd-text min-w-0 flex-1 font-lcd text-base ${item.undone ? "line-through" : ""}`}
          >
            {item.summary}
          </span>
          {item.undo !== null && item.ok ? (
            item.undone ? (
              <span className="lcd-text font-lcd text-sm">Undone</span>
            ) : (
              <button
                type="button"
                onClick={() => onUndo(item.id)}
                className="lcd-text cursor-pointer border-0 bg-transparent p-0 font-lcd text-sm underline"
              >
                Undo
              </button>
            )
          ) : null}
        </div>
      );
    case "error":
      return (
        <div className="grid gap-1">
          <Lcd role="alert" error className="text-base">
            {item.problem.message}
          </Lcd>
          {onRetry ? (
            <div className="flex justify-end">
              <Button onClick={onRetry}>Retry</Button>
            </div>
          ) : null}
        </div>
      );
  }
}
