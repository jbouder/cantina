import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import ampDef from "../../../pedals/amp.pedal.json";
import chorusDef from "../../../pedals/chorus.pedal.json";
import distortionDef from "../../../pedals/distortion.pedal.json";
import eqDef from "../../../pedals/eq.pedal.json";
import flangerDef from "../../../pedals/flanger.pedal.json";
import fuzzDef from "../../../pedals/fuzz.pedal.json";
import overdriveDef from "../../../pedals/overdrive.pedal.json";
import phaserDef from "../../../pedals/phaser.pedal.json";
import tremoloDef from "../../../pedals/tremolo.pedal.json";
import { ACTIONS, type ActionId, type ActionState } from "./actions";
import { type AnalysisFrame, analysisOf, decodeFrame, encodeFrame } from "./frame";
import {
  type BrokenFile,
  brokenOf,
  type LibraryKind,
  type LibraryPattern,
  type LibraryStatus,
  patternFileName,
} from "./library";
import { mockAnalysis, mockBeat } from "./mockAnalysis";

export type { BrokenFile, LibraryKind, LibraryPattern, LibraryStatus };
export { brokenOf, patternFileName };

import {
  disconnect,
  empty,
  type ParamDef,
  type PedalDef,
  type Problem,
  paramRange,
  validate,
} from "./pedal";

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
  /** The stock pedals `chainAdd` accepts, with how each is made. */
  pedalKinds: PedalKindInfo[];
  plugins: PluginsStatus;
  latency: LatencyStatus;
  /** The drum kits installed, for a drum track to pick from. */
  kits: KitInfo[];
  /** The cabinet impulse responses installed, for a cabinet pedal to pick from. */
  irs: IrInfo[];
  /** The pattern looping on a drum track for editing, if any. Not part of the project. */
  audition: AuditionStatus | null;
  /** The track the stage's analysis listens to, or null for the master. */
  analysisSource: number | null;
  /** The library folders: where they are, their pattern files, and what will not load. */
  library: LibraryStatus;
  metronome: MetronomeStatus;
}

/** The metronome's settings. Mirrors `MetronomeSettings` in src-tauri. */
export interface Metronome {
  /** Clicks on every beat while the transport plays. */
  on: boolean;
  /** One of `MetronomeStatus.sounds`. */
  sound: string;
  /** -40 to +6 dB. */
  levelDb: number;
}

/** The metronome and the sounds it offers. Mirrors `MetronomeStatus` in src-tauri. */
export interface MetronomeStatus extends Metronome {
  sounds: string[];
}

/** A kind of pedal the picker offers. Mirrors `PedalKindInfo` in src-tauri. */
export interface PedalKindInfo {
  /** What `chainAdd` takes. */
  spec: string;
  name: string;
  /** Ships with Cantina. */
  stock: boolean;
  /** Defined in the pedal format, so the designer can open it as a starting point. */
  designable: boolean;
}

/** What the designer gets back from an update. Mirrors `DesignerResult` in src-tauri. */
export interface DesignerResult {
  status: EngineStatus;
  /** What stopped the definition from being used; empty when it was. */
  problems: Problem[];
}

/** An installed kit. Mirrors `KitInfo` in src-tauri. */
export interface KitInfo {
  id: string;
  name: string;
}

/** An installed cabinet impulse response. Mirrors `IrInfo` in src-tauri. */
export interface IrInfo {
  id: string;
  name: string;
}

/** A pattern looping on a drum track. Mirrors `AuditionStatus` in src-tauri. */
export interface AuditionStatus {
  track: number;
  pattern: number;
}

/** A drum track's kit with the pads the grid shows rows for. Mirrors `KitStatus` in src-tauri. */
export interface KitStatus {
  id: string;
  name: string;
  pads: PadInfo[];
  /** True when no installed kit has this id; the track plays silence. */
  missing: boolean;
}

export interface PadInfo {
  id: string;
  name: string;
}

/** A step sequencer pattern. Mirrors `PatternInfo` in src-tauri. */
export interface PatternInfo {
  id: number;
  name: string;
  /** Steps per quarter note: 2, 4, 8, or 3 and 6 for triplets. */
  stepsPerBeat: number;
  /** Steps in the pattern, which loops after the last. */
  length: number;
  /** 0 straight; 1 delays every second step by a third of a step. */
  swing: number;
  /** One row per pad that has hits; `steps` holds `length` velocities. */
  rows: PatternRow[];
}

export interface PatternRow {
  pad: string;
  steps: number[];
}

/** Steps per beat a pattern may use, with their labels. */
export const RESOLUTIONS: { stepsPerBeat: number; label: string }[] = [
  { stepsPerBeat: 1, label: "1/4" },
  { stepsPerBeat: 2, label: "1/8" },
  { stepsPerBeat: 3, label: "1/8T" },
  { stepsPerBeat: 4, label: "1/16" },
  { stepsPerBeat: 6, label: "1/16T" },
  { stepsPerBeat: 8, label: "1/32" },
];

