import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Assistant } from "./components/Assistant";
import { Button } from "./components/Button";
import { Designer } from "./components/Designer";
import { Divider } from "./components/Divider";
import { Droid } from "./components/Droid";
import { Editor, isEditorTab } from "./components/Editor";
import { EngineControl } from "./components/EngineControl";
import { ExportSheet } from "./components/ExportSheet";
import { JamView } from "./components/JamView";
import { Lcd } from "./components/Lcd";
import { Meter } from "./components/Meter";
import { Notice } from "./components/Notice";
import { PedalPicker } from "./components/PedalPicker";
import { Performance } from "./components/Performance";
import { ProjectBar } from "./components/ProjectBar";
import { ProjectName, type ProjectNameMode } from "./components/ProjectName";
import { Projects } from "./components/Projects";
import { Recovery } from "./components/Recovery";
import { Settings } from "./components/Settings";
import { Shortcuts } from "./components/Shortcuts";
import { StageView } from "./components/StageView";
import { StartDialog } from "./components/StartDialog";
import { Timeline, type TimelineEdit, type TimelineEditState } from "./components/Timeline";
import { Transport, transportNote } from "./components/Transport";
import { UpdateBanner } from "./components/UpdateBanner";
import { Window } from "./components/Window";
import {
  type ActionId,
  actionForKey,
  type Bindings,
  editTitle,
  isTyping,
  menuStates,
} from "./lib/actions";
import { assistantReady } from "./lib/assistant";
import {
  audioImport,
  brokenOf,
  chainBypass,
  chooseAudioPaths,
  chooseImportPaths,
  chooseKitDir,
  choosePatternImportPaths,
  countInSet,
  designerNew,
  dismissNotice,
  type EngineStatus,
  engineStart,
  engineStatus,
  engineStop,
  inTauri,
  jamClose,
  jamOpen,
  jamPlay,
  jamStop,
  kitImport,
  loopSet,
  type MeterFrame,
  menuState,
  metronomeSet,
  ms,
  onFileDrop,
  onMenuAction,
  onMeters,
  onOpenRecent,
  onStatus,
  openLink,
  patternImport,
  pedalImport,
  projectOpen,
  projectRedo,
  projectSave,
  projectUndo,
  reveal,
  SILENT,
  stripMute,
  stripSolo,
  trackAdd,
  trackAddDrums,
  trackArm,
  trackRemove,
  transportLocate,
  transportPlay,
  transportRecord,
  transportStop,
  type UpdateInfo,
  updateCheck,
} from "./lib/engine";
import { readoutsByPedal } from "./lib/readouts";
import { openStageWindow } from "./lib/stage";
import { samplesPerTick } from "./lib/timeline";
import { SKINS, type SkinId, useSkin } from "./skins";

const IDLE: MeterFrame = {
  input: SILENT,
  output: SILENT,
  tracks: [],
  position: 0,
  playing: false,
  recording: false,
  countIn: 0,
  readouts: [],
};

const params = inTauri ? null : new URLSearchParams(window.location.search);

const SPLIT_KEY = "cantina.split";
const METERS_KEY = "cantina.meters";
/** Room for the strip, the ruler, two tracks and the master at the default window. */
const DEFAULT_SPLIT = 352;
const MIN_PANE = 160;

