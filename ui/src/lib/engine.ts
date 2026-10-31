import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { decodeFrame, encodeFrame } from "./frame";

/** What the shell reports about the engine. Mirrors `EngineStatus` in src-tauri. */
export interface EngineStatus {
  running: boolean;
  sampleRate: number;
  bufferFrames: number;
  inputUid: string | null;
  outputUid: string | null;
  inputName: string | null;
  outputName: string | null;
  /** Why the last start, stop or rebuild failed, if it did. */
  error: string | null;
  /** What the engine did on its own, such as falling back to another device. */
  notice: string | null;
  noticeSeq: number;
  transport: TransportStatus;
  project: ProjectStatus;
  /** Specs `chainAdd` accepts. */
  pedalKinds: string[];
  plugins: PluginsStatus;
}

/** The scanner in brief; `pluginList` has the list. Mirrors `PluginsStatus` in src-tauri. */
export interface PluginsStatus {
  /** True while a scan runs in the background. */
  scanning: boolean;
  count: number;
  failed: number;
  /** Seconds since the epoch of the last scan, zero before the first. */
  scannedAt: number;
}

export type PluginFormat = "au" | "clap";

/** A plugin the scanner found. Mirrors `PluginEntry` in crates/hosting. */
export interface PluginEntry {
  format: PluginFormat;
  /** The spec `chainAdd` takes: `au:…` or `clap:…#…`. */
  spec: string;
  source: string;
  name: string;
  vendor: string;
  version: string;
  inputs: number;
  outputs: number;
  hasEditor: boolean;
}

/** A plugin the scanner's child process could not load, or died loading. */
export interface PluginFailure {
  format: PluginFormat;
  source: string;
  name: string;
  why: string;
}

/** Everything the scanner knows. Mirrors `PluginList` in crates/hosting. */
export interface PluginList {
  version: number;
  scannedAt: number;
  plugins: PluginEntry[];
  failed: PluginFailure[];
}

export interface TransportStatus {
  playing: boolean;
  /** True while any track is recording. */
  recording: boolean;
}

/** The open project. Mirrors `ProjectStatus` in src-tauri. */
export interface ProjectStatus {
  name: string;
  dir: string;
  /** True when there are edits since the last save. */
  dirty: boolean;
  /** What Undo would revert, if anything. */
  undoLabel: string | null;
  /** What Redo would reapply, if anything. */
  redoLabel: string | null;
  /** Unsaved edits from a session that did not save, waiting on the user. */
  recovery: RecoveryInfo | null;
  sampleRate: number;
  tempo: Tempo;
  tracks: TrackStatus[];
  /** The master strip; pan and solo are unused on it. */
  master: Strip;
}

/** A mixer strip. Mirrors `Strip` in crates/project. */
export interface Strip {
  gainDb: number;
  /** -1 is hard left, 1 hard right. */
  pan: number;
  mute: boolean;
  solo: boolean;
}

export const UNITY: Strip = { gainDb: 0, pan: 0, mute: false, solo: false };

export interface RecoveryInfo {
  edits: number;
  /** The last edit's label. */
  last: string;
}

export interface Tempo {
  bpm: number;
  numerator: number;
  denominator: number;
}

/** A track. Mirrors `TrackStatus` in src-tauri. */
export interface TrackStatus {
  id: number;
  name: string;
  /** Armed tracks monitor their input and record when the transport records. */
  armed: boolean;
  recording: boolean;
  chain: PedalInfo[];
  clips: ClipInfo[];
  strip: Strip;
}

/** A clip. Positions are in samples at the project's rate. */
export interface ClipInfo {
  id: number;
  name: string;
  file: string;
  start: number;
  offset: number;
  length: number;
}

/** A pedal in a chain. Mirrors `PedalInfo` in src-tauri. */
export interface PedalInfo {
  /** Stable for the pedal's life. */
  id: number;
  spec: string;
  name: string;
  params: ParamInfo[];
}

