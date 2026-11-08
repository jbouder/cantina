import { Channel, invoke } from "@tauri-apps/api/core";
import {
  type AssistantProblem,
  chainAdd,
  type EngineStatus,
  engineStatus,
  inTauri,
  projectUndo,
} from "./engine";

/** One entry of the chat thread. Mirrors `Item` in src-tauri/src/assistant/chat.rs. */
export type ChatItem =
  | { type: "user"; text: string }
  | { type: "reply"; text: string; stopped: boolean }
  | {
      type: "call";
      id: string;
      name: string;
      arguments: string;
      /** One line for the card, such as "Added Overdrive to Guitar". */
      summary: string;
      result: string;
      ok: boolean;
      /** Set when the call changed the project: the card offers Undo. */
      undo: number | null;
      undone: boolean;
    }
  | { type: "error"; problem: AssistantProblem };

/** What the shell sends while it answers. Mirrors `Event` in src-tauri. */
export type ChatEvent =
  | { type: "text"; text: string }
  | { type: "item"; item: ChatItem }
  | { type: "confirm"; id: string; question: string }
  | { type: "done" };

/** The assistant is on and has somewhere to send a message. */
export function assistantReady(status: EngineStatus | null): boolean {
  const a = status?.assistant;
  return !!a && a.enabled && a.endpoint.trim() !== "" && a.model.trim() !== "";
}

/** The thread so far, kept between launches. */
export async function assistantThread(): Promise<ChatItem[]> {
  if (inTauri) return invoke<ChatItem[]>("assistant_thread");
  return structuredClone(standIn.thread());
}

/**
 * Sends a message and calls `onEvent` with each event until `done`.
 * Rejects with an `AssistantProblem` when no request could be made.
 */
export async function assistantSend(
  text: string,
  onEvent: (event: ChatEvent) => void,
): Promise<void> {
  if (inTauri) {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke("assistant_send", { text, channel });
  }
  return standIn.send(text, onEvent);
}

/** Asks again after a failure, or for a new last reply. */
export async function assistantRetry(onEvent: (event: ChatEvent) => void): Promise<void> {
  if (inTauri) {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke("assistant_retry", { channel });
  }
  return standIn.retry(onEvent);
}

/** Stops the answer being written. What was written so far stays. */
export async function assistantStop(): Promise<void> {
  if (inTauri) return invoke("assistant_stop");
  standIn.stop();
}

/** Answers a tool that asked first. */
export async function assistantConfirm(id: string, yes: boolean): Promise<void> {
  if (inTauri) return invoke("assistant_confirm", { id, yes });
  standIn.answer(id, yes);
}

/** Forgets the thread. */
export async function assistantClear(): Promise<void> {
  if (inTauri) return invoke("assistant_clear");
  standIn.clear();
}

/** Undoes the edit behind a tool card, if nothing has changed since. */
export async function assistantUndo(id: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("assistant_undo", { id });
  return standIn.undo(id);
}

/** Opens a link from a reply in the browser. */
export async function assistantOpenUrl(url: string): Promise<void> {
  if (inTauri) return invoke("assistant_open_url", { url });
  window.open(url, "_blank", "noopener");
}

/*
 * The browser stand-in: a scripted droid. "pedal" or "drive" in a message
 * adds an Overdrive to the first track, "remove" asks first, "offline"
 * fails, and anything else gets a reply with a list and some code. The
 * thread is kept in localStorage, as the shell keeps it in a file.
 * `?chat` starts with a conversation in it for the screenshots.
 */
const STORE = "cantina.assistant.thread";

const SAMPLE: ChatItem[] = [
  { type: "user", text: "My overdrive sounds fizzy. Can you tame it?" },
  {
    type: "reply",
    text: "Fizz is usually treble the drive adds on top. Two quick fixes:\n\n1. Turn the **Tone** down to around `0.35`.\n2. Put an **EQ** after the drive and cut above 5 kHz.\n\nTrying the first one now.",
    stopped: false,
  },
  {
    type: "call",
    id: "sample-1",
    name: "set_param",
    arguments: '{"pedal":3,"param":"Tone","value":0.35}',
    summary: "Set Overdrive's Tone to 0.35 on Guitar",
    result: '{"ok":true}',
    ok: true,
    undo: 1,
    undone: false,
  },
  {
    type: "reply",
    text: "Tone recalibrated. Play a chord: if it is still bright, say so and I will add the EQ.",
    stopped: false,
  },
];