function loadSplit(): number {
  // `?split=200` sets the arrangement's height, for screenshots.
  const asked = Number(params?.get("split"));
  if (Number.isFinite(asked) && asked >= MIN_PANE) return asked;
  try {
    const saved = Number(window.sessionStorage.getItem(SPLIT_KEY));
    if (Number.isFinite(saved) && saved >= MIN_PANE) return saved;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return DEFAULT_SPLIT;
}

/** Four bars from the start of the bar at `seconds`, in samples. */
function barsFrom(status: EngineStatus, seconds: number): { start: number; end: number } {
  const { bpm, numerator, denominator } = status.project.tempo;
  const bar = numerator * (60 / bpm) * (4 / denominator) * status.project.sampleRate;
  const start = Math.floor((seconds * status.project.sampleRate) / bar) * bar;
  return { start: Math.round(start), end: Math.round(start + 4 * bar) };
}

/** The end of the song: the end of the last clip on any track, in samples. */
function songEnd(status: EngineStatus): number {
  const spt = samplesPerTick(status.project.tempo, status.project.sampleRate);
  let end = 0;
  for (const t of status.project.tracks) {
    for (const c of t.clips) end = Math.max(end, c.start + c.length);
    for (const c of t.patternClips) end = Math.max(end, (c.start + c.length) * spt);
  }
  return Math.round(end);
}

export function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [busy, setBusy] = useState(false);
  // A newer release, found at launch or from Settings, until dismissed.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  useEffect(() => {
    // Development builds are not releases: nothing to update them to.
    if (import.meta.env.DEV && inTauri) return;
    updateCheck().then(setUpdate, () => {});
  }, []);
  const [error, setError] = useState<string | null>(null);
  const [frame, setFrame] = useState<MeterFrame>(IDLE);
  // In a browser `?settings`, `?projects` and `?picker` open the
  // sheets, so screenshots can show them.
  const [settingsOpen, setSettingsOpen] = useState(() => params?.has("settings") ?? false);
  // `?chat` opens the assistant's panel (with `?assistant` connected).
  const [assistantOpen, setAssistantOpen] = useState(() => params?.has("chat") ?? false);
  // `?settings=assistant` (or `instrument`) opens Settings scrolled to that section.
  const [settingsSection, setSettingsSection] = useState<"assistant" | "instrument" | null>(() => {
    const asked = params?.get("settings");
    return asked === "assistant" || asked === "instrument" ? asked : null;
  });
  const [projectsOpen, setProjectsOpen] = useState(() => params?.has("projects") ?? false);
  const [exportOpen, setExportOpen] = useState(() => params?.has("export") ?? false);
  const [pickerOpen, setPickerOpen] = useState(() => params?.has("picker") ?? false);
  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
    setSettingsSection(null);
    setCheckUpdates(false);
  }, []);
  // In a browser `?stage` opens the stage, `?perform` performance mode
  // (with `?board` its pedalboard), and `?scene=<id>` picks the scene.
  const [stageOpen, setStageOpen] = useState(() => params?.has("stage") ?? false);
  const closeStage = useCallback(() => setStageOpen(false), []);
  const [performing, setPerforming] = useState(() => params?.has("perform") ?? false);
  // Jam mode: the jam is heard in place of the project. In a browser
  // `?jam` opens it.
  const [jamming, setJamming] = useState(() => params?.has("jam") ?? false);
  const perform = useCallback(() => {
    setStageOpen(false);
    setPerforming(true);
  }, []);
  const openJam = useCallback(() => {
    setStageOpen(false);
    setPerforming(false);
    jamOpen().then(
      (s) => {
        setStatus(s);
        setJamming(true);
      },
      (e) => setError(String(e)),
    );
  }, []);
  const leaveJam = useCallback(() => {
    jamClose().then(
      (s) => {
        setStatus(s);
        setJamming(false);
      },
      (e) => setError(String(e)),
    );
  }, []);
  // The start dialog: at launch unless Settings says to open the last
  // project, once any crash recovery is settled. In a browser `?start`
  // shows it (`?start=none` with no recent projects).
  const [startOpen, setStartOpen] = useState<boolean | null>(() =>
    params ? params.has("start") : null,
  );
  const closeStart = useCallback(() => setStartOpen(false), []);
  useEffect(() => {
    if (startOpen === null && status) setStartOpen(!status.openLastAtLaunch);
  }, [startOpen, status]);
  const closeProjects = useCallback(() => {
    setProjectsOpen(false);
    setProjectsIntent({});
  }, []);
  // Why the Projects sheet opened from the menu: New Project focuses the
  // name, Open Recent asks before dropping unsaved edits.
  const [projectsIntent, setProjectsIntent] = useState<{ startNew?: boolean; opening?: string }>(
    {},
  );
  // File ▸ Rename and File ▸ Duplicate.
  // In a browser `?rename` and `?duplicate` open the sheet, for screenshots.
  const [naming, setNaming] = useState<ProjectNameMode | null>(() =>
    params?.has("rename") ? "rename" : params?.has("duplicate") ? "duplicate" : null,
  );
  const closeNaming = useCallback(() => setNaming(null), []);
  // Cantina ▸ Check for Updates opens Settings and checks.
  const [checkUpdates, setCheckUpdates] = useState(false);
  // The timeline's clip edits, for the Edit menu.
  const timelineEdit = useRef<TimelineEdit | null>(null);
  const [clipEdit, setClipEdit] = useState<TimelineEditState>({
    selected: false,
    canSplit: false,
    span: null,
  });
  const [skin, setSkin] = useSkin();
  // View ▸ Show Meters, remembered for next time.
  // In a browser `?nometers` hides them, for screenshots.
  const [metersShown, setMetersShown] = useState(() => {
    if (params?.has("nometers")) return false;
    try {
      return window.localStorage.getItem(METERS_KEY) !== "hidden";
    } catch {
      return true;
    }
  });
  const showMeters = useCallback((on: boolean) => {
    setMetersShown(on);
    try {
      window.localStorage.setItem(METERS_KEY, on ? "shown" : "hidden");
    } catch {
      // Not remembered this time.
    }
  }, []);
  // Help ▸ Keyboard Shortcuts. In a browser `?shortcuts` opens it.
  const [shortcutsOpen, setShortcutsOpen] = useState(() => params?.has("shortcuts") ?? false);
  const closeShortcuts = useCallback(() => setShortcutsOpen(false), []);
  // Counts Track ▸ Rename Track for the editor.
  const [renameRequest, setRenameRequest] = useState(0);
  // Counts File ▸ Bounce Track in Place for the editor.
  const [bounceRequest, setBounceRequest] = useState(0);
  const closeExport = useCallback(() => setExportOpen(false), []);
  const closePicker = useCallback(() => setPickerOpen(false), []);
  // Whether a text field has focus, so Undo and Redo stay available to
  // the field's own undo when the project has nothing to undo.
  const [typing, setTyping] = useState(false);
  useEffect(() => {
    const update = () => setTyping(isTyping(document.activeElement));
    window.addEventListener("focusin", update);
    window.addEventListener("focusout", update);
    return () => {
      window.removeEventListener("focusin", update);
      window.removeEventListener("focusout", update);
    };
  }, []);
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
    if (params?.has("jam")) {
      jamOpen().then(setStatus, (e) => setError(String(e)));
      return;
    }
    engineStatus().then(setStatus, (e) => setError(String(e)));
  }, []);

  useEffect(() => {
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
        for (const path of paths.filter((p) => p.endsWith(".pattern.json"))) {
          patternImport(path, track ?? undefined).then(setStatus, (e) => setError(String(e)));
        }
      }),
    [],
  );

  // A take over a loop stops itself at the loop's end; the shell then
  // needs Stop to put the take on its track. Only once the take has
  // rolled, so the frames before the transport starts do not count.
  const rolledWhileRecording = useRef(false);
  useEffect(() => {
    if (!frame.recording) {
      rolledWhileRecording.current = false;
      return;
    }
    if (frame.playing) {
      rolledWhileRecording.current = true;
      return;
    }
    if (rolledWhileRecording.current && frame.countIn === 0) {
      rolledWhileRecording.current = false;
      transportStop().then(setStatus, (e) => setError(String(e)));
    }
  }, [frame.recording, frame.playing, frame.countIn]);

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

  // Every action the menu bar and the keyboard run, from the registry in
  // `lib/actions.json`, wired to the app and to whether it applies now.
  // The menu bar learns the state through `menuState`; a chosen item or a
  // key runs the same handler a button does.
  const report = useCallback(
    (action: Promise<EngineStatus>) => action.then(setStatus, (e) => setError(String(e))),
    [],
  );
  const rolling = frame.playing || frame.recording;
  const hasClips =
    status !== null &&
    (status.audition !== null ||
      status.metronome.on ||
      status.project.tracks.some((t) => t.clips.length > 0 || t.patternClips.length > 0));
  const project = status?.project ?? null;
  const has = status !== null;
  // The assistant shows only once it is on and has an endpoint and a model.
  const ready = assistantReady(status);
  const idle = has && !frame.recording;
  const bounceable =
    selected !== null &&
    (selected.kit ? selected.patternClips.length > 0 : selected.clips.length > 0);
  const failed = (e: unknown) => setError(String(e));
  // View ▸ Skin: one checked item per skin, as the Settings picker.
  const skinBindings = Object.fromEntries(
    SKINS.map((s) => [
      `skin-${s.id}`,
      { enabled: true, checked: skin === s.id, run: () => setSkin(s.id) },
    ]),
  ) as Pick<Bindings, `skin-${SkinId}`>;
  const chainBypassed =
    selected !== null && selected.chain.length > 0 && selected.chain.every((p) => p.bypassed);
  const bindings: Bindings = {
    settings: { enabled: has, run: () => setSettingsOpen(true) },
    checkUpdates: {
      enabled: has,
      run: () => {
        setCheckUpdates(true);
        setSettingsOpen(true);
      },
    },
    newProject: {
      enabled: idle,
      run: () => {
        setStartOpen(false);
        setProjectsIntent({ startNew: true });
        setProjectsOpen(true);
      },
    },
    openProject: {
      enabled: idle,
      run: () => {
        setStartOpen(false);
        setProjectsIntent({});
        setProjectsOpen(true);
      },
    },
    save: { enabled: project?.dirty ?? false, run: () => report(projectSave()) },
    duplicateProject: { enabled: idle, run: () => setNaming("duplicate") },
    renameProject: { enabled: idle, run: () => setNaming("rename") },
    importAudio: {
      enabled: idle,
      run: () => {
        if (!status) return;
        const start = Math.round(frame.position * status.project.sampleRate);
        chooseAudioPaths().then(async (paths) => {
          if (paths.length === 0) return;
          // Onto the selected audio track, or a new one.
          let track = selected && !selected.kit ? selected.id : null;
          if (track === null) {
            const added = await trackAdd();
            track = added.project.tracks.filter((t) => !t.kit).at(-1)?.id ?? null;
            setStatus(added);
          }
          if (track === null) return;
          setSelectedTrack(track);
          report(audioImport(paths, track, start));
        }, failed);
      },
    },
    importPedal: {
      enabled: has,
      run: () =>
        chooseImportPaths().then((paths) => {
          for (const path of paths) report(pedalImport(path, selected?.id));
        }, failed),
    },
    importPattern: {
      enabled: has,
      run: () => {
        if (!status) return;
        const drums = selected?.kit ? selected.id : undefined;
        choosePatternImportPaths(status.library.patternsDir).then((paths) => {
          for (const path of paths) report(patternImport(path, drums));
        }, failed);
      },
    },
    importKit: {
      enabled: has,
      run: () =>
        chooseKitDir().then((dir) => {
          if (dir) report(kitImport(dir));
        }, failed),
    },
    export: { enabled: has, run: () => setExportOpen(true) },
    bounce: {
      enabled: idle && !rolling && bounceable,
      run: () => setBounceRequest((n) => n + 1),
    },
    revealProject: { enabled: has, run: () => reveal("project").catch(failed) },
    revealLibrary: { enabled: has, run: () => reveal("library").catch(failed) },
    // In a text field ⌘Z is the field's own undo.
    undo: {
      enabled: typing || (status?.project.undoLabel ?? null) !== null,
      label: typing ? "Undo" : editTitle("Undo", status?.project.undoLabel ?? null),
      run: () => {
        if (isTyping(document.activeElement)) document.execCommand("undo");
        else report(projectUndo());
      },
    },
    redo: {
      enabled: typing || (status?.project.redoLabel ?? null) !== null,
      label: typing ? "Redo" : editTitle("Redo", status?.project.redoLabel ?? null),
      run: () => {
        if (isTyping(document.activeElement)) document.execCommand("redo");
        else report(projectRedo());
      },
    },
    // The selected clip in the timeline.
    duplicate: {
      enabled: clipEdit.selected && !typing,
      run: () => timelineEdit.current?.duplicate(),
    },
    delete: {
      enabled: clipEdit.selected && !typing,
      run: () => timelineEdit.current?.remove(),
    },
    split: {
      enabled: clipEdit.canSplit && !typing,
      run: () => timelineEdit.current?.splitAtPlayhead(),
    },
    play: {
      enabled: running && (jamming || rolling || hasClips),
      label: rolling || frame.countIn > 0 ? "Stop" : "Play",
      run: () => {
        if (jamming) report(rolling || frame.countIn > 0 ? jamStop() : jamPlay());
        else report(rolling ? transportStop() : transportPlay());
      },
    },
    record: {
      enabled: running && !jamming && !rolling && tracks.some((t) => t.armed && !t.kit),
      run: () => report(transportRecord()),
    },
    countIn: {
      enabled: has,
      checked: status?.countIn ?? true,
      run: () => report(countInSet(!(status?.countIn ?? true))),
    },
    // Loop on and off; with no region yet, four bars from the playhead's bar.
    loop: {
      enabled: has,
      checked: project?.loop?.on ?? false,
      run: () => {
        if (!status) return;
        const region = status.project.loop;
        if (region) report(loopSet({ ...region, on: !region.on }));
        else report(loopSet({ ...barsFrom(status, frame.position), on: true }));
      },
    },
    loopToClip: {
      enabled: clipEdit.span !== null,
      run: () => {
        if (clipEdit.span) report(loopSet({ ...clipEdit.span, on: true }));
      },
    },
    goToStart: { enabled: running && !frame.recording, run: () => report(transportLocate(0)) },
    goToEnd: {
      enabled: running && !frame.recording,
      run: () => {
        if (status) report(transportLocate(songEnd(status)));
      },
    },
    tempo: {
      enabled: has,
      run: () => {
        const field = document.getElementById("tempo-bpm") as HTMLInputElement | null;
        field?.focus();
        field?.select();
      },
    },
    newAudioTrack: {
      enabled: idle,
      run: () =>
        trackAdd().then((s) => {
          setStatus(s);
          setSelectedTrack(s.project.tracks.at(-1)?.id ?? null);
        }, failed),
    },
    newDrumTrack: {
      enabled: idle && (status?.kits.length ?? 0) > 0,
      run: () =>
        trackAddDrums().then((s) => {
          setStatus(s);
          setSelectedTrack(s.project.tracks.at(-1)?.id ?? null);
        }, failed),
    },
    arm: {
      enabled: selected !== null && !selected.kit && !frame.recording,
      checked: selected?.armed ?? false,
      run: () => {
        if (selected) report(trackArm(selected.id, !selected.armed));
      },
    },
    mute: {
      enabled: selected !== null,
      checked: selected?.strip.mute ?? false,
      run: () => {
        if (selected) report(stripMute(selected.id, !selected.strip.mute));
      },
    },
    solo: {
      enabled: selected !== null,
      checked: selected?.strip.solo ?? false,
      run: () => {
        if (selected) report(stripSolo(selected.id, !selected.strip.solo));
      },
    },
    renameTrack: {
      enabled: selected !== null,
      run: () => setRenameRequest((n) => n + 1),
    },
    removeTrack: {
      enabled: selected !== null && !frame.recording,
      run: () => {
        if (selected) report(trackRemove(selected.id));
      },
    },
    addPedal: { enabled: selected !== null, run: () => setPickerOpen(true) },
    bypassChain: {
      enabled: (selected?.chain.length ?? 0) > 0,
      checked: chainBypassed,
      run: () => {
        if (selected) report(chainBypass(selected.id, !chainBypassed));
      },
    },
    arrange: {
      enabled: true,
      run: () => {
        setStageOpen(false);
        setPerforming(false);
        setDesigning(null);
        if (jamming) leaveJam();
      },
    },
    jam: {
      enabled: has && !frame.recording,
      checked: jamming,
      run: () => {
        setStartOpen(false);
        if (jamming) leaveJam();
        else openJam();
      },
    },
    // The selected track's first designed pedal, or a new design on it.
    designer: {
      enabled: selected !== null,
      run: () => {
        if (!selected) return;
        const designed = selected.chain.find((p) => p.spec.startsWith("pedal:"));
        design(designed?.id ?? null, selected.id);
      },
    },
    stage: { enabled: has, run: () => setStageOpen(true) },
    assistant: {
      enabled: ready,
      checked: assistantOpen && ready,
      label: status?.assistant.persona ? "Tapster" : "Assistant",
      run: () => setAssistantOpen((open) => !open),
    },
    ...skinBindings,
    zoomIn: { enabled: has, run: () => timelineEdit.current?.zoomIn() },
    zoomOut: { enabled: has, run: () => timelineEdit.current?.zoomOut() },
    zoomFit: { enabled: has, run: () => timelineEdit.current?.fit() },
    showMeters: { enabled: true, checked: metersShown, run: () => showMeters(!metersShown) },
    stageWindow: {
      enabled: inTauri,
      run: () => {
        openStageWindow().catch(failed);
      },
    },
    help: { enabled: true, run: () => openLink("docs").catch(failed) },
    shortcuts: { enabled: true, run: () => setShortcutsOpen(true) },
    releaseNotes: { enabled: true, run: () => openLink("releases").catch(failed) },
    reportIssue: { enabled: true, run: () => openLink("issue").catch(failed) },
    metronome: {
      enabled: status !== null,
      checked: status?.metronome.on ?? false,
      run: () => {
        if (!status) return;
        const { on, sound, levelDb } = status.metronome;
        report(metronomeSet({ on: !on, sound, levelDb }));
      },
    },
    perform: { enabled: status !== null, run: perform },
  };
  const bound = useRef(bindings);
  bound.current = bindings;
  const keys = useRef(status);
  keys.current = status;
  const menu = JSON.stringify(menuStates(bindings));
  useEffect(() => {
    menuState(JSON.parse(menu)).catch((e) => setError(String(e)));
  }, [menu]);
  useEffect(() => {
    const run = (id: ActionId) => {
      const action = bound.current[id];
      if (!action.enabled) return false;
      action.run();
      return true;
    };
    const onKey = (e: KeyboardEvent) => {
      const id = actionForKey(e, e.target as Element | null, inTauri);
      if (id && run(id)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    const stopMenu = onMenuAction(run);
    // File ▸ Open Recent: opened at once, or through the Projects sheet,
    // which asks before dropping unsaved edits.
    const stopRecent = onOpenRecent((name) => {
      const s = keys.current;
      setStartOpen(false);
      if (!s || s.project.name === name) return;
      if (s.project.dirty) {
        setProjectsIntent({ opening: name });
        setProjectsOpen(true);
      } else {
        projectOpen(name).then(setStatus, (e) => setError(String(e)));
      }
    });
    return () => {
      window.removeEventListener("keydown", onKey);
      stopMenu();
      stopRecent();
    };
  }, []);

  return (
    <Window
      title="Cantina"
      start={
        status ? (
          <ProjectBar
            project={status.project}
            onOpenProjects={() => setProjectsOpen(true)}
            onOpenExport={() => setExportOpen(true)}
            onStatus={setStatus}
            onError={setError}
          />
        ) : null
      }
      end={
        <>
          <EngineControl status={status} busy={busy} onToggle={toggle} />
          {ready ? (
            <Button
              aria-label={status?.assistant.persona ? "Tapster" : "Assistant"}
              title={`${status?.assistant.persona ? "Tapster" : "Assistant"} (⌘J)`}
              aria-pressed={assistantOpen}
              onClick={() => setAssistantOpen((open) => !open)}
              square
            >
              <Droid size={20} />
            </Button>
          ) : null}
          <Button
            aria-label="Jam"
            title="Jam (J)"
            aria-pressed={jamming}
            disabled={!status || frame.recording}
            onClick={() => (jamming ? leaveJam() : openJam())}
            square
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
              <path d="M11 1.5v8.6a2.4 2.4 0 1 1-1.5-2.2V4.3L6 5.2v6.4a2.4 2.4 0 1 1-1.5-2.2V3.1z" />
            </svg>
          </Button>
          <Button
            aria-label="Stage"
            title="Stage"
            aria-haspopup="dialog"
            aria-expanded={stageOpen}
            onClick={() => setStageOpen(true)}
            square
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
              <path d="M1 3h14v9H1zm1.5 1.5v6h11v-6zM4 9l2.5-3 2 2.2L10 6.5 12 9zM5 13.5h6V15H5z" />
            </svg>
          </Button>
          <Button
            aria-label="Settings"
            title="Settings (⌘,)"
            aria-haspopup="dialog"
            aria-expanded={settingsOpen}
            onClick={() => setSettingsOpen(true)}
            square
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
              <path d="M8 5a3 3 0 1 0 0 6 3 3 0 0 0 0-6zm6.6 2-1.5-.3a5.3 5.3 0 0 0-.5-1.2l.9-1.3-1.7-1.7-1.3.9a5.3 5.3 0 0 0-1.2-.5L9 1.4H7l-.3 1.5a5.3 5.3 0 0 0-1.2.5l-1.3-.9-1.7 1.7.9 1.3a5.3 5.3 0 0 0-.5 1.2L1.4 7v2l1.5.3c.1.4.3.8.5 1.2l-.9 1.3 1.7 1.7 1.3-.9c.4.2.8.4 1.2.5L7 14.6h2l.3-1.5c.4-.1.8-.3 1.2-.5l1.3.9 1.7-1.7-.9-1.3c.2-.4.4-.8.5-1.2l1.5-.3V7z" />
            </svg>
          </Button>
        </>
      }
      status={
        status && transport && !jamming
          ? transportNote(transport, status.project.tracks, frame.countIn)
          : null
      }
      meters={
        metersShown && !jamming ? (
          <>
            <Meter compact label="In" level={frame.input} />
            <Meter compact label="Out" level={frame.output} />
          </>
        ) : null
      }
    >
      <div className="flex min-h-0 flex-1">
        <main className="flex min-h-0 min-w-0 flex-1 flex-col gap-3 px-window pt-3 pb-2">
          {status?.notice ? <Notice message={status.notice} onDismiss={dismiss} /> : null}
          {update && status ? (
            <UpdateBanner
              update={update}
              dirty={status.project.dirty}
              recording={frame.recording}
              onDismiss={() => setUpdate(null)}
            />
          ) : null}
          {status?.project.recovery ? (
            <Recovery recovery={status.project.recovery} onStatus={setStatus} onError={setError} />
          ) : null}
          {status?.latency.warn ? (
            <Lcd role="status" error className="text-readout">
              Plugins delay playback by {ms(status.latency.compensationFrames, status.sampleRate)},
              above the {status.latency.warnMs} ms you set. Live monitoring is not delayed.
            </Lcd>
          ) : null}
          {problem ? (
            <Lcd role="alert" error>
              {problem}
            </Lcd>
          ) : null}

          {status && jamming ? (
            <JamView
              status={status}
              frame={frame}
              onPlay={() => bound.current.play.run()}
              onPerform={perform}
              onLeave={leaveJam}
              onStatus={setStatus}
              onError={setError}
            />
          ) : status && transport ? (
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
                      metronome={status.metronome}
                      running={running}
                      position={frame.position}
                      countIn={frame.countIn}
                      auditioning={status.audition !== null}
                      timeline={controls}
                      onStatus={setStatus}
                      onError={setError}
                    />
                  )}
                  onSelectTrack={setSelectedTrack}
                  onStatus={setStatus}
                  onError={setError}
                  editRef={timelineEdit}
                  onEditState={setClipEdit}
                  loopRegion={status.project.loop}
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
                  library={status.library}
                  audition={status.audition}
                  practice={status.practice}
                  grooves={status.grooves}
                  rigs={status.rigs}
                  rigAudition={status.rigAudition}
                  rigsOpen={params?.has("rigs") ?? false}
                  profile={status.instrumentProfile}
                  drumRows={status.drumRows}
                  tempo={status.project.tempo}
                  position={frame.position}
                  playing={transport.playing}
                  readouts={readouts}
                  irs={status.irs}
                  initialTab={initialTab}
                  bounceRequest={bounceRequest}
                  renameRequest={renameRequest}
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
        {assistantOpen && ready && status ? (
          <Assistant
            assistant={status.assistant}
            onClose={() => setAssistantOpen(false)}
            onStatus={setStatus}
          />
        ) : null}
      </div>
      {stageOpen && status ? (
        <StageView status={status} onClose={closeStage} onPerform={perform} onStatus={setStatus} />
      ) : null}
      {performing && status ? (
        <Performance
          status={status}
          initialBoard={params?.has("board") ?? false}
          frame={frame}
          onPlay={() => bound.current.play.run()}
          onExit={() => setPerforming(false)}
          onStatus={setStatus}
          onError={setError}
        />
      ) : null}
      {settingsOpen && status ? (
        <Settings
          status={status}
          busy={busy}
          onToggleEngine={toggle}
          onClose={closeSettings}
          onStatusChange={setStatus}
          onUpdate={setUpdate}
          checkForUpdates={checkUpdates}
          section={settingsSection}
        />
      ) : null}
      {exportOpen && status ? (
        <ExportSheet
          projectName={status.project.name}
          sampleRate={status.project.sampleRate}
          onClose={closeExport}
        />
      ) : null}
      {projectsOpen && status ? (
        <Projects
          current={status.project.name}
          dirty={status.project.dirty}
          onClose={closeProjects}
          onStatus={setStatus}
          startNew={projectsIntent.startNew ?? false}
          opening={projectsIntent.opening ?? null}
        />
      ) : null}
      {startOpen && status && !status.project.recovery && !jamming && !projectsOpen ? (
        <StartDialog
          current={status.project.name}
          sampleRate={status.sampleRate}
          hasKits={status.kits.length > 0}
          onStatus={setStatus}
          onClose={closeStart}
          onBrowse={() => {
            setStartOpen(false);
            setProjectsIntent({});
            setProjectsOpen(true);
          }}
          onJam={() => {
            setStartOpen(false);
            openJam();
          }}
        />
      ) : null}
      {shortcutsOpen ? <Shortcuts onClose={closeShortcuts} /> : null}
      {naming && status ? (
        <ProjectName
          mode={naming}
          current={status.project.name}
          onClose={closeNaming}
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
          broken={brokenOf(status.library, "pedal")}
          plugins={status.plugins}
          onClose={closePicker}
          onDesign={() => design(null, selected.id)}
          onStatus={setStatus}
        />
      ) : null}
    </Window>
  );
}