export interface ParamInfo {
  name: string;
  /** "dB", "Hz", "s", "%" or "". */
  unit: string;
  min: number;
  max: number;
  default: number;
  value: number;
}

/** Which devices to open and how. `null` follows the system default. */
export interface AudioSettings {
  inputUid: string | null;
  outputUid: string | null;
  sampleRate: number;
  bufferFrames: number;
}

/** A device as the picker shows it. Mirrors `DeviceEntry` in src-tauri. */
export interface AudioDevice {
  uid: string;
  name: string;
  inputChannels: number;
  outputChannels: number;
  sampleRates: number[];
  bufferMin: number;
  bufferMax: number;
  defaultInput: boolean;
  defaultOutput: boolean;
}

/** Linear levels, 0 to 1. */
export interface Level {
  peak: number;
  rms: number;
}

/** One meter update, at display rate. Mirrors `MeterFrame` in src-tauri. */
export interface MeterFrame {
  input: Level;
  /** The master, post-fader. */
  output: Level;
  /** One per track, post-fader, in the project's track order. */
  tracks: Level[];
  /** Timeline position in seconds. */
  position: number;
  playing: boolean;
  recording: boolean;
}

/** True inside the Tauri web view, false in a plain browser tab. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const SILENT: Level = { peak: 0, rms: 0 };

/*
 * In a browser (`npm run dev` without the shell) there is no engine, so the
 * UI talks to this stand-in instead. It keeps `vite dev` useful for layout
 * and skin work and is what the PR screenshots are taken from. `?running`
 * starts it with the stream open; `?armed`, `?take`, `?playing` and
 * `?recording` set up the project and transport.
 */
const mockDevices: AudioDevice[] = [
  {
    uid: "BuiltInMicrophoneDevice",
    name: "MacBook Pro Microphone",
    inputChannels: 1,
    outputChannels: 0,
    sampleRates: [44100, 48000, 88200, 96000],
    bufferMin: 14,
    bufferMax: 4096,
    defaultInput: true,
    defaultOutput: false,
  },
  {
    uid: "BuiltInSpeakerDevice",
    name: "MacBook Pro Speakers",
    inputChannels: 0,
    outputChannels: 2,
    sampleRates: [44100, 48000, 88200, 96000],
    bufferMin: 14,
    bufferMax: 4096,
    defaultInput: false,
    defaultOutput: true,
  },
  {
    uid: "AppleUSBAudioEngine:Focusrite:Scarlett 2i2 USB",
    name: "Scarlett 2i2 USB",
    inputChannels: 2,
    outputChannels: 2,
    sampleRates: [44100, 48000, 88200, 96000, 176400, 192000],
    bufferMin: 16,
    bufferMax: 1024,
    defaultInput: false,
    defaultOutput: false,
  },
];

const mockSettings: AudioSettings = {
  inputUid: null,
  outputUid: null,
  sampleRate: 48000,
  bufferFrames: 128,
};

const mockParams = inTauri ? null : new URLSearchParams(window.location.search);
const MOCK_RATE = 48000;
/** The stand-in scanned ten minutes ago. */
const MOCK_SCANNED_AT = Math.floor(Date.now() / 1000) - 600;

function mockPedal(id: number, spec: string): PedalInfo {
  return spec === "overdrive"
    ? {
        id,
        spec,
        name: "Overdrive",
        params: [
          { name: "Drive", unit: "dB", min: 0, max: 40, default: 20, value: 20 },
          { name: "Tone", unit: "Hz", min: 500, max: 8000, default: 3000, value: 3000 },
          { name: "Level", unit: "dB", min: -24, max: 6, default: -6, value: -6 },
        ],
      }
    : {
        id,
        spec: "gain",
        name: "Gain",
        params: [{ name: "Gain", unit: "dB", min: -60, max: 24, default: 0, value: 0 }],
      };
}