/** Where the time goes. Mirrors `LatencyStatus` in src-tauri. */
export interface LatencyStatus {
  /** The largest chain latency; every other track's playback is delayed to match. */
  compensationFrames: number;
  /** What CoreAudio reports for the open devices, each way. */
  inputFrames: number;
  outputFrames: number;
  /** Compensation above this many milliseconds is flagged. */
  warnMs: number;
  warn: boolean;
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
  /** Drum patterns, shared by every drum track. */
  patterns: PatternInfo[];
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
  /** The kit of a drum track; an audio track has none. */
  kit: KitStatus | null;
  chain: PedalInfo[];
  clips: ClipInfo[];
  /** Patterns on the timeline, for a drum track. Positions in ticks. */
  patternClips: PatternClipInfo[];
  strip: Strip;
  /** Frames the chain delays the signal by. */
  latencyFrames: number;
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

/** A pattern clip on a drum track. Positions are in ticks (960 per beat). */
export interface PatternClipInfo {
  id: number;
  pattern: number;
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
  /** Whether the plugin can open its own editor window. */
  hasEditor: boolean;
  editorOpen: boolean;
  /** Frames of delay this pedal adds. */
  latencyFrames: number;
  /**
   * Names of the values the pedal measures for its display, in the
   * order they appear in a frame's readouts (see readouts.ts).
   */
  readouts: string[];
  /** Out of the signal path, settings kept. */
  bypassed: boolean;
}

export interface ParamInfo {
  name: string;
  /** "dB", "Hz", "s", "ms", "%" or "". */
  unit: string;
  min: number;
  max: number;
  default: number;
  value: number;
  /** A switch: the value is 0 or 1. */
  toggle: boolean;
  /** For a pick from a list, the names; the value is an index. */
  choices: string[];
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
  /** The beat a count-in is on, from 1; zero with no count-in. */
  countIn: number;
  /** Every pedal readout, flat; `readoutsByPedal` splits them up. */
  readouts: ArrayLike<number>;
}

/** True inside the Tauri web view, false in a plain browser tab. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const SILENT: Level = { peak: 0, rms: 0 };

/*
 * In a browser (`npm run dev` without the shell) there is no engine, so the
 * UI talks to this stand-in instead. It keeps `vite dev` useful for layout
 * and skin work and is what the PR screenshots are taken from. `?running`
 * starts it with the stream open; `?armed`, `?take`, `?playing` and
 * `?recording` set up the project and transport; `?countin=<1-4>` holds a
 * count-in on that beat.
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

/** A fader parameter as the shell describes it. */
function p(
  name: string,
  unit: string,
  min: number,
  max: number,
  def: number,
  value = def,
): ParamInfo {
  return { name, unit, min, max, default: def, value, toggle: false, choices: [] };
}

function toggle(name: string, on: boolean): ParamInfo {
  return { ...p(name, "", 0, 1, on ? 1 : 0), toggle: true };
}

function choice(name: string, choices: string[], def: number): ParamInfo {
  return { ...p(name, "", 0, choices.length - 1, def), choices };
}

const NOTES = [
  "1/1",
  "1/2",
  "1/2T",
  "1/4",
  "1/4D",
  "1/4T",
  "1/8",
  "1/8D",
  "1/8T",
  "1/16",
  "1/16T",
  "1/32",
];

/** The knobs of a chorus, flanger or phaser. */
function modulation(rate: number, depth: number, feedback: number) {
  return [
    p("Rate", "Hz", 0.05, 10, rate),
    p("Depth", "%", 0, 100, depth),
    p("Feedback", "%", 0, 100, feedback),
    p("Mix", "%", 0, 100, 50),
    toggle("Sync", false),
    choice("Note", NOTES, 3),
  ];
}

/** The five knobs every drive pedal has. */
function drive(driveMax: number, driveDefault: number, level: number, curves: string[]) {
  return [
    p("Drive", "dB", 0, driveMax, driveDefault),
    p("Tone", "Hz", 500, 8000, 3000),
    p("Level", "dB", -24, 6, level),
    choice("Curve", curves, 0),
    choice("Oversampling", ["Off", "2×", "4×", "8×"], 2),
  ];
}

const MOCK_IRS: IrInfo[] = [
  { id: "1x12-open-back", name: "1x12 open back" },
  { id: "2x12-closed", name: "2x12 closed" },
  { id: "4x10-bass", name: "4x10 bass" },
  { id: "4x12-modern", name: "4x12 modern" },
];

/** The stock pedals that ship in the pedal format (`pedals/`). */
const MOCK_DESIGNABLE = [
  "overdrive",
  "distortion",
  "fuzz",
  "eq",
  "chorus",
  "flanger",
  "phaser",
  "tremolo",
  "amp",
];

/** The stock pedals, as `crates/dsp` describes them. */
const MOCK_STOCK: Record<string, { name: string; params: ParamInfo[]; readouts: string[] }> = {
  tuner: {
    name: "Tuner",
    params: [p("Reference", "Hz", 415, 466, 440), toggle("Mute", false)],
    readouts: ["Pitch", "Clarity"],
  },
  gate: {
    name: "Gate",
    params: [
      p("Threshold", "dB", -80, 0, -50),
      p("Attack", "ms", 0.1, 100, 1),
      p("Hold", "ms", 0, 1000, 50),
      p("Release", "ms", 1, 2000, 100),
    ],
    readouts: ["Open"],
  },
  compressor: {
    name: "Compressor",
    params: [
      p("Threshold", "dB", -60, 0, -20),
      p("Ratio", "", 1, 20, 4),
      p("Attack", "ms", 0.1, 200, 10),
      p("Release", "ms", 10, 2000, 100),
      p("Knee", "dB", 0, 24, 6),
      p("Makeup", "dB", 0, 24, 0),
    ],
    readouts: ["Reduction"],
  },
  overdrive: {
    name: "Overdrive",
    params: drive(40, 20, -6, ["Soft", "Asymmetric", "Hard"]),
    readouts: [],
  },
  distortion: {
    name: "Distortion",
    params: drive(60, 30, -12, ["Hard", "Soft", "Fold"]),
    readouts: [],
  },
  fuzz: {
    name: "Fuzz",
    params: drive(60, 40, -12, ["Silicon", "Germanium", "Octave"]),
    readouts: [],
  },
  eq: {
    name: "EQ",
    params: [
      [1, 100, 0.7],
      [0, 400, 1],
      [0, 1600, 1],
      [2, 5000, 0.7],
    ].flatMap(([kind, hz, q], i) => [
      choice(
        `Band ${String(i + 1)}`,
        ["Peak", "Low shelf", "High shelf", "Low cut", "High cut", "Notch"],
        kind ?? 0,
      ),
      p("Freq", "Hz", 20, 20000, hz ?? 1000),
      p("Gain", "dB", -18, 18, 0),
      p("Q", "", 0.1, 10, q ?? 1),
    ]),
    readouts: [],
  },
  chorus: { name: "Chorus", params: modulation(0.8, 50, 0), readouts: [] },
  flanger: { name: "Flanger", params: modulation(0.3, 70, 50), readouts: [] },
  phaser: { name: "Phaser", params: modulation(0.5, 70, 30), readouts: [] },
  tremolo: {
    name: "Tremolo",
    params: [
      p("Rate", "Hz", 0.05, 20, 5),
      p("Depth", "%", 0, 100, 60),
      choice("Shape", ["Sine", "Triangle", "Square"], 0),
      toggle("Sync", false),
      choice("Note", NOTES, 3),
    ],
    readouts: [],
  },
  delay: {
    name: "Delay",
    params: [
      p("Time", "ms", 1, 2000, 375),
      toggle("Sync", false),
      choice("Note", NOTES, 3),
      p("Feedback", "%", 0, 100, 35),
      p("Tone", "Hz", 500, 12000, 4000),
      p("Mix", "%", 0, 100, 35),
      choice("Mode", ["Mono", "Stereo", "Ping-pong"], 0),
    ],
    readouts: [],
  },
  reverb: {
    name: "Reverb",
    params: [
      p("Size", "%", 0, 100, 50),
      p("Decay", "s", 0.1, 20, 2),
      p("Damping", "%", 0, 100, 50),
      p("Pre-delay", "ms", 0, 200, 10),
      p("Mix", "%", 0, 100, 30),
    ],
    readouts: [],
  },
  amp: {
    name: "Amp",
    params: [
      p("Gain", "dB", 0, 40, 15),
      p("Bass", "dB", -12, 12, 0),
      p("Mid", "dB", -12, 12, 0),
      p("Treble", "dB", -12, 12, 0),
      p("Presence", "dB", -12, 12, 0),
      p("Level", "dB", -24, 12, -6),
    ],
    readouts: [],
  },
  cab: { name: "Cabinet", params: [p("Level", "dB", -24, 12, 0)], readouts: [] },
  gain: { name: "Gain", params: [p("Gain", "dB", -60, 24, 0)], readouts: [] },
};

/** The stock definitions, as the shell serves them. */
const MOCK_DEFS: Record<string, PedalDef> = {
  overdrive: overdriveDef as unknown as PedalDef,
  distortion: distortionDef as unknown as PedalDef,
  fuzz: fuzzDef as unknown as PedalDef,
  eq: eqDef as unknown as PedalDef,
  chorus: chorusDef as unknown as PedalDef,
  flanger: flangerDef as unknown as PedalDef,
  phaser: phaserDef as unknown as PedalDef,
  tremolo: tremoloDef as unknown as PedalDef,
  amp: ampDef as unknown as PedalDef,
};

/** Pedals designed in the stand-in, by the id in their `pedal:<id>` spec. */
const mockDesigns = new Map<string, PedalDef>();

/** The stand-in's library: `?library` fills it with two pedals. */
const mockLibrary: PedalKindInfo[] = mockParams?.has("library")
  ? [
      { spec: "pedal:warm-drive", name: "Warm Drive", stock: false, designable: true },
      { spec: "pedal:slapback", name: "Slapback", stock: false, designable: true },
    ]
  : [];
for (const entry of mockLibrary) {
  const def = structuredClone(
    entry.spec === "pedal:warm-drive" ? MOCK_DEFS.overdrive : MOCK_DEFS.chorus,
  ) as PedalDef;
  def.name = entry.name;
  mockDesigns.set(entry.spec.slice(6), def);
}

function mockKinds(): PedalKindInfo[] {
  return [
    ...Object.entries(MOCK_STOCK).map(([spec, kind]) => ({
      spec,
      name: kind.name,
      stock: true,
      designable: MOCK_DESIGNABLE.includes(spec),
    })),
    ...mockLibrary,
  ];
}

/** A parameter as the shell describes a designed pedal's. */
function paramFromDef(def: ParamDef, value?: number): ParamInfo {
  const [min, max] = paramRange(def);
  return {
    name: def.label || def.id,
    unit: def.choices?.length || def.toggle ? "" : (def.unit ?? ""),
    min,
    max,
    default: def.default,
    value: value ?? def.default,
    toggle: def.toggle ?? false,
    choices: [...(def.choices ?? [])],
  };
}

function mockPedal(id: number, spec: string): PedalInfo {
  if (spec.startsWith("pedal:")) {
    const def = mockDesigns.get(spec.slice(6)) ?? empty("New pedal");
    return {
      id,
      spec,
      name: def.name,
      params: def.params.map((p) => paramFromDef(p)),
      hasEditor: false,
      editorOpen: false,
      latencyFrames: 0,
      readouts: [],
      bypassed: false,
    };
  }
  if (spec.startsWith("au:")) {
    // Apple's AUDelay, as the shell describes it.
    return {
      id,
      spec,
      name: "AUDelay",
      params: [
        p("Dry/Wet Mix", "%", 0, 100, 50, 35),
        p("Delay Time", "s", 0, 2, 1, 0.375),
        p("Feedback", "%", -100, 100, 50, 40),
        p("Lowpass Cutoff", "Hz", 10, 22050, 15000, 6500),
      ],
      hasEditor: true,
      editorOpen: mockParams?.has("editor") ?? false,
      latencyFrames: mockParams?.has("latency") ? MOCK_LATENCY : 0,
      readouts: [],
      bypassed: false,
    };
  }
  // A cabinet's spec carries its impulse: `cab:<id>`; a bare `cab`
  // starts on the first one installed, as the shell does.
  const base = spec.startsWith("cab:") ? "cab" : spec;
  const kind = MOCK_STOCK[base] ?? MOCK_STOCK.gain;
  const resolved = spec === "cab" ? `cab:${MOCK_IRS[0]?.id ?? ""}` : spec;
  return {
    id,
    spec: MOCK_STOCK[base] ? resolved : "gain",
    name: kind?.name ?? "Gain",
    params: (kind?.params ?? []).map((q) => ({ ...q })),
    hasEditor: false,
    editorOpen: false,
    latencyFrames: 0,
    readouts: [...(kind?.readouts ?? [])],
    bypassed: false,
  };
}

/**
 * The guitar track's chain: `?chain=tuner,gate,compressor` names stock
 * pedals in order; `?plugin` appends the stand-in plugin;
 * `?design=overdrive` (or `new`) appends a designed pedal started from
 * that stock pedal, with `?broken` leaving its tone unwired so the
 * designer shows problems. `?bypassed=overdrive` turns those stock pedals off.
 */
function mockGuitarChain(): PedalInfo[] {
  const specs = mockParams?.get("chain")?.split(",").filter(Boolean) ?? ["gain", "overdrive"];
  const chain = specs.map((spec, i) => mockPedal(i === 0 ? 2 : i === 1 ? 3 : 30 + i, spec));
  if (mockParams?.has("plugin")) chain.push(mockPedal(9, "au:aufx:dely:appl"));
  const design = mockParams?.get("design");
  if (design) {
    let def = structuredClone(MOCK_DEFS[design] ?? empty("New pedal"));
    if (mockParams?.has("broken")) def = disconnect(def, "tone", "level");
    mockDesigns.set("draft-19", def);
    chain.push(mockPedal(19, "pedal:draft-19"));
  }
  const off = mockParams?.get("bypassed")?.split(",") ?? [];
  for (const pedal of chain) pedal.bypassed = off.includes(pedal.spec);
  return chain;
}

/** The stand-in plugin's latency with `?latency`: 32 ms, above the default warning. */
const MOCK_LATENCY = 1536;

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
const mockTransport = {
  position: 0,
  playing: false,
  recording: false,
  /** Seconds of count-in left. */
  countIn: 0,
};
const MOCK_COUNT_BEATS = 4;
const mockBeatSeconds = () => 60 / mock.project.tempo.bpm;
const mockProjects = ["Untitled", "Blues in A", "Riff ideas"];

const mockKits: KitInfo[] = [
  { id: "acoustic", name: "Acoustic" },
  { id: "rock", name: "Tight rock" },
  { id: "electronic", name: "Electronic" },
  { id: "metal", name: "Metal (synth)" },
];

const mockPads: PadInfo[] = [
  { id: "kick", name: "Kick" },
  { id: "snare", name: "Snare" },
  { id: "hh-closed", name: "Closed hat" },
  { id: "hh-open", name: "Open hat" },
  { id: "tom-lo", name: "Low tom" },
  { id: "tom-hi", name: "High tom" },
  { id: "crash", name: "Crash" },
  { id: "ride", name: "Ride" },
];

function mockKitStatus(id: string): KitStatus {
  const kit = mockKits.find((k) => k.id === id);
  return { id, name: kit?.name ?? id, pads: kit ? mockPads : [], missing: !kit };
}

/** A sixteen-step rock beat with a swung hat. */
function mockPattern(id: number, name: string): PatternInfo {
  const row = (pad: string, hits: Record<number, number>) => ({
    pad,
    steps: Array.from({ length: 16 }, (_, i) => hits[i] ?? 0),
  });
  return {
    id,
    name,
    stepsPerBeat: 4,
    length: 16,
    swing: 0.25,
    rows: [
      row("hh-closed", { 0: 0.8, 2: 0.5, 4: 0.8, 6: 0.5, 8: 0.8, 10: 0.5, 12: 0.8 }),
      row("hh-open", { 14: 0.7 }),
      row("kick", { 0: 1, 7: 0.6, 8: 0.9, 10: 0.5 }),
      row("snare", { 4: 1, 12: 1, 15: 0.3 }),
    ],
  };
}

/** The stand-in's drum track, with `?drums`. */
const MOCK_DRUMS = 10;

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
        kit: null,
        chain: mockGuitarChain(),
        clips: mockParams?.has("take")
          ? [mockClip(4, "Guitar 1", 0, 12.4), mockClip(5, "Guitar 2", 16, 6.1)]
          : [],
        patternClips: [],
        strip: { gainDb: -3, pan: -0.25, mute: false, solo: mockParams?.has("solo") ?? false },
        latencyFrames: mockParams?.has("latency") ? MOCK_LATENCY : 0,
      },
      {
        id: 6,
        name: "Bass",
        armed: false,
        recording: false,
        kit: null,
        chain: [mockPedal(7, "gain")],
        clips: mockParams?.has("take") ? [mockClip(8, "Bass 1", 2, 20)] : [],
        patternClips: [],
        strip: { gainDb: 0, pan: 0.1, mute: mockParams?.has("mute") ?? false, solo: false },
        latencyFrames: 0,
      },
      ...(mockParams?.has("drums")
        ? [
            {
              id: MOCK_DRUMS,
              name: "Drums",
              armed: false,
              recording: false,
              kit: mockKitStatus(mockParams.get("kit") ?? "acoustic"),
              chain: [],
              clips: [],
              // `?beats` places the patterns: two bars of Verse, then
              // two of Chorus starting a beat in.
              patternClips: mockParams.has("beats")
                ? [
                    { id: 13, pattern: 11, start: 0, offset: 0, length: 7680 },
                    { id: 14, pattern: 12, start: 7680, offset: 960, length: 7680 },
                    { id: 15, pattern: 11, start: 19200, offset: 0, length: 3840 },
                  ]
                : [],
              strip: { gainDb: -2, pan: 0, mute: false, solo: false },
              latencyFrames: 0,
            },
          ]
        : []),
    ],
    patterns: mockParams?.has("drums")
      ? [mockPattern(11, "Verse"), { ...mockPattern(12, "Chorus"), swing: 0 }]
      : [],
    master: { gainDb: -1, pan: 0, mute: false, solo: false },
  },
  pedalKinds: mockKinds(),
  plugins: {
    scanning: mockParams?.has("scanning") ?? false,
    count: 5,
    failed: 1,
    scannedAt: mockParams?.has("scanning") ? 0 : MOCK_SCANNED_AT,
  },
  latency: {
    compensationFrames: mockParams?.has("latency") ? MOCK_LATENCY : 0,
    inputFrames: 128 + 37,
    outputFrames: 128 + 29,
    warnMs: 20,
    warn: mockParams?.has("latency") ?? false,
  },
  kits: mockKits,
  irs: MOCK_IRS,
  audition: mockParams?.has("audition") ? { track: MOCK_DRUMS, pattern: 11 } : null,
  analysisSource: null,
  metronome: {
    on: mockParams?.has("metronome") ?? false,
    sound: mockParams?.get("metronome") || "Standard",
    levelDb: -6,
    sounds: ["Standard", "Woodblock", "Cowbell", "Drum", "Hi-hat"],
  },
  library: {
    pedalsDir: "/Users/you/Music/Cantina/Pedals",
    kitsDir: "/Users/you/Music/Cantina/Kits",
    patternsDir: "/Users/you/Music/Cantina/Patterns",
    patterns: mockParams?.has("library")
      ? [
          {
            name: "Half-time",
            path: "/Users/you/Music/Cantina/Patterns/Half-time.pattern.json",
            stepsPerBeat: 4,
            length: 32,
          },
        ]
      : [],
    broken: mockParams?.has("broken-files")
      ? [
          {
            kind: "pedal",
            name: "octave-fuzz.pedal.json",
            path: "/Users/you/Music/Cantina/Pedals/octave-fuzz.pedal.json",
            reason: 'invalid pedal: node "mix": nothing is connected to its input',
          },
          {
            kind: "kit",
            name: "lofi",
            path: "/Users/you/Music/Cantina/Kits/lofi",
            reason: 'pad "snare" names "snare-2.wav", which is not in the folder',
          },
          {
            kind: "pattern",
            name: "Shuffle.pattern.json",
            path: "/Users/you/Music/Cantina/Patterns/Shuffle.pattern.json",
            reason: 'the pattern row "kick" has 12 steps, the pattern 16',
          },
        ]
      : [],
  },
};
mockTransport.playing = mock.transport.playing || mock.transport.recording;
mockTransport.recording = mock.transport.recording;
if (mockParams?.has("countin")) {
  const beat = Number(mockParams.get("countin"));
  mockTransport.playing = false;
  mockTransport.recording = true;
  mockTransport.countIn = (MOCK_COUNT_BEATS + 1 - beat) * mockBeatSeconds() - 0.01;
}

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
    pedalKinds: mockKinds(),
    transport: { playing: mockTransport.playing, recording: mockTransport.recording },
    project: {
      ...mock.project,
      tracks: mock.project.tracks.map((t) => ({
        ...t,
        recording: mockTransport.recording && t.armed,
        kit: t.kit ? { ...t.kit, pads: t.kit.pads.map((p) => ({ ...p })) } : null,
        chain: t.chain.map((p) => ({ ...p, params: p.params.map((q) => ({ ...q })) })),
        clips: t.clips.map((c) => ({ ...c })),
        patternClips: t.patternClips.map((c) => ({ ...c })),
        strip: { ...t.strip },
      })),
      patterns: mock.project.patterns.map((p) => ({
        ...p,
        rows: p.rows.map((r) => ({ pad: r.pad, steps: [...r.steps] })),
      })),
      master: { ...mock.project.master },
    },
    audition: mock.audition ? { ...mock.audition } : null,
    metronome: { ...mock.metronome, sounds: [...mock.metronome.sounds] },
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

