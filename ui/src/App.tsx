import { useCallback, useEffect, useState } from "react";
import { Button } from "./components/Button";
import { Lcd } from "./components/Lcd";
import { Led } from "./components/Led";
import { Meter } from "./components/Meter";
import { Notice } from "./components/Notice";
import { ProjectBar } from "./components/ProjectBar";
import { Projects } from "./components/Projects";
import { Settings } from "./components/Settings";
import { Timeline } from "./components/Timeline";
import { TrackPanel } from "./components/TrackPanel";
import { Transport } from "./components/Transport";
import { Window } from "./components/Window";
import {
  dismissNotice,
  type EngineStatus,
  engineStart,
  engineStatus,
  engineStop,
  inTauri,
  type MeterFrame,
  onMeters,
  onStatus,
  SILENT,
} from "./lib/engine";

const IDLE: MeterFrame = {
  input: SILENT,
  output: SILENT,
  position: 0,
  playing: false,
  recording: false,
};

const params = inTauri ? null : new URLSearchParams(window.location.search);

export function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [frame, setFrame] = useState<MeterFrame>(IDLE);
  // In a browser `?settings` and `?projects` open the sheets, so
  // screenshots can show them.
  const [settingsOpen, setSettingsOpen] = useState(() => params?.has("settings") ?? false);
  const [projectsOpen, setProjectsOpen] = useState(() => params?.has("projects") ?? false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const closeProjects = useCallback(() => setProjectsOpen(false), []);
  const [selectedTrack, setSelectedTrack] = useState<number | null>(null);

  useEffect(() => {
    engineStatus().then(setStatus, (e) => setError(String(e)));
    const stopStatus = onStatus(setStatus);
    const stopMeters = onMeters(setFrame);
    return () => {
      stopStatus();
      stopMeters();
    };
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

  const dismiss = useCallback(() => {
    dismissNotice().then(setStatus, (e) => setError(String(e)));
  }, []);

  const running = status?.running ?? false;
  // The selected track, or the first one when the selection is gone.
  const tracks = status?.project.tracks ?? [];
  const selected = tracks.find((t) => t.id === selectedTrack) ?? tracks[0] ?? null;
  const problem = error ?? status?.error ?? null;
  // The transport flags in the frame are fresher than the last status push.
  const transport = status
    ? { ...status.transport, playing: frame.playing, recording: frame.recording }
    : null;

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
      <main className="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto p-window">
        {status?.notice ? <Notice message={status.notice} onDismiss={dismiss} /> : null}

        <section
          aria-labelledby="engine-title"
          className="grid gap-2 md:grid-cols-[auto_1fr_auto] md:items-center"
        >
          <div className="flex items-center gap-2">
            <h2 id="engine-title" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
              Engine
            </h2>
            <Led on={running} label="Engine" />
          </div>
          <div className="grid gap-1">
            <Lcd aria-live="polite" className="text-base">
              {status === null
                ? "..."
                : running
                  ? `${status.inputName ?? "?"} → ${status.outputName ?? "?"} · ${status.sampleRate / 1000} kHz · ${status.bufferFrames} frames`
                  : "engine stopped"}
            </Lcd>
            <Meter label="In" level={frame.input} />
            <Meter label="Out" level={frame.output} />
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="transport"
              onClick={toggle}
              disabled={status === null || busy}
              aria-pressed={running}
            >
              {busy ? "…" : running ? "Stop engine" : "Start engine"}
            </Button>
            <Button onClick={() => setSettingsOpen(true)}>Devices…</Button>
          </div>
        </section>

        {problem ? (
          <Lcd role="alert" error>
            {problem}
          </Lcd>
        ) : null}

        {status && transport ? (
          <section aria-labelledby="project-title" className="grid gap-3">
            <h2 id="project-title" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
              Project
            </h2>
            <ProjectBar
              project={status.project}
              onOpenProjects={() => setProjectsOpen(true)}
              onStatus={setStatus}
              onError={setError}
            />
            <Transport
              transport={transport}
              tracks={status.project.tracks}
              running={running}
              position={frame.position}
              onStatus={setStatus}
              onError={setError}
            />
            <Timeline
              tracks={status.project.tracks}
              sampleRate={status.project.sampleRate}
              tempo={status.project.tempo}
              position={frame.position}
              playing={transport.playing}
              running={running}
              selectedTrack={selected?.id ?? null}
              onSelectTrack={setSelectedTrack}
              onStatus={setStatus}
              onError={setError}
            />
            {selected ? (
              <TrackPanel
                key={selected.id}
                track={selected}
                index={status.project.tracks.indexOf(selected)}
                count={status.project.tracks.length}
                sampleRate={status.project.sampleRate}
                kinds={status.pedalKinds}
                onStatus={setStatus}
                onError={setError}
              />
            ) : null}
          </section>
        ) : null}
      </main>
      {settingsOpen ? <Settings onClose={closeSettings} onStatusChange={setStatus} /> : null}
      {projectsOpen && status ? (
        <Projects
          current={status.project.name}
          dirty={status.project.dirty}
          onClose={closeProjects}
          onStatus={setStatus}
        />
      ) : null}
    </Window>
  );
}