function mockClip(id: number, name: string, startSeconds: number, seconds: number): ClipInfo {
  return {
    id,
    name,
    file: `${name.replace(/\s+/g, "-")}.wav`,
    start: Math.round(startSeconds * MOCK_RATE),
    offset: 0,
    length: Math.round(seconds * MOCK_RATE),
  };
}

let mockNextId = 20;
const mockTransport = { position: 0, playing: false, recording: false };
const mockProjects = ["Untitled", "Blues in A", "Riff ideas"];

const mock: EngineStatus = {
  running: mockParams?.has("running") ?? false,
  sampleRate: MOCK_RATE,
  bufferFrames: 128,
  inputUid: null,
  outputUid: null,
  inputName: null,
  outputName: null,
  error: null,
  notice: mockParams?.has("notice")
    ? "Scarlett 2i2 USB was disconnected; the saved input device is not connected; using MacBook Pro Microphone"
    : null,
  noticeSeq: mockParams?.has("notice") ? 1 : 0,
  transport: {
    playing: mockParams?.has("playing") ?? false,
    recording: mockParams?.has("recording") ?? false,
  },
  project: {
    name: mockParams?.get("project") ?? "Untitled",
    dir: "/Users/you/Music/Cantina/Untitled.cantina",
    dirty: mockParams?.has("dirty") ?? false,
    undoLabel: mockParams?.has("dirty") ? "move clip" : null,
    redoLabel: null,
    recovery: mockParams?.has("recovery") ? { edits: 7, last: "split clip" } : null,
    sampleRate: MOCK_RATE,
    tempo: { bpm: 120, numerator: 4, denominator: 4 },
    tracks: [
      {
        id: 1,
        name: "Guitar",
        armed: mockParams?.has("armed") ?? false,
        recording: mockParams?.has("recording") ?? false,
        chain: [mockPedal(2, "gain"), mockPedal(3, "overdrive")],
        clips: mockParams?.has("take")
          ? [mockClip(4, "Guitar 1", 0, 12.4), mockClip(5, "Guitar 2", 16, 6.1)]
          : [],
        strip: { gainDb: -3, pan: -0.25, mute: false, solo: mockParams?.has("solo") ?? false },
      },
      {
        id: 6,
        name: "Bass",
        armed: false,
        recording: false,
        chain: [mockPedal(7, "gain")],
        clips: mockParams?.has("take") ? [mockClip(8, "Bass 1", 2, 20)] : [],
        strip: { gainDb: 0, pan: 0.1, mute: mockParams?.has("mute") ?? false, solo: false },
      },
    ],
    master: { gainDb: -1, pan: 0, mute: false, solo: false },
  },
  pedalKinds: ["gain", "overdrive"],
  plugins: {
    scanning: mockParams?.has("scanning") ?? false,
    count: 5,
    failed: 1,
    scannedAt: mockParams?.has("scanning") ? 0 : MOCK_SCANNED_AT,
  },
};
mockTransport.playing = mock.transport.playing || mock.transport.recording;
mockTransport.recording = mock.transport.recording;

function mockDevice(uid: string | null, input: boolean): AudioDevice | undefined {
  return uid
    ? mockDevices.find((d) => d.uid === uid)
    : mockDevices.find((d) => (input ? d.defaultInput : d.defaultOutput));
}

function mockStatus(): EngineStatus {
  const i = mockDevice(mockSettings.inputUid, true);
  const o = mockDevice(mockSettings.outputUid, false);
  return {
    ...mock,
    transport: { playing: mockTransport.playing, recording: mockTransport.recording },
    project: {
      ...mock.project,
      tracks: mock.project.tracks.map((t) => ({
        ...t,
        recording: mockTransport.recording && t.armed,
        chain: t.chain.map((p) => ({ ...p, params: p.params.map((q) => ({ ...q })) })),
        clips: t.clips.map((c) => ({ ...c })),
        strip: { ...t.strip },
      })),
      master: { ...mock.project.master },
    },
    sampleRate: mockSettings.sampleRate,
    bufferFrames: mockSettings.bufferFrames,
    inputUid: mockSettings.inputUid,
    outputUid: mockSettings.outputUid,
    inputName: mock.running ? (i?.name ?? null) : null,
    outputName: mock.running ? (o?.name ?? null) : null,
  };
}