/** Sets the compensation above which the UI warns, in milliseconds. */
export async function latencyWarningSet(ms: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("latency_warning_set", { ms });
  const comp = (mock.latency.compensationFrames / MOCK_RATE) * 1000;
  mock.latency = { ...mock.latency, warnMs: ms, warn: comp > ms };
  mockPublish();
  return mockStatus();
}

/** Turns the metronome on or off and sets its sound and level. Not undoable. */
export async function metronomeSet(metronome: Metronome): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("metronome_set", { metronome });
  if (!mock.metronome.sounds.includes(metronome.sound)) {
    throw new Error(`the metronome has no sound called ${metronome.sound}`);
  }
  mock.metronome = {
    ...mock.metronome,
    on: metronome.on,
    sound: metronome.sound,
    levelDb: Math.min(6, Math.max(-40, metronome.levelDb)),
  };
  mockPublish();
  return mockStatus();
}

/** Frames at a rate as milliseconds, to one decimal. */
export function ms(frames: number, sampleRate: number): string {
  return `${((frames / sampleRate) * 1000).toFixed(1)} ms`;
}

/** The open route in one line, or why there is none. */
export function routeLabel(status: EngineStatus): string {
  if (!status.running) return "Engine stopped";
  const comp =
    status.latency.compensationFrames > 0
      ? ` · comp ${ms(status.latency.compensationFrames, status.sampleRate)}`
      : "";
  return `${status.inputName ?? "?"} → ${status.outputName ?? "?"} · ${status.sampleRate / 1000} kHz · ${status.bufferFrames} frames${comp}`;
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
        kit: null,
        chain: [mockPedal(2, "gain"), mockPedal(3, "overdrive")],
        clips: [],
        patternClips: [],
        strip: { ...UNITY },
        latencyFrames: 0,
      },
    ],
    patterns: [],
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

