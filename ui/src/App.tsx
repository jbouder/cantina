import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "./components/Button";
import { Designer } from "./components/Designer";
import { Divider } from "./components/Divider";
import { Editor, isEditorTab } from "./components/Editor";
import { EngineControl } from "./components/EngineControl";
import { Lcd } from "./components/Lcd";
import { Meter } from "./components/Meter";
import { Notice } from "./components/Notice";
import { PedalPicker } from "./components/PedalPicker";
import { ProjectBar } from "./components/ProjectBar";
import { Projects } from "./components/Projects";
import { Recovery } from "./components/Recovery";
import { Settings } from "./components/Settings";
import { Timeline } from "./components/Timeline";
import { Transport } from "./components/Transport";
import { Window } from "./components/Window";
import {
  designerNew,
  dismissNotice,
  type EngineStatus,
  engineStart,
  engineStatus,
  engineStop,
  inTauri,
  type MeterFrame,
  ms,
  onFileDrop,
  onMeters,
  onStatus,
  pedalImport,
  projectRedo,
  projectUndo,
  SILENT,
  transportPlay,
  transportRecord,
  transportStop,
} from "./lib/engine";
import { readoutsByPedal } from "./lib/readouts";

const IDLE: MeterFrame = {
  input: SILENT,
  output: SILENT,
  tracks: [],
  position: 0,
  playing: false,
  recording: false,
  readouts: [],
};

const params = inTauri ? null : new URLSearchParams(window.location.search);

const SPLIT_KEY = "cantina.split";
/** Room for the strip, the ruler, two tracks and the master at the default window. */
const DEFAULT_SPLIT = 352;
const MIN_PANE = 160;