function mockTrack(id: number): TrackStatus {
  const track = mock.project.tracks.find((t) => t.id === id);
  if (!track) throw new Error(`no track ${id}`);
  return track;
}

// The stand-in's undo: a stack of snapshots taken after each edit, so
// the one before the top is what undo goes back to. No coalescing.
const mockUndo: ProjectStatus[] = [];
const mockRedo: ProjectStatus[] = [];
let mockLast: ProjectStatus = structuredClone(mock.project);

/** Called after an edit to the mock project. */
function mockEdit(label = "edit"): EngineStatus {
  mock.project.dirty = true;
  mock.project.undoLabel = label;
  mock.project.redoLabel = null;
  mockRedo.length = 0;
  mockUndo.push(mockLast);
  mockLast = structuredClone(mock.project);
  return mockStatus();
}

const statusListeners = new Set<(s: EngineStatus) => void>();
function mockPublish() {
  const s = mockStatus();
  for (const l of statusListeners) l(s);
}

// ----- plugins -----

function mockPlugin(
  format: PluginFormat,
  vendor: string,
  name: string,
  io: [number, number],
  hasEditor: boolean,
): PluginEntry {
  const spec =
    format === "au"
      ? `au:aufx:${name.slice(0, 4).toLowerCase()}:appl`
      : `clap:/Library/Audio/Plug-Ins/CLAP/${name}.clap#com.${vendor.toLowerCase()}.${name.toLowerCase()}`;
  return {
    format,
    spec,
    source: spec,
    name,
    vendor,
    version: format === "au" ? "1.7.0" : "2.3.1",
    inputs: io[0],
    outputs: io[1],
    hasEditor,
  };
}

const mockPlugins: PluginList = {
  version: 1,
  scannedAt: MOCK_SCANNED_AT,
  plugins: [
    mockPlugin("au", "Apple", "AUDelay", [2, 2], false),
    mockPlugin("au", "Apple", "AUDistortion", [2, 2], false),
    mockPlugin("au", "Apple", "AUMatrixReverb", [2, 2], false),
    mockPlugin("au", "Neural DSP", "Archetype Plini", [2, 2], true),
    mockPlugin("clap", "Surge Synth Team", "Surge XT Effects", [2, 2], true),
  ],
  failed: [
    {
      format: "clap",
      source: "clap:/Library/Audio/Plug-Ins/CLAP/Old Fuzz.clap",
      name: "Old Fuzz",
      why: "crashed while loading (signal: 6 (SIGABRT))",
    },
  ],
};

/** What the scanner found. Starts a scan if none has run yet. */
export async function pluginList(): Promise<PluginList> {
  if (inTauri) return invoke<PluginList>("plugin_list");
  return mockParams?.has("scanning")
    ? { version: 1, scannedAt: 0, plugins: [], failed: [] }
    : structuredClone(mockPlugins);
}

/** Scans new and changed plugins in the background, or everything when `full`. */
export async function pluginRescan(full: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("plugin_rescan", { full });
  mock.plugins = { ...mock.plugins, scanning: true };
  mockPublish();
  window.setTimeout(() => {
    mock.plugins = { ...mock.plugins, scanning: false, scannedAt: Date.now() / 1000 };
    mockPublish();
  }, 1200);
  return mockStatus();
}

export async function engineStatus(): Promise<EngineStatus> {
  return inTauri ? invoke<EngineStatus>("engine_status") : mockStatus();
}

export async function engineStart(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("engine_start");
  mock.running = true;
  mockPublish();
  return mockStatus();
}