/**
 * Renames the open project: saves it and moves its folder. Not an edit,
 * so nothing to undo; the undo history stays.
 */
export async function projectRename(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_rename", { name });
  mockFreeName(name);
  const at = mockProjects.indexOf(mock.project.name);
  if (at >= 0) mockProjects.splice(at, 1, name.trim());
  mock.project.name = name.trim();
  mock.project.dir = `/Users/you/Music/Cantina/${name.trim()}.cantina`;
  mock.project.dirty = false;
  return mockStatus();
}

/**
 * Copies the open project as it is now to a new name and switches to the
 * copy, as Save As does; the original stays as last saved.
 */
export async function projectDuplicate(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_duplicate", { name });
  mockFreeName(name);
  mockProjects.push(name.trim());
  mock.project.name = name.trim();
  mock.project.dir = `/Users/you/Music/Cantina/${name.trim()}.cantina`;
  mock.project.dirty = false;
  return mockStatus();
}

function mockFreeName(name: string) {
  const n = name.trim();
  if (!n) throw new Error("a project needs a name");
  if (/[/\\:]/.test(n) || n.startsWith(".")) {
    throw new Error("a project name cannot contain / \\ or : or start with a dot");
  }
  if (mockProjects.includes(n)) throw new Error(`there is already a project called ${n}`);
}

/** Shows the open project's folder, or the library folder, in Finder. Nothing in a browser. */
export async function reveal(what: "project" | "library"): Promise<void> {
  if (inTauri) await invoke("reveal", { what });
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
    kit: null,
    chain: [],
    clips: [],
    patternClips: [],
    strip: { ...UNITY },
    latencyFrames: 0,
  });
  return mockEdit();
}

/** Adds a drum track playing `kit`, or the first installed kit. */
export async function trackAddDrums(kit?: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_add_drums", { kit: kit ?? null });
  const id = kit ?? mockKits[0]?.id ?? "acoustic";
  const drums = mock.project.tracks.filter((t) => t.kit).length;
  mock.project.tracks.push({
    id: mockNextId++,
    name: drums === 0 ? "Drums" : `Drums ${drums + 1}`,
    armed: false,
    recording: false,
    kit: mockKitStatus(id),
    chain: [],
    clips: [],
    patternClips: [],
    strip: { ...UNITY },
    latencyFrames: 0,
  });
  return mockEdit("add track");
}

export async function trackSetKit(id: number, kit: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_set_kit", { id, kit });
  mockTrack(id).kit = mockKitStatus(kit);
  return mockEdit("change kit");
}

/** Looks for installed kits again. */
export async function kitsRescan(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("kits_rescan");
  return mockStatus();
}

/** Swaps what a pedal is: a cabinet's impulse response (`cab:<id>`). */
export async function chainSetSpec(id: number, spec: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_set_spec", { id, spec });
  const p = mockPedalById(id);
  if (p) p.spec = spec;
  return mockEdit("change pedal");
}

