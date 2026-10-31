import { useCallback, useEffect, useState } from "react";
import { type EngineStatus, engineStart, engineStatus, engineStop } from "./lib/engine";

export function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    engineStatus().then(setStatus, (e) => setError(String(e)));
  }, []);

  const toggle = useCallback(async () => {
    if (!status || busy) return;
    setBusy(true);
    setError(null);
    try {
      setStatus(await (status.running ? engineStop() : engineStart()));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [status, busy]);

  const running = status?.running ?? false;

  return (
    <main className="flex h-full items-center justify-center p-6">
      <section
        aria-labelledby="engine-title"
        className="w-full max-w-md rounded-window bg-frame p-4 shadow-window"
      >
        <header className="mb-4 flex items-center justify-between">
          <h1
            id="engine-title"
            className="font-lcd text-lg uppercase tracking-[0.18em] text-title-text"
          >
            Cantina
          </h1>
          <span
            aria-hidden="true"
            className={`size-3 rounded-full ${running ? "bg-led-on" : "bg-led-off"}`}
          />
        </header>

        <output
          aria-live="polite"
          className="mb-4 block rounded-control bg-lcd-bg px-3 py-2 font-lcd text-lcd-fg text-xl"
        >
          {status === null
            ? "..."
            : running
              ? `${status.inputName ?? "?"} → ${status.outputName ?? "?"} · ${status.sampleRate} Hz · ${status.bufferFrames} frames`
              : "engine stopped"}
        </output>

        <button
          type="button"
          onClick={toggle}
          disabled={status === null || busy}
          aria-pressed={running}
          className="min-h-11 w-full rounded-control bg-silver px-4 font-semibold text-glyph hover:bg-silver-hi active:bg-silver-lo disabled:opacity-50"
        >
          {busy ? "…" : running ? "Stop engine" : "Start engine"}
        </button>

        {error || status?.error ? (
          <p role="alert" className="mt-3 font-lcd text-lcd-error">
            {error ?? status?.error}
          </p>
        ) : null}
      </section>
    </main>
  );
}