export async function engineStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("engine_stop");
  mock.running = false;
  mockPublish();
  return mockStatus();
}

export async function dismissNotice(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("dismiss_notice");
  mock.notice = null;
  return mockStatus();
}

export async function listDevices(): Promise<AudioDevice[]> {
  return inTauri ? invoke<AudioDevice[]>("list_devices") : mockDevices;
}

export async function audioSettings(): Promise<AudioSettings> {
  return inTauri ? invoke<AudioSettings>("audio_settings") : { ...mockSettings };
}

export async function setAudioSettings(settings: AudioSettings): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("set_audio_settings", { settings });
  Object.assign(mockSettings, settings);
  mockPublish();
  return mockStatus();
}

// ----- the project -----

/** Names of the projects in the projects folder. */
export async function projectList(): Promise<string[]> {
  return inTauri ? invoke<string[]>("project_list") : [...mockProjects];
}

export async function projectNew(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_new", { name });
  if (!name.trim()) throw new Error("a project needs a name");
  if (mockProjects.includes(name)) throw new Error(`there is already a project called ${name}`);
  mockProjects.push(name);
  mock.project = {
    ...mock.project,
    name,
    dir: `/Users/you/Music/Cantina/${name}.cantina`,
    dirty: false,
    tracks: [
      {
        id: 1,
        name: "Track 1",
        armed: false,
        recording: false,
        chain: [mockPedal(2, "gain"), mockPedal(3, "overdrive")],
        clips: [],
        strip: { ...UNITY },
      },
    ],
    master: { ...UNITY },
  };
  return mockStatus();
}

export async function projectOpen(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_open", { name });
  mock.project = {
    ...mock.project,
    name,
    dir: `/Users/you/Music/Cantina/${name}.cantina`,
    dirty: false,
  };
  return mockStatus();
}

export async function projectSave(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_save");
  mock.project.dirty = false;
  return mockStatus();
}

export async function projectRecover(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_recover");
  mock.project.recovery = null;
  mock.project.dirty = true;
  return mockStatus();
}

export async function projectDiscardRecovery(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_discard_recovery");
  mock.project.recovery = null;
  return mockStatus();
}

export async function projectUndo(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_undo");
  const previous = mockUndo.pop();
  if (!previous) return mockStatus();
  mockRedo.push(mockLast);
  mock.project = structuredClone(previous);
  mockLast = previous;
  mock.project.redoLabel = "edit";
  mock.project.undoLabel = mockUndo.length > 0 ? "edit" : null;
  mock.project.dirty = true;
  return mockStatus();
}

export async function projectRedo(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_redo");
  const next = mockRedo.pop();
  if (!next) return mockStatus();
  mockUndo.push(mockLast);
  mock.project = structuredClone(next);
  mockLast = next;
  mock.project.undoLabel = "edit";
  mock.project.redoLabel = mockRedo.length > 0 ? "edit" : null;
  mock.project.dirty = true;
  return mockStatus();
}

export async function tempoSet(tempo: Tempo): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("tempo_set", { tempo });
  mock.project.tempo = tempo;
  return mockEdit();
}

// ----- the mixer -----

function mockStrip(track: number | null): Strip {
  return track === null ? mock.project.master : mockTrack(track).strip;
}

/** Gain on a track's strip, or the master's with `null`. Not echoed. */
export async function stripGain(track: number | null, db: number): Promise<void> {
  if (inTauri) return invoke("strip_gain", { track, db });
  mockStrip(track).gainDb = db;
  mock.project.dirty = true;
}

/** Pan on a track's strip. Not echoed. */
export async function stripPan(track: number, pan: number): Promise<void> {
  if (inTauri) return invoke("strip_pan", { track, pan });
  mockStrip(track).pan = pan;
  mock.project.dirty = true;
}

export async function stripMute(track: number | null, on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("strip_mute", { track, on });
  mockStrip(track).mute = on;
  return mockEdit();
}