/** Looks for installed impulse responses again. */
export async function irsRescan(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("irs_rescan");
  return mockStatus();
}

// ----- patterns -----

function mockPatternById(id: number): PatternInfo {
  const p = mock.project.patterns.find((p) => p.id === id);
  if (!p) throw new Error(`no pattern ${id}`);
  return p;
}

export async function patternAdd(name?: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_add", { name: name ?? null });
  mock.project.patterns.push({
    id: mockNextId++,
    name: name ?? `Pattern ${mock.project.patterns.length + 1}`,
    stepsPerBeat: 4,
    length: 16,
    swing: 0,
    rows: [],
  });
  return mockEdit("add pattern");
}

export async function patternRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_remove", { id });
  if (mock.project.tracks.some((t) => t.patternClips.some((c) => c.pattern === id))) {
    throw new Error("the pattern is on the timeline; delete its clips first");
  }
  mock.project.patterns = mock.project.patterns.filter((p) => p.id !== id);
  if (mock.audition?.pattern === id) mock.audition = null;
  return mockEdit("remove pattern");
}

export async function patternRename(id: number, name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_rename", { id, name });
  if (!name.trim()) throw new Error("a pattern needs a name");
  mockPatternById(id).name = name.trim();
  return mockEdit("rename pattern");
}

/** Sets one cell: a velocity from 0 (clear) to 1. */
export async function patternSetStep(
  id: number,
  pad: string,
  step: number,
  velocity: number,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_set_step", { id, pad, step, velocity });
  const p = mockPatternById(id);
  let row = p.rows.find((r) => r.pad === pad);
  if (!row) {
    if (velocity <= 0) return mockStatus();
    row = { pad, steps: new Array(p.length).fill(0) };
    p.rows.push(row);
    p.rows.sort((a, b) => a.pad.localeCompare(b.pad));
  }
  row.steps[step] = velocity;
  if (row.steps.every((v) => v === 0)) p.rows = p.rows.filter((r) => r !== row);
  return mockEdit(velocity > 0 ? "set step" : "clear step");
}

export async function patternSetLength(id: number, length: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_set_length", { id, length });
  const p = mockPatternById(id);
  p.length = length;
  for (const r of p.rows) {
    r.steps = Array.from({ length }, (_, i) => r.steps[i] ?? 0);
  }
  p.rows = p.rows.filter((r) => r.steps.some((v) => v > 0));
  return mockEdit("change pattern length");
}

/** Steps per beat: 1, 2, 3, 4, 6 or 8. */
export async function patternSetResolution(
  id: number,
  stepsPerBeat: number,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_set_resolution", { id, stepsPerBeat });
  mockPatternById(id).stepsPerBeat = stepsPerBeat;
  return mockEdit("change step resolution");
}

/** Swing from 0 to 1. Not echoed. */
export async function patternSetSwing(id: number, swing: number): Promise<void> {
  if (inTauri) return invoke("pattern_set_swing", { id, swing });
  mockPatternById(id).swing = swing;
  mock.project.dirty = true;
}

// ----- pattern clips -----

function mockPatternClipAt(id: number): {
  track: TrackStatus;
  clip: PatternClipInfo;
  index: number;
} {
  for (const track of mock.project.tracks) {
    const index = track.patternClips.findIndex((c) => c.id === id);
    const clip = track.patternClips[index];
    if (clip) return { track, clip, index };
  }
  throw new Error(`no pattern clip ${id}`);
}

function sortPatternClips(track: TrackStatus) {
  track.patternClips.sort((a, b) => a.start - b.start || a.id - b.id);
}

/** Puts a pattern on a drum track at `start` ticks, for `length` ticks or one pass. */
export async function patternClipAdd(
  track: number,
  pattern: number,
  start: number,
  length?: number,
): Promise<EngineStatus> {
  start = Math.round(start);
  if (inTauri) {
    return invoke<EngineStatus>("pattern_clip_add", {
      track,
      pattern,
      start,
      length: length ?? null,
    });
  }
  const t = mockTrack(track);
  if (!t.kit) throw new Error(`${t.name} is an audio track; patterns go on drum tracks`);
  const p = mockPatternById(pattern);
  t.patternClips.push({
    id: mockNextId++,
    pattern,
    start,
    offset: 0,
    length: length ?? (960 / p.stepsPerBeat) * p.length,
  });
  sortPatternClips(t);
  return mockEdit("place pattern");
}

export async function patternClipRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_clip_remove", { id });
  for (const t of mock.project.tracks) t.patternClips = t.patternClips.filter((c) => c.id !== id);
  return mockEdit("delete pattern clip");
}

export async function patternClipMove(
  id: number,
  track: number,
  start: number,
): Promise<EngineStatus> {
  start = Math.round(start);
  if (inTauri) return invoke<EngineStatus>("pattern_clip_move", { id, track, start });
  const found = mockPatternClipAt(id);
  const dest = mockTrack(track);
  if (!dest.kit) throw new Error(`${dest.name} is an audio track; patterns go on drum tracks`);
  found.track.patternClips.splice(found.index, 1);
  dest.patternClips.push({ ...found.clip, start });
  sortPatternClips(dest);
  return mockEdit("move pattern clip");
}

export async function patternClipTrim(
  id: number,
  start: number,
  offset: number,
  length: number,
): Promise<EngineStatus> {
  start = Math.round(start);
  offset = Math.round(offset);
  length = Math.round(length);
  if (inTauri) return invoke<EngineStatus>("pattern_clip_trim", { id, start, offset, length });
  const found = mockPatternClipAt(id);
  Object.assign(found.clip, { start, offset, length });
  sortPatternClips(found.track);
  return mockEdit("trim pattern clip");
}

/** The first position after `clip` where a copy clears every clip beside it. */
function mockFreeStart(
  clips: { start: number; length: number }[],
  clip: { start: number; length: number },
): number {
  let at = clip.start + clip.length;
  for (const c of [...clips].sort((a, b) => a.start - b.start)) {
    if (c.start >= at + clip.length) break;
    at = Math.max(at, c.start + c.length);
  }
  return at;
}

/** Copies a pattern clip to the first space after it that holds it. */
export async function patternClipDuplicate(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_clip_duplicate", { id });
  const { track, clip } = mockPatternClipAt(id);
  const start = mockFreeStart(track.patternClips, clip);
  track.patternClips.push({ ...clip, id: mockNextId++, start });
  sortPatternClips(track);
  return mockEdit("place pattern");
}

export async function patternClipSplit(id: number, at: number): Promise<EngineStatus> {
  at = Math.round(at);
  if (inTauri) return invoke<EngineStatus>("pattern_clip_split", { id, at });
  const { track, clip, index } = mockPatternClipAt(id);
  if (at <= clip.start || at >= clip.start + clip.length) {
    throw new Error(`tick ${at} is not inside pattern clip ${id}`);
  }
  const left = at - clip.start;
  track.patternClips.splice(index + 1, 0, {
    ...clip,
    id: mockNextId++,
    start: at,
    offset: clip.offset + left,
    length: clip.length - left,
  });
  clip.length = left;
  return mockEdit("split pattern clip");
}

/** Loops a pattern on a drum track while the transport plays; `null` stops. */
export async function patternAudition(
  track: number | null,
  pattern: number | null,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_audition", { track, pattern });
  mock.audition = track !== null && pattern !== null ? { track, pattern } : null;
  return mockStatus();
}

/** Takes a pedal out of the signal path, or puts it back. */
export async function pedalSetBypass(id: number, on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pedal_set_bypass", { id, on });
  const pedal = mockPedalById(id);
  if (!pedal) throw new Error(`no pedal ${id}`);
  pedal.bypassed = on;
  return mockEdit(on ? "bypass pedal" : "turn pedal on");
}

/** Plays the input through one audio track's chain: arms it and disarms the rest. */
export async function chainSelect(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_select", { id });
  for (const t of mock.project.tracks) if (!t.kit) t.armed = t.id === id;
  return mockEdit("switch chain");
}

