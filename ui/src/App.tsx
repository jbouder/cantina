import { useCallback, useEffect, useState } from "react";
import { Button } from "./components/Button";
import { Lcd } from "./components/Lcd";
import { Led } from "./components/Led";
import { Settings } from "./components/Settings";
import { Window } from "./components/Window";
import { type EngineStatus, engineStart, engineStatus, engineStop, inTauri } from "./lib/engine";

export function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // In a browser `?settings` opens the sheet, so screenshots can show it.
  const [settingsOpen, setSettingsOpen] = useState(
    () => !inTauri && new URLSearchParams(window.location.search).has("settings"),
  );
  const closeSettings = useCallback(() => setSettingsOpen(false), []);

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
  const problem = error ?? status?.error ?? null;

  return (
    <Window
      title="Cantina"
      controls={
        <Button
          aria-label="Settings"
          aria-haspopup="dialog"
          aria-expanded={settingsOpen}
          onClick={() => setSettingsOpen(true)}
          className="px-2"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
            <path d="M8 5a3 3 0 1 0 0 6 3 3 0 0 0 0-6zm6.6 2-1.5-.3a5.3 5.3 0 0 0-.5-1.2l.9-1.3-1.7-1.7-1.3.9a5.3 5.3 0 0 0-1.2-.5L9 1.4H7l-.3 1.5a5.3 5.3 0 0 0-1.2.5l-1.3-.9-1.7 1.7.9 1.3a5.3 5.3 0 0 0-.5 1.2L1.4 7v2l1.5.3c.1.4.3.8.5 1.2l-.9 1.3 1.7 1.7 1.3-.9c.4.2.8.4 1.2.5L7 14.6h2l.3-1.5c.4-.1.8-.3 1.2-.5l1.3.9 1.7-1.7-.9-1.3c.2-.4.4-.8.5-1.2l1.5-.3V7z" />
          </svg>
        </Button>
      }
    >
      <main className="flex flex-1 items-center justify-center p-6">
        <section aria-labelledby="engine-title" className="grid w-full max-w-md gap-3">
          <header className="flex items-center justify-between">
            <h2 id="engine-title" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
              Engine
            </h2>
            <Led on={running} label="Engine" />
          </header>

          <Lcd aria-live="polite">
            {status === null
              ? "..."
              : running
                ? `${status.inputName ?? "?"} → ${status.outputName ?? "?"} · ${status.sampleRate} Hz · ${status.bufferFrames} frames`
                : "engine stopped"}
          </Lcd>

          <Button
            variant="transport"
            onClick={toggle}
            disabled={status === null || busy}
            aria-pressed={running}
            className="w-full"
          >
            {busy ? "…" : running ? "Stop engine" : "Start engine"}
          </Button>

          {problem ? (
            <Lcd role="alert" error>
              {problem}
            </Lcd>
          ) : null}
        </section>
      </main>
      {settingsOpen ? <Settings onClose={closeSettings} /> : null}
    </Window>
  );
}