export async function stripSolo(track: number, on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("strip_solo", { track, on });
  mockStrip(track).solo = on;
  return mockEdit();
}

// ----- tracks and clips -----

export async function trackAdd(name?: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_add", { name: name ?? null });
  mock.project.tracks.push({
    id: mockNextId++,
    name: name ?? `Track ${mock.project.tracks.length + 1}`,
    armed: false,
    recording: false,
    chain: [],
    clips: [],
    strip: { ...UNITY },
  });
  return mockEdit();
}

export async function trackRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_remove", { id });
  mock.project.tracks = mock.project.tracks.filter((t) => t.id !== id);
  return mockEdit();
}

export async function trackRename(id: number, name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_rename", { id, name });
  if (!name.trim()) throw new Error("a track needs a name");
  mockTrack(id).name = name.trim();
  return mockEdit();
}

export async function trackMove(id: number, to: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_move", { id, to });
  const tracks = mock.project.tracks;
  const from = tracks.findIndex((t) => t.id === id);
  const [track] = tracks.splice(from, 1);
  if (track) tracks.splice(Math.min(to, tracks.length), 0, track);
  return mockEdit();
}

export async function trackArm(id: number, armed: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_arm", { id, armed });
  mockTrack(id).armed = armed;
  return mockEdit();
}

export async function clipRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_remove", { id });
  for (const t of mock.project.tracks) t.clips = t.clips.filter((c) => c.id !== id);
  return mockEdit();
}

function mockClipAt(id: number): { track: TrackStatus; clip: ClipInfo; index: number } {
  for (const track of mock.project.tracks) {
    const index = track.clips.findIndex((c) => c.id === id);
    const clip = track.clips[index];
    if (clip) return { track, clip, index };
  }
  throw new Error(`no clip ${id}`);
}

/** Puts a clip at `start` on `track`, which may be another track. */
export async function clipMove(id: number, track: number, start: number): Promise<EngineStatus> {
  start = Math.round(start);
  if (inTauri) return invoke<EngineStatus>("clip_move", { id, track, start });
  const found = mockClipAt(id);
  found.track.clips.splice(found.index, 1);
  const dest = mockTrack(track);
  dest.clips.push({ ...found.clip, start });
  dest.clips.sort((a, b) => a.start - b.start);
  return mockEdit();
}

/** Sets a clip's start, offset into its file and length. */
export async function clipTrim(
  id: number,
  start: number,
  offset: number,
  length: number,
): Promise<EngineStatus> {
  start = Math.round(start);
  offset = Math.round(offset);
  length = Math.round(length);
  if (inTauri) return invoke<EngineStatus>("clip_trim", { id, start, offset, length });
  const found = mockClipAt(id);
  Object.assign(found.clip, { start, offset, length });
  found.track.clips.sort((a, b) => a.start - b.start);
  return mockEdit();
}

/** Cuts a clip at a timeline sample; the right part becomes a new clip. */
export async function clipSplit(id: number, at: number): Promise<EngineStatus> {
  at = Math.round(at);
  if (inTauri) return invoke<EngineStatus>("clip_split", { id, at });
  const { track, clip, index } = mockClipAt(id);
  if (at <= clip.start || at >= clip.start + clip.length) {
    throw new Error(`sample ${at} is not inside clip ${id}`);
  }
  const left = at - clip.start;
  track.clips.splice(index + 1, 0, {
    ...clip,
    id: mockNextId++,
    start: at,
    offset: clip.offset + left,
    length: clip.length - left,
  });
  clip.length = left;
  return mockEdit();
}

/**
 * A file in `audio/` as mono f32 samples, for the peak cache. The
 * stand-in synthesizes a plucked tone per file name so clips show a
 * waveform in a browser.
 */