/** Points the stage's analysis at a track, or the master with `null`. */
export async function analysisSetSource(track: number | null): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("analysis_set_source", { track });
  mock.analysisSource = track;
  return mockStatus();
}

export async function trackRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_remove", { id });
  mock.project.tracks = mock.project.tracks.filter((t) => t.id !== id);
  if (mock.analysisSource === id) mock.analysisSource = null;
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
/** Copies a clip to the first space after it that holds it. */
export async function clipDuplicate(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_duplicate", { id });
  const { track, clip } = mockClipAt(id);
  const start = mockFreeStart(track.clips, clip);
  track.clips.push({ ...clip, id: mockNextId++, start });
  track.clips.sort((a, b) => a.start - b.start);
  return mockEdit(`add clip ${clip.name}`);
}

/** A clip as Copy keeps it, to paste later: an audio clip's file and trim, or a pattern clip's pattern and trim in ticks. */
export type CopiedClip =
  | { kind: "audio"; name: string; file: string; offset: number; length: number }
  | { kind: "pattern"; pattern: number; offset: number; length: number };

/**
 * Pastes a copied clip onto `track` at `start` (samples for audio, ticks
 * for a pattern), or in the first space after it that holds the clip.
 */
export async function clipPaste(
  copied: CopiedClip,
  track: number,
  start: number,
): Promise<EngineStatus> {
  start = Math.max(0, Math.round(start));
  if (copied.kind === "audio") {
    const { name, file, offset, length } = copied;
    if (inTauri) {
      return invoke<EngineStatus>("clip_paste", { track, start, name, file, offset, length });
    }
    const t = mockTrack(track);
    if (t.kit) throw new Error(`${t.name} is a drum track; paste audio on an audio track`);
    const at = mockFreeStart(t.clips, { start: start - length, length });
    t.clips.push({ id: mockNextId++, name, file, start: at, offset, length });
    t.clips.sort((a, b) => a.start - b.start);
    return mockEdit(`add clip ${name}`);
  }
  const { pattern, offset, length } = copied;
  if (inTauri) {
    return invoke<EngineStatus>("pattern_clip_paste", { track, start, pattern, offset, length });
  }
  const t = mockTrack(track);
  if (!t.kit) throw new Error(`${t.name} is an audio track; paste a pattern on a drum track`);
  const at = mockFreeStart(t.patternClips, { start: start - length, length });
  t.patternClips.push({ id: mockNextId++, pattern, start: at, offset, length });
  sortPatternClips(t);
  return mockEdit("place pattern");
}

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
  mockTransport.countIn = MOCK_COUNT_BEATS * mockBeatSeconds();
  mockTransport.recording = true;
  return mockStatus();
}

export async function transportStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_stop");
  if (mockTransport.recording && mockTransport.countIn === 0) {
    for (const t of mock.project.tracks) {
      if (!t.armed) continue;
      const n = t.clips.length + 1;
      t.clips.push(mockClip(mockNextId++, `${t.name} ${n}`, 0, mockTransport.position));
    }
    mock.project.dirty = true;
  }
  mockTransport.playing = false;
  mockTransport.recording = false;
  mockTransport.countIn = 0;
  return mockStatus();
}

/** Moves the playhead to a sample. A count-in holds it until it ends. */
export async function transportLocate(sample: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("transport_locate", { sample: Math.round(sample) });
  if (mockTransport.countIn === 0) mockTransport.position = sample / MOCK_RATE;
  return mockStatus();
}

// ----- the chain -----

export async function chainAdd(track: number, spec: string, index?: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_add", { track, spec, index: index ?? null });
  const chain = mockTrack(track).chain;
  chain.splice(index ?? chain.length, 0, mockPedal(mockNextId++, spec));
  return mockEdit();
}

function mockPedalById(id: number): PedalInfo | undefined {
  for (const t of mock.project.tracks) {
    const p = t.chain.find((p) => p.id === id);
    if (p) return p;
  }
  return undefined;
}

// ----- the designer -----

/** Starts a new design at the end of a track's chain. */
export async function designerNew(track: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("designer_new", { track });
  const key = `draft-${mockNextId}`;
  mockDesigns.set(key, empty("New pedal"));
  mockTrack(track).chain.push(mockPedal(mockNextId++, `pedal:${key}`));
  return mockEdit("add pedal");
}

/**
 * Replaces a designed pedal's definition and parameter values. A
 * definition with problems changes nothing: the last valid pedal keeps
 * playing and the problems come back to show.
 */
export async function designerUpdate(
  id: number,
  definition: PedalDef,
  values: number[],
): Promise<DesignerResult> {
  if (inTauri) return invoke<DesignerResult>("designer_update", { id, definition, values });
  const problems = validate(definition);
  if (problems.length > 0) return { status: mockStatus(), problems };
  const pedal = mockPedalById(id);
  if (!pedal || !pedal.spec.startsWith("pedal:")) throw new Error(`pedal ${id} is not a design`);
  mockDesigns.set(pedal.spec.slice(6), structuredClone(definition));
  pedal.name = definition.name;
  pedal.params = definition.params.map((p, i) => paramFromDef(p, values[i]));
  return { status: mockEdit("change pedal knobs"), problems: [] };
}

/** The definition behind a designed pedal. */
export async function pedalDefinition(id: number): Promise<PedalDef> {
  if (inTauri) return invoke<PedalDef>("pedal_definition", { id });
  const pedal = mockPedalById(id);
  const def = pedal && mockDesigns.get(pedal.spec.slice(6));
  if (!def) throw new Error(`pedal ${id} is not a design`);
  return structuredClone(def);
}

/** A stock pedal's definition, to start a design from. */
export async function pedalStockDefinition(name: string): Promise<PedalDef> {
  if (inTauri) return invoke<PedalDef>("pedal_stock_definition", { name });
  const def = MOCK_DEFS[name];
  if (!def) throw new Error(`${name} is not a stock pedal in the pedal format`);
  return structuredClone(def);
}

/** Every problem with a definition; empty when it would compile. */
export async function pedalCheck(definition: PedalDef): Promise<Problem[]> {
  if (inTauri) return invoke<Problem[]>("pedal_check", { definition });
  return validate(definition);
}

/** An id made from a name, as the shell makes them. */
export function pedalSlug(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "pedal";
}

/**
 * Saves a designed pedal to the library under an id made from its
 * name, and the project's own copy; a draft's spec becomes that id.
 */
export async function designerSave(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("designer_save", { id });
  const pedal = mockPedalById(id);
  const def = pedal && mockDesigns.get(pedal.spec.slice(6));
  if (!pedal || !def) throw new Error(`pedal ${id} is not a design`);
  const slug = pedalSlug(def.name);
  mockDesigns.set(slug, structuredClone(def));
  if (!mockLibrary.some((k) => k.spec === `pedal:${slug}`)) {
    mockLibrary.push({ spec: `pedal:${slug}`, name: def.name, stock: false, designable: true });
  }
  pedal.spec = `pedal:${slug}`;
  return mockEdit("change pedal");
}

/** Writes a designed pedal's definition to `path`. */
export async function pedalExport(id: number, path: string): Promise<void> {
  if (inTauri) return invoke("pedal_export", { id, path });
  if (!mockPedalById(id)) throw new Error(`no pedal ${id}`);
}

/**
 * Copies a pedal file into the library and, with a track, adds it to
 * that track's chain.
 */
export async function pedalImport(path: string, track?: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pedal_import", { path, track: track ?? null });
  const stem = (path.split("/").pop() ?? "pedal").replace(/\.pedal\.json$/, "");
  const slug = pedalSlug(stem);
  const def = structuredClone(MOCK_DEFS.overdrive) as PedalDef;
  def.name = stem;
  mockDesigns.set(slug, def);
  if (!mockLibrary.some((k) => k.spec === `pedal:${slug}`)) {
    mockLibrary.push({ spec: `pedal:${slug}`, name: stem, stock: false, designable: true });
  }
  if (track !== undefined) mockTrack(track).chain.push(mockPedal(mockNextId++, `pedal:${slug}`));
  return mockEdit("add pedal");
}