function loadSplit(): number {
  try {
    const saved = Number(window.sessionStorage.getItem(SPLIT_KEY));
    if (Number.isFinite(saved) && saved >= MIN_PANE) return saved;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return DEFAULT_SPLIT;
}

export function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [frame, setFrame] = useState<MeterFrame>(IDLE);
  // In a browser `?settings`, `?projects` and `?picker` open the
  // sheets, so screenshots can show them.
  const [settingsOpen, setSettingsOpen] = useState(() => params?.has("settings") ?? false);
  const [projectsOpen, setProjectsOpen] = useState(() => params?.has("projects") ?? false);
  const [pickerOpen, setPickerOpen] = useState(() => params?.has("picker") ?? false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const closeProjects = useCallback(() => setProjectsOpen(false), []);
  const closePicker = useCallback(() => setPickerOpen(false), []);
  // In a browser `?drums` adds a drum track to the stand-in and selects it.
  const [selectedTrack, setSelectedTrack] = useState<number | null>(() =>
    params?.has("drums") ? 10 : null,
  );
  // The designed pedal open in the designer. In a browser `?design=<stock>`
  // puts one on the guitar track and opens it.
  const [designing, setDesigning] = useState<number | null>(() =>
    params?.has("design") ? 19 : null,
  );
  const closeDesigner = useCallback(() => setDesigning(null), []);
  // Where a dropped pedal file goes: the selected track's chain.
  const dropTarget = useRef<number | null>(null);

  // The arrangement's height; the divider under it moves it and the
  // session remembers it. In a browser `?tab=<beats|pedalboard|clips>`
  // opens the editor on a tab, so screenshots can show each.
  const [split, setSplit] = useState(() => loadSplit());
  const body = useRef<HTMLDivElement>(null);
  const [bodyHeight, setBodyHeight] = useState(720);
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const measure = () => setBodyHeight(el.clientHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const changeSplit = useCallback((value: number) => {
    setSplit(value);
    try {
      window.sessionStorage.setItem(SPLIT_KEY, String(Math.round(value)));
    } catch {
      // Not remembered this time; the split still applies.
    }
  }, []);
  const tabParam = params?.get("tab");
  const initialTab = isEditorTab(tabParam) ? tabParam : undefined;

  useEffect(() => {
    engineStatus().then(setStatus, (e) => setError(String(e)));
    const stopStatus = onStatus(setStatus);
    const stopMeters = onMeters(setFrame);
    return () => {
      stopStatus();
      stopMeters();
    };
  }, []);

  // A .pedal.json dropped on the window is imported into the library
  // and added to the selected track's chain.
  useEffect(
    () =>
      onFileDrop((paths) => {
        const track = dropTarget.current;
        for (const path of paths.filter((p) => p.endsWith(".pedal.json"))) {
          pedalImport(path, track ?? undefined).then(setStatus, (e) => setError(String(e)));
        }
      }),
    [],
  );

  // Cmd-Z and Shift-Cmd-Z anywhere but inside a text field, where the
  // field's own undo applies. Space plays or stops and R records, except
  // on a control that uses those keys itself.
  const keys = useRef({ status, frame });
  keys.current = { status, frame };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName;
      const typing =
        (tag === "INPUT" && (target as HTMLInputElement).type === "text") ||
        tag === "TEXTAREA" ||
        target?.isContentEditable;
      const report = (action: Promise<EngineStatus>) =>
        action.then(setStatus, (err) => setError(String(err)));
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z" && !e.altKey) {
        if (typing) return;
        e.preventDefault();
        report(e.shiftKey ? projectRedo() : projectUndo());
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey || typing) return;
      if (tag === "INPUT" || tag === "SELECT" || tag === "BUTTON") return;
      const { status: s, frame: f } = keys.current;
      if (!s?.running) return;
      if (e.key === " ") {
        e.preventDefault();
        const rolling = f.playing || f.recording;
        const hasClips =
          s.audition !== null ||
          s.project.tracks.some((t) => t.clips.length > 0 || t.patternClips.length > 0);
        if (rolling) report(transportStop());
        else if (hasClips) report(transportPlay());
      } else if (e.key === "r" || e.key === "R") {
        if (f.playing || f.recording) return;
        if (!s.project.tracks.some((t) => t.armed && !t.kit)) return;
        e.preventDefault();
        report(transportRecord());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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

  // Opens the designer on a pedal, or starts a new design at the end
  // of the selected track's chain and opens that.
  const design = useCallback((pedal: number | null, track: number) => {
    if (pedal !== null) {
      setDesigning(pedal);
      return;
    }
    designerNew(track).then(
      (next) => {
        setStatus(next);
        const chain = next.project.tracks.find((t) => t.id === track)?.chain ?? [];
        setDesigning(chain[chain.length - 1]?.id ?? null);
      },
      (e) => setError(String(e)),
    );
  }, []);

  const running = status?.running ?? false;
  // The selected track, or the first one when the selection is gone.
  const tracks = status?.project.tracks ?? [];
  const readouts = useMemo(() => readoutsByPedal(tracks, frame.readouts), [tracks, frame.readouts]);
  const selected = tracks.find((t) => t.id === selectedTrack) ?? tracks[0] ?? null;
  dropTarget.current = selected?.id ?? null;
  const designed =
    designing === null
      ? null
      : (tracks.flatMap((t) => t.chain).find((p) => p.id === designing) ?? null);
  const problem = error ?? status?.error ?? null;
  // The split never leaves the editor less than a pane's worth.
  const arrangeHeight = Math.min(split, Math.max(MIN_PANE, bodyHeight - MIN_PANE));
  // The transport flags in the frame are fresher than the last status push.
  const transport = status
    ? { ...status.transport, playing: frame.playing, recording: frame.recording }
    : null;

  return (
    <Window
      title="Cantina"
      start={
        status ? (
          <ProjectBar
            project={status.project}
            onOpenProjects={() => setProjectsOpen(true)}
            onStatus={setStatus}
            onError={setError}
          />
        ) : null
      }
      end={
        <>
          <EngineControl status={status} busy={busy} onToggle={toggle} />
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
        </>
      }
      meters={
        <>
          <Meter label="In" level={frame.input} />
          <Meter label="Out" level={frame.output} />
        </>
      }
    >
      <main className="flex min-h-0 flex-1 flex-col gap-3 p-window">
        {status?.notice ? <Notice message={status.notice} onDismiss={dismiss} /> : null}
        {status?.project.recovery ? (
          <Recovery recovery={status.project.recovery} onStatus={setStatus} onError={setError} />
        ) : null}
        {status?.latency.warn ? (
          <Lcd role="status" error className="text-base">
            Plugins delay playback by {ms(status.latency.compensationFrames, status.sampleRate)},
            above the {status.latency.warnMs} ms you set. Live monitoring is not delayed.
          </Lcd>
        ) : null}
        {problem ? (
          <Lcd role="alert" error>
            {problem}
          </Lcd>
        ) : null}

        {status && transport ? (
          <div
            ref={body}
            className="grid min-h-0 flex-1"
            style={{ gridTemplateRows: `${arrangeHeight}px auto minmax(0, 1fr)` }}
          >
            <div className="min-h-0">
              <Timeline
                tracks={status.project.tracks}
                patterns={status.project.patterns}
                sampleRate={status.project.sampleRate}
                tempo={status.project.tempo}
                position={frame.position}
                playing={transport.playing}
                running={running}
                selectedTrack={selected?.id ?? null}
                hasKits={status.kits.length > 0}
                levels={frame.tracks}
                master={status.project.master}
                masterLevel={frame.output}
                strip={(controls) => (
                  <Transport
                    transport={transport}
                    tracks={status.project.tracks}
                    tempo={status.project.tempo}
                    running={running}
                    position={frame.position}
                    auditioning={status.audition !== null}
                    timeline={controls}
                    onStatus={setStatus}
                    onError={setError}
                  />
                )}
                onSelectTrack={setSelectedTrack}
                onStatus={setStatus}
                onError={setError}
              />
            </div>
            <Divider
              value={arrangeHeight}
              min={MIN_PANE}
              max={Math.max(MIN_PANE, bodyHeight - MIN_PANE)}
              onChange={changeSplit}
            />
            {selected ? (
              <Editor
                key={selected.id}
                track={selected}
                index={status.project.tracks.indexOf(selected)}
                count={status.project.tracks.length}
                sampleRate={status.project.sampleRate}
                kinds={status.pedalKinds}
                patterns={status.project.patterns}
                kits={status.kits}
                audition={status.audition}
                tempo={status.project.tempo}
                position={frame.position}
                playing={transport.playing}
                readouts={readouts}
                irs={status.irs}
                initialTab={initialTab}
                onAdd={() => setPickerOpen(true)}
                onDesign={(pedal) => design(pedal, selected.id)}
                onStatus={setStatus}
                onError={setError}
              />
            ) : (
              <div />
            )}
          </div>
        ) : null}
      </main>
      {settingsOpen && status ? (
        <Settings
          status={status}
          busy={busy}
          onToggleEngine={toggle}
          onClose={closeSettings}
          onStatusChange={setStatus}
        />
      ) : null}
      {projectsOpen && status ? (
        <Projects
          current={status.project.name}
          dirty={status.project.dirty}
          onClose={closeProjects}
          onStatus={setStatus}
        />
      ) : null}
      {designed && status ? (
        <Designer
          key={designed.id}
          pedal={designed}
          kinds={status.pedalKinds}
          onClose={closeDesigner}
          onStatus={setStatus}
          onError={setError}
        />
      ) : null}
      {pickerOpen && status && selected ? (
        <PedalPicker
          track={selected.id}
          trackName={selected.name}
          kinds={status.pedalKinds}
          plugins={status.plugins}
          onClose={closePicker}
          onDesign={() => design(null, selected.id)}
          onStatus={setStatus}
        />
      ) : null}
    </Window>
  );
}