export async function audioSamples(file: string): Promise<Float32Array> {
  if (inTauri) {
    const bytes = await invoke<ArrayBuffer>("audio_samples", { file });
    return new Float32Array(bytes);
  }
  let frames = MOCK_RATE;
  for (const t of mock.project.tracks) {
    for (const c of t.clips) {
      if (c.file === file) frames = Math.max(frames, c.offset + c.length);
    }
  }
  const out = new Float32Array(frames);
  let seed = 0;
  for (const ch of file) seed = (seed * 31 + ch.charCodeAt(0)) >>> 0;
  const hz = 82 + (seed % 7) * 18;
  for (let i = 0; i < frames; i++) {
    const t = i / MOCK_RATE;
    const pluck = Math.exp(-((t * 2.5) % 2.5) * 1.6);
    out[i] =
      pluck * 0.8 * (Math.sin(2 * Math.PI * hz * t) + 0.4 * Math.sin(2 * Math.PI * hz * 2.01 * t));
  }
  return out;
}

// ----- the transport -----

export async function transportPlay(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_play");
  mockTransport.playing = true;
  return mockStatus();
}

export async function transportRecord(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_record");
  if (!mock.project.tracks.some((t) => t.armed)) throw new Error("arm a track before recording");
  mockTransport.playing = true;
  mockTransport.recording = true;
  return mockStatus();
}

export async function transportStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_stop");
  if (mockTransport.recording) {
    for (const t of mock.project.tracks) {
      if (!t.armed) continue;
      const n = t.clips.length + 1;
      t.clips.push(mockClip(mockNextId++, `${t.name} ${n}`, 0, mockTransport.position));
    }
    mock.project.dirty = true;
  }
  mockTransport.playing = false;
  mockTransport.recording = false;
  return mockStatus();
}

/** Moves the playhead to a sample. */
export async function transportLocate(sample: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_locate", { sample: Math.round(sample) });
  mockTransport.position = sample / MOCK_RATE;
  return mockStatus();
}

// ----- the chain -----

export async function chainAdd(track: number, spec: string, index?: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_add", { track, spec, index: index ?? null });
  const chain = mockTrack(track).chain;
  chain.splice(index ?? chain.length, 0, mockPedal(mockNextId++, spec));
  return mockEdit();
}

export async function chainRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_remove", { id });
  for (const t of mock.project.tracks) t.chain = t.chain.filter((p) => p.id !== id);
  return mockEdit();
}

export async function chainMove(id: number, to: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_move", { id, to });
  for (const t of mock.project.tracks) {
    const from = t.chain.findIndex((p) => p.id === id);
    if (from < 0) continue;
    const [pedal] = t.chain.splice(from, 1);
    if (pedal) t.chain.splice(Math.min(to, t.chain.length), 0, pedal);
  }
  return mockEdit();
}

/** Sets a knob. Not echoed back as a status: the UI already shows the value. */
export async function chainSetParam(id: number, param: number, value: number): Promise<void> {
  if (inTauri) return invoke("chain_set_param", { id, param, value });
  for (const t of mock.project.tracks) {
    const p = t.chain.find((p) => p.id === id)?.params[param];
    if (p) p.value = value;
  }
  mock.project.dirty = true;
}

/**
 * Status changes the engine makes on its own, such as rebuilding after a
 * device vanished. Returns a function that stops listening.
 */
export function onStatus(listener: (status: EngineStatus) => void): () => void {
  if (!inTauri) {
    statusListeners.add(listener);
    return () => statusListeners.delete(listener);
  }
  let active = true;
  const stop = listen<EngineStatus>("engine://status", (e) => {
    if (active) listener(e.payload);
  });
  return () => {
    active = false;
    stop.then((unlisten) => unlisten());
  };
}

/**
 * Meter frames at display rate, as binary frames (see frame.ts). One
 * channel is opened per page and shared by every listener. Frames are
 * delivered on the next animation frame and acknowledged then, so the
 * shell never sends more than one frame per displayed frame: a hidden or
 * busy UI gets a fresh frame when it is ready, not a queue of stale ones.
 */