const standIn = (() => {
  let stopped = false;
  const asks = new Map<string, (yes: boolean) => void>();
  let next = 1;

  const thread = (): ChatItem[] => {
    try {
      const kept = window.localStorage.getItem(STORE);
      if (kept) return JSON.parse(kept) as ChatItem[];
    } catch {
      // A stand-in with no storage starts empty.
    }
    return new URLSearchParams(window.location.search).has("chat") ? structuredClone(SAMPLE) : [];
  };
  const save = (items: ChatItem[]) => {
    try {
      window.localStorage.setItem(STORE, JSON.stringify(items));
    } catch {
      // Not kept; the stand-in still works.
    }
  };
  const push = (item: ChatItem, onEvent: (e: ChatEvent) => void) => {
    save([...thread(), item]);
    onEvent({ type: "item", item });
  };
  const wait = (ms: number) => new Promise((r) => window.setTimeout(r, ms));

  /** Writes `text` a few characters at a time, unless stopped. */
  const stream = async (text: string, onEvent: (e: ChatEvent) => void) => {
    let written = "";
    for (const piece of text.match(/.{1,6}/gs) ?? []) {
      if (stopped) break;
      await wait(25);
      written += piece;
      onEvent({ type: "text", text: piece });
    }
    if (written) push({ type: "reply", text: written, stopped }, onEvent);
  };

  const answer = async (text: string, onEvent: (e: ChatEvent) => void) => {
    stopped = false;
    await wait(400);
    const lower = text.toLowerCase();
    if (lower.includes("offline")) {
      push(
        {
          type: "error",
          problem: {
            kind: "offline",
            message:
              "https://droids.example/v1 took too long to answer. Check your connection and try again.",
          },
        },
        onEvent,
      );
    } else if (lower.includes("remove")) {
      const id = `stand-in-${next++}`;
      const question = "Remove the track Bass, with its clips and pedals?";
      onEvent({ type: "confirm", id, question });
      const yes = await new Promise<boolean>((resolve) => asks.set(id, resolve));
      push(
        {
          type: "call",
          id,
          name: "remove_track",
          arguments: '{"track":2}',
          summary: yes ? "Would remove Bass (the stand-in keeps it)" : `Not done: ${question}`,
          result: yes ? '{"ok":true}' : '{"declined":"The player said no."}',
          ok: yes,
          undo: null,
          undone: false,
        },
        onEvent,
      );
      await stream(yes ? "Done." : "Understood. Bass stays.", onEvent);
    } else if (lower.includes("pedal") || lower.includes("drive")) {
      await stream("Adding an overdrive to your first track.", onEvent);
      const status = await engineStatus();
      const track = status.project.tracks.find((t) => t.kit === null);
      if (track && !stopped) {
        await chainAdd(track.id, "overdrive");
        push(
          {
            type: "call",
            id: `stand-in-${next++}`,
            name: "add_pedal",
            arguments: JSON.stringify({ track: track.id, kind: "overdrive" }),
            summary: `Added Overdrive to ${track.name}`,
            result: '{"ok":true}',
            ok: true,
            undo: 1,
            undone: false,
          },
          onEvent,
        );
        await stream("Signal chain recalibrated. Drive is at noon; turn it up for more.", onEvent);
      }
    } else {
      await stream(
        "Here is how I would set up a clean, punchy rhythm tone:\n\n- **Gain** low, so the amp stays clean\n- A **compressor** before it, for even strums\n- A touch of **chorus** after\n\nThen set the tempo:\n\n```\ntempo 96, 4/4\n```",
        onEvent,
      );
    }
    onEvent({ type: "done" });
  };

  return {
    thread,
    async send(text: string, onEvent: (e: ChatEvent) => void) {
      push({ type: "user", text }, onEvent);
      await answer(text, onEvent);
    },
    async retry(onEvent: (e: ChatEvent) => void) {
      const items = thread();
      while (items.length && (items.at(-1)?.type === "error" || items.at(-1)?.type === "reply")) {
        items.pop();
      }
      save(items);
      const last = [...items].reverse().find((i) => i.type === "user");
      await answer(last?.type === "user" ? last.text : "", onEvent);
    },
    stop() {
      stopped = true;
      for (const resolve of asks.values()) resolve(false);
      asks.clear();
    },
    answer(id: string, yes: boolean) {
      asks.get(id)?.(yes);
      asks.delete(id);
    },
    clear() {
      save([]);
    },
    async undo(id: string): Promise<EngineStatus> {
      const status = await projectUndo();
      save(thread().map((i) => (i.type === "call" && i.id === id ? { ...i, undone: true } : i)));
      return status;
    },
  };
})();