/**
 * Copies WAV files into the project and puts them on an audio track at
 * `start`, one after another. A file at another sample rate is converted
 * in the copy; the original is never touched. Each file is one edit.
 */
export async function audioImport(
  paths: string[],
  track: number,
  start: number,
): Promise<EngineStatus> {
  start = Math.round(start);
  if (inTauri) return invoke<EngineStatus>("audio_import", { paths, track, start });
  const target = mockTrack(track);
  if (target.kit) throw new Error("audio goes on an audio track, not a drum track");
  let at = start;
  for (const path of paths) {
    const name = (path.split("/").pop() ?? "import").replace(/\.wav$/i, "");
    const clip = mockClip(mockNextId++, name, at / MOCK_RATE, 8);
    target.clips.push(clip);
    at += clip.length;
  }
  return mockEdit(`add clip ${paths.length === 1 ? "import" : "imports"}`);
}

/** Copies a kit folder into the user's kits after checking it loads. */
export async function kitImport(dir: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("kit_import", { dir });
  const id = dir.split("/").pop() ?? "kit";
  if (mockKits.some((k) => k.id === id))
    throw new Error(`there is already a kit folder called ${id}`);
  mockKits.push({ id, name: id });
  return mockStatus();
}

/** Asks for WAV files to import; empty when the user cancels. */
export async function chooseAudioPaths(): Promise<string[]> {
  if (!inTauri) return ["/Users/you/Desktop/Backing.wav"];
  const chosen = await openDialog({
    multiple: true,
    filters: [{ name: "WAV audio", extensions: ["wav", "wave"] }],
  });
  return chosen ?? [];
}

/** Asks for a kit folder, one holding a `kit.json`; null when the user cancels. */
export async function chooseKitDir(): Promise<string | null> {
  if (!inTauri) return "/Users/you/Downloads/Garage";
  return openDialog({ directory: true, multiple: false });
}

/** Looks for library pedals again. */
export async function pedalsRescan(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pedals_rescan");
  return mockStatus();
}

/**
 * Adds a `.pattern.json` to the project with a fresh id, renamed on a
 * clash. With a track, rows for pads its kit lacks are kept and named in
 * a notice.
 */
export async function patternImport(path: string, track?: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_import", { path, track: track ?? null });
  const base = (path.split("/").pop() ?? "Pattern").replace(/\.pattern\.json$/, "");
  const taken = new Set(mock.project.patterns.map((p) => p.name));
  let name = base;
  for (let n = 2; taken.has(name); n++) name = `${base} ${n}`;
  mock.project.patterns.push({ ...mockPattern(mockNextId++, name) });
  return mockEdit("import pattern");
}

/** Writes a pattern to a `.pattern.json` file. */
export async function patternExport(id: number, path: string): Promise<void> {
  if (inTauri) return invoke("pattern_export", { id, path });
  mockPatternById(id);
}

/** Scans the library folders again; the shell also does this when they change. */
export async function libraryRescan(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("library_rescan");
  return mockStatus();
}

const PATTERN_FILTER = [{ name: "Pattern", extensions: ["json"] }];

/** Asks where to write a pattern file; null when the user cancels. */
export async function choosePatternExportPath(name: string, dir: string): Promise<string | null> {
  if (!inTauri) return `${dir}/${patternFileName(name)}`;
  return saveDialog({ defaultPath: `${dir}/${patternFileName(name)}`, filters: PATTERN_FILTER });
}

/** Asks for pattern files to import; empty when the user cancels. */
export async function choosePatternImportPaths(dir: string): Promise<string[]> {
  if (!inTauri) return [`${dir}/Shuffle.pattern.json`];
  const chosen = await openDialog({ multiple: true, defaultPath: dir, filters: PATTERN_FILTER });
  return chosen ?? [];
}

const PEDAL_FILTER = [{ name: "Pedal", extensions: ["json"] }];

/** Asks where to write a pedal file; null when the user cancels. */
export async function chooseExportPath(name: string): Promise<string | null> {
  if (!inTauri) return `/Users/you/Desktop/${pedalSlug(name)}.pedal.json`;
  return saveDialog({ defaultPath: `${pedalSlug(name)}.pedal.json`, filters: PEDAL_FILTER });
}

/** Asks for pedal files to import; empty when the user cancels. */
export async function chooseImportPaths(): Promise<string[]> {
  if (!inTauri) return ["/Users/you/Downloads/Shared Drive.pedal.json"];
  const chosen = await openDialog({ multiple: true, filters: PEDAL_FILTER });
  return chosen ?? [];
}

/**
 * Files dropped on the window. Returns a function that stops
 * listening. Nothing in a browser.
 */
export function onFileDrop(listener: (paths: string[]) => void): () => void {
  if (!inTauri) return () => {};
  let stopped = false;
  let stop: (() => void) | null = null;
  getCurrentWebview()
    .onDragDropEvent((event) => {
      if (event.payload.type === "drop") listener(event.payload.paths);
    })
    .then((unlisten) => {
      if (stopped) unlisten();
      else stop = unlisten;
    });
  return () => {
    stopped = true;
    stop?.();
  };
}

/** Opens a plugin's native editor window, or brings it to the front. */
export async function editorOpen(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("editor_open", { id });
  const p = mockPedalById(id);
  if (p) p.editorOpen = p.hasEditor;
  return mockStatus();
}

export async function editorClose(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("editor_close", { id });
  const p = mockPedalById(id);
  if (p) p.editorOpen = false;
  return mockStatus();
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
 * Tells the menu bar which items apply, are checked and what they say.
 * The shell keeps the native items in step; nothing in a browser, which
 * has no menu bar.
 */
export async function menuState(items: ({ id: ActionId } & ActionState)[]): Promise<void> {
  if (inTauri) await invoke("menu_state", { items });
}

/**
 * A project chosen from File ▸ Open Recent, by name. Returns a function
 * that stops listening. Nothing in a browser.
 */
export function onOpenRecent(listener: (name: string) => void): () => void {
  if (!inTauri) return () => {};
  let active = true;
  const stop = listen<string>("menu://open-recent", (e) => {
    if (active) listener(e.payload);
  });
  return () => {
    active = false;
    stop.then((unlisten) => unlisten());
  };
}

/**
 * Menu bar items chosen, by action id. Returns a function that stops
 * listening. Nothing in a browser.
 */
export function onMenuAction(listener: (id: ActionId) => void): () => void {
  if (!inTauri) return () => {};
  let active = true;
  const stop = listen<string>("menu://action", (e) => {
    if (active && e.payload in ACTIONS) listener(e.payload as ActionId);
  });
  return () => {
    active = false;
    stop.then((unlisten) => unlisten());
  };
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
const analysisListeners = new Set<(f: AnalysisFrame) => void>();
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
    countIn: frame.countIn,
    readouts: frame.readouts,
  };
  for (const l of meterListeners) l(out);
  if (analysisListeners.size === 0) return;
  const analysis = analysisOf(frame);
  if (analysis) for (const l of analysisListeners) l(analysis);
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
    if (mockTransport.countIn > 0 && !mockParams?.has("countin")) {
      mockTransport.countIn = Math.max(0, mockTransport.countIn - 1 / 60);
      mockTransport.playing = mockTransport.countIn === 0;
    }
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
    // What each pedal would measure: a tuner hearing an A string a
    // touch sharp and drifting, a gate open, a compressor working.
    const readouts = mock.project.tracks.flatMap((track) =>
      track.chain.flatMap((pedal) =>
        pedal.readouts.map((name) => {
          if (!mock.running) return 0;
          switch (name) {
            case "Pitch":
              return 110 * 2 ** ((4 + 6 * Math.sin(t * 0.7)) / 1200);
            case "Clarity":
              return 0.95;
            case "Open":
              return 1;
            case "Reduction":
              return -(3 + 3 * Math.abs(Math.sin(t * 3.7)));
            default:
              return 0;
          }
        }),
      ),
    );
    const { bpm, numerator } = mock.project.tempo;
    const beat = mockTransport.playing ? mockBeat(mockTransport.position, bpm, numerator) : null;
    const analysis = mockAnalysis(t, beat, mock.running ? 1 : 0);
    // No shell to acknowledge, and headless captures never reach an
    // animation frame, so the stand-in publishes directly.
    publish(
      encodeFrame({
        sequence: sequence++,
        dropped: 0,
        position: mockTransport.position,
        playing: mockTransport.playing,
        recording: mockTransport.recording,
        countIn:
          mockTransport.countIn > 0
            ? MOCK_COUNT_BEATS + 1 - Math.ceil(mockTransport.countIn / mockBeatSeconds())
            : 0,
        beat,
        meters,
        readouts,
        spectrum: analysis.spectrum,
        waveform: analysis.waveform,
        analysis: { rms: analysis.rms, source: 1 },
      }),
    );
  }, 1000 / 60);
}