const meterListeners = new Set<(f: MeterFrame) => void>();
let meterChannelOpen = false;
let latest: ArrayBuffer | null = null;
let scheduled = false;

/** How many frames the shell reported skipping, for the stats readout. */
export let framesDropped = 0;

function deliver() {
  scheduled = false;
  const buffer = latest;
  latest = null;
  if (inTauri) invoke("meter_ack");
  if (buffer) publish(buffer);
}

function publish(buffer: ArrayBuffer) {
  let frame: ReturnType<typeof decodeFrame>;
  try {
    frame = decodeFrame(buffer);
  } catch (e) {
    console.error(e);
    return;
  }
  framesDropped = frame.dropped;
  const m = frame.meters;
  const tracks: Level[] = [];
  for (let i = 4; i + 1 < m.length; i += 2) tracks.push({ peak: m[i] ?? 0, rms: m[i + 1] ?? 0 });
  const out: MeterFrame = {
    input: { peak: m[0] ?? 0, rms: m[1] ?? 0 },
    output: { peak: m[2] ?? 0, rms: m[3] ?? 0 },
    tracks,
    position: frame.position,
    playing: frame.playing,
    recording: frame.recording,
  };
  for (const l of meterListeners) l(out);
}

/** A frame from the shell: shown on the next animation frame, then acknowledged. */
function receive(buffer: ArrayBuffer) {
  latest = buffer;
  if (!scheduled) {
    scheduled = true;
    requestAnimationFrame(deliver);
  }
}

function openMeterChannel() {
  if (meterChannelOpen) return;
  meterChannelOpen = true;
  if (inTauri) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = receive;
    invoke("subscribe_meters", { channel });
    return;
  }
  // The stand-in: a slow swell with a peak that leads the RMS, encoded
  // the way the shell encodes it so the decoder is exercised too.
  let t = 0;
  let sequence = 0;
  if (mockParams?.has("position")) mockTransport.position = Number(mockParams.get("position"));
  window.setInterval(() => {
    t += 1 / 60;
    if (mockTransport.playing) mockTransport.position += 1 / 60;
    const anySolo = mock.project.tracks.some((t) => t.strip.solo);
    const meters = mock.running
      ? [
          0.4 + 0.3 * Math.abs(Math.sin(t * 3.7)),
          0.18 + 0.12 * Math.sin(t * 2.1),
          0.55 + 0.35 * Math.abs(Math.sin(t * 3.7 + 0.4)),
          0.3 + 0.15 * Math.sin(t * 2.1 + 0.4),
          ...mock.project.tracks.flatMap((track, i) => {
            const silent = track.strip.mute || (anySolo && !track.strip.solo);
            const g = silent ? 0 : 10 ** (track.strip.gainDb / 20);
            return [
              g * (0.5 + 0.3 * Math.abs(Math.sin(t * (2.9 + i) + i))),
              g * (0.25 + 0.1 * Math.sin(t * (1.7 + i) + i)),
            ];
          }),
        ]
      : new Array(4 + mock.project.tracks.length * 2).fill(0);
    // No shell to acknowledge, and headless captures never reach an
    // animation frame, so the stand-in publishes directly.
    publish(
      encodeFrame({
        sequence: sequence++,
        dropped: 0,
        position: mockTransport.position,
        playing: mockTransport.playing,
        recording: mockTransport.recording,
        beat: null,
        meters,
      }),
    );
  }, 1000 / 60);
}

export function onMeters(listener: (frame: MeterFrame) => void): () => void {
  openMeterChannel();
  meterListeners.add(listener);
  return () => meterListeners.delete(listener);
}

/** Linear amplitude to decibels, floored so silence reads finite. */
export function toDb(linear: number): number {
  return 20 * Math.log10(Math.max(linear, 1e-5));
}

/** Seconds as `mm:ss.s`. */
export function clock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds - m * 60;
  return `${String(m).padStart(2, "0")}:${s.toFixed(1).padStart(4, "0")}`;
}