export function onMeters(listener: (frame: MeterFrame) => void): () => void {
  openMeterChannel();
  meterListeners.add(listener);
  return () => meterListeners.delete(listener);
}

/** The stage's input: spectrum, waveform, level and beat, at display rate. */
export function onAnalysis(listener: (frame: AnalysisFrame) => void): () => void {
  openMeterChannel();
  analysisListeners.add(listener);
  return () => analysisListeners.delete(listener);
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

// ----- mixdown and bounce -----

/** What an export writes and covers. */
export interface ExportOptions {
  /** Where to write; `.wav` is added when missing. */
  path: string;
  sampleRate: 44100 | 48000;
  /** 16 or 24-bit integers (dithered), or 32-bit float. */
  bits: 16 | 24 | 32;
  /** The first bar, from 1, or null for the start of the song. */
  fromBar: number | null;
  /** The last bar, included, or null for the end of the song. */
  toBar: number | null;
  /** Render past the end until reverbs and delays have rung out. */
  tail: boolean;
}

/** What an export wrote. */
export interface ExportResult {
  path: string;
  seconds: number;
  /** How long the render took; under `seconds` is faster than real time. */
  renderSeconds: number;
  peakDb: number;
  rmsDb: number;
}

/** How far an export or a bounce has got. */
export interface RenderProgress {
  what: "export" | "bounce";
  /** 0 to 1. */
  fraction: number;
}

const renderListeners = new Set<(p: RenderProgress) => void>();

/** Calls `listener` as a render progresses. Returns a function that stops listening. */
export function onRenderProgress(listener: (p: RenderProgress) => void): () => void {
  if (inTauri) {
    const off = listen<RenderProgress>("render-progress", (e) => listener(e.payload));
    return () => {
      off.then((f) => f());
    };
  }
  renderListeners.add(listener);
  return () => renderListeners.delete(listener);
}

let mockCancelled = false;

/** The stand-in's render: progress in steps, a little slower than instant. */
async function mockRender(what: RenderProgress["what"]): Promise<void> {
  mockCancelled = false;
  for (let i = 1; i <= 10; i++) {
    await new Promise((r) => setTimeout(r, 60));
    if (mockCancelled) throw new Error("cancelled");
    for (const l of renderListeners) l({ what, fraction: i / 10 });
  }
}

/** Renders the mix to a WAV file, faster than real time. */
export async function mixdownExport(options: ExportOptions): Promise<ExportResult> {
  if (inTauri) return invoke<ExportResult>("mixdown_export", { options });
  if (options.fromBar !== null && options.toBar !== null && options.toBar < options.fromBar) {
    throw new Error("the range is empty: the last bar must come after the first");
  }
  await mockRender("export");
  const path = options.path.toLowerCase().endsWith(".wav") ? options.path : `${options.path}.wav`;
  return { path, seconds: 32, renderSeconds: 1.4, peakDb: -1.2, rmsDb: -16.8 };
}

/**
 * Renders a track through its chain to one new clip in its place, as one
 * undoable edit; undo brings the chain and the clips back.
 */
export async function trackBounce(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_bounce", { id });
  const track = mockTrack(id);
  const rate = mock.project.sampleRate;
  const tickSamples = (ticks: number) => (ticks / 960) * (60 / mock.project.tempo.bpm) * rate;
  const spans: [number, number][] = track.kit
    ? track.patternClips.map((c) => [tickSamples(c.start), tickSamples(c.start + c.length)])
    : track.clips.map((c) => [c.start, c.start + c.length]);
  if (spans.length === 0) throw new Error(`${track.name} has nothing on it to bounce`);
  await mockRender("bounce");
  const start = Math.round(Math.min(...spans.map((s) => s[0])));
  const end = Math.round(Math.max(...spans.map((s) => s[1])));
  const name = `${track.name} bounce 1`;
  track.clips = [
    {
      id: mockNextId++,
      name,
      file: `${track.name.replace(/[^A-Za-z0-9]/g, "-")}-bounce-1.wav`,
      start,
      offset: 0,
      // The tail the render lets ring.
      length: end - start + Math.round(rate * 1.5),
    },
  ];
  track.chain = [];
  track.kit = null;
  track.patternClips = [];
  track.latencyFrames = 0;
  return mockEdit(`bounce ${track.name}`);
}

/** Stops the running export or bounce; its promise fails with "cancelled". */
export async function renderCancel(): Promise<void> {
  if (inTauri) return invoke("render_cancel");
  mockCancelled = true;
}

const WAV_FILTER = [{ name: "WAV audio", extensions: ["wav"] }];

/** Asks where to write a mixdown; null when the user cancels. */
export async function chooseMixdownPath(project: string): Promise<string | null> {
  const name = `${project.replace(/[/:]/g, "-")}.wav`;
  if (!inTauri) return `/Users/you/Music/${name}`;
  return saveDialog({ defaultPath: name, filters: WAV_FILTER });
}

// `?bounce` bounces the guitar track as the page opens, so a screenshot
// shows a track after a bounce.
if (mockParams?.has("bounce")) {
  setTimeout(() => trackBounce(1).then(mockPublish, () => {}), 0);
}

// ----- updates -----

/** A newer release than the one running. */
export interface UpdateInfo {
  version: string;
  /** The release notes, when the release has any. */
  notes: string | null;
}

/** The release that is running. */
export async function appVersion(): Promise<string> {
  if (inTauri) return getVersion();
  return "0.1.0";
}

let pendingUpdate: Update | null = null;

/**
 * Asks the release feed whether a newer version is out. Null when this
 * is the newest. The update manifest is signed; one that does not verify
 * is refused.
 */
export async function updateCheck(): Promise<UpdateInfo | null> {
  if (!inTauri) {
    return mockParams?.has("update")
      ? { version: "0.2.0", notes: "Faster exports and a new kit." }
      : null;
  }
  pendingUpdate = await check();
  return pendingUpdate
    ? { version: pendingUpdate.version, notes: pendingUpdate.body ?? null }
    : null;
}

/**
 * Downloads the update found by `updateCheck`, verifies its signature,
 * installs it over this copy and restarts into it. `onProgress` hears the
 * fraction downloaded, or null while the size is unknown.
 */
export async function updateInstall(onProgress: (fraction: number | null) => void): Promise<void> {
  if (!inTauri) {
    for (let i = 1; i <= 10; i++) {
      await new Promise((r) => setTimeout(r, 80));
      onProgress(i / 10);
    }
    return;
  }
  if (!pendingUpdate) throw new Error("no update to install; check again");
  let total: number | null = null;
  let done = 0;
  await pendingUpdate.downloadAndInstall((event) => {
    if (event.event === "Started") total = event.data.contentLength ?? null;
    if (event.event === "Progress") {
      done += event.data.chunkLength;
      onProgress(total ? Math.min(1, done / total) : null);
    }
  });
  await relaunch();
}
