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
import { profileTrim } from "./calibrate";
import { type DrumPart, partFromTrack, layout as partLayout, patternTicks } from "./drumPart";
import { type Exercise, exerciseTab, loopTicks } from "./exercise";
import { type AnalysisFrame, analysisOf, decodeFrame, encodeFrame } from "./frame";
import {
  type BrokenFile,
  brokenOf,
  type GrooveInfo,
  type LibraryKind,
  type LibraryPattern,
  type LibraryStatus,
  patternFileName,
} from "./library";
import { mockAnalysis, mockBeat } from "./mockAnalysis";
import { addVariation, passes } from "./patternTools";
import { sortTabNotes, tabPitch } from "./tabEdit";

export type { BrokenFile, GrooveInfo, LibraryKind, LibraryPattern, LibraryStatus };
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
  /** Record counts in four beats first. An app setting. */
  countIn: boolean;
  /** Open the last project at launch rather than show the start dialog. An app setting. */
  openLastAtLaunch: boolean;
  /** The assistant's connection, never its key. An app setting. */
  assistant: AssistantStatus;
  /** Hearing tabs and practice speed. Session state, not part of the project. */
  practice: Practice;
  /** The stock grooves that ship with the app, read-only, by genre then section. */
  grooves: StockGroove[];
  /** Every rig the picker offers: the stock ones, then the user's. */
  rigs: RigInfo[];
  /** Every jam the Jam view offers: the stock ones, then the user's. */
  jams: JamInfo[];
  /** Every exercise the practice view offers: the stock ones, then the user's. */
  exercises: ExerciseInfo[];
  /** Every course, the stock ones then the user's, with your progress. */
  courses: CourseInfo[];
  /** The rig heard on a track in place of its chain. Session state, never saved. */
  rigAudition: RigAudition | null;
  /** What the player plays through, and the trim it gives rigs. An app setting. */
  instrumentProfile: InstrumentProfileStatus;
  /** Rows muted or soloed on drum tracks, by track id. Session state, never saved. */
  drumRows: Record<number, DrumRows>;
  /** The jam, while jam mode is open. Session state, not part of the project. */
  jam: JamStatus | null;
  /**
   * The exercise, while practice is open. Practice is jam mode with the
   * exercise's phrase as its backing, so `jam` is there too.
   */
  practicing: PracticeStatus | null;
  /** The drum designer, while it is open. Session state: the part is not the project's until placed. */
  drumDesign: DrumDesignStatus | null;
}

/** What the drum designer plays round and round. Mirrors `DesignPlays` in src-tauri. */
export type DesignPlays = { kind: "section"; name: string } | { kind: "order"; from: number };

/** What plays where, in ticks. Mirrors `PlaySpan` in src-tauri. */
export interface PlaySpan {
  start: number;
  length: number;
  /** The section, by name. */
  section: string;
  /** The step of the order, for the whole song. */
  step: number | null;
}

/** The open drum designer. Mirrors `DrumDesignStatus` in src-tauri. */
export interface DrumDesignStatus {
  /** The drum track it was opened from. */
  track: number;
  part: DrumPart;
  plays: DesignPlays;
  /** The part differs from the track's: closing loses it. */
  changed: boolean;
  /** Ticks in a bar, at the project's time signature. */
  barTicks: number;
  /** The tick the loop starts on. */
  loopStart: number;
  /** The tick the loop wraps at. */
  loopEnd: number;
  /** What plays where, up to the loop's end. */
  spans: PlaySpan[];
  /** The pads of the kit the designer hears, in the kit's order. */
  pads: PadInfo[];
  /** The kit in the workbench, while it is open. */
  kitEdit: KitEditStatus | null;
  /** The track has a part the designer placed on it. */
  placed: boolean;
  /** Pattern clips on the track now. */
  trackClips: number;
}

/** Where Place puts the part. Mirrors `PlaceMode` in cantina-project. */
export type PlaceMode = "replace" | "append";

/** A layer of a kit's pad. Mirrors `Layer` in cantina-dsp. */
export interface KitLayer {
  file: string;
  velocity?: [number, number];
  gainDb?: number;
}

/** A pad as the kit format has it. Mirrors `Pad` in cantina-dsp. */
export interface KitPad {
  id: string;
  name: string;
  gainDb?: number;
  pan?: number;
  choke?: string;
  /** Semitones, −12 to 12. Kit format 2. */
  tune?: number;
  /** How much of each hit rings, 0.05 to 1. Kit format 2. */
  decay?: number;
  layers: KitLayer[];
}

/** A kit's `kit.json`. Mirrors `Kit` in cantina-dsp. */
export interface KitFile {
  version: number;
  name: string;
  pads: KitPad[];
}

/** One pad of a kit recipe. Mirrors `PadRecipe` in cantina-dsp. */
export interface PadRecipe {
  id: string;
  name?: string;
  voice: "kick" | "snare" | "hat" | "clap" | "tom" | "cowbell";
  pitch?: number;
  decay?: number;
  tone?: number;
  noise?: number;
  layers?: number;
  gainDb?: number;
  pan?: number;
  choke?: string;
}

/** A kit described as synthesis settings. Mirrors `KitRecipe` in cantina-dsp. */
export interface KitRecipe {
  $schema?: string;
  version: number;
  name: string;
  seed?: number;
  pads: PadRecipe[];
}

/** The kit in the drum designer's workbench. Mirrors `KitEditStatus` in src-tauri. */
export interface KitEditStatus {
  /** The kit it started from, by id. */
  from: string;
  kit: KitFile;
  /** The recipe it is made from, when it is. */
  recipe: KitRecipe | null;
  /** It differs from the kit it started from. */
  changed: boolean;
}

/** Where a pad's new samples come from. Mirrors `PadSource` in src-tauri. */
export type PadSource = { from: "file"; path: string } | { from: "kit"; kit: string; pad: string };

/** The exercise being practiced. Mirrors `PracticeStatus` in src-tauri. */
export interface PracticeStatus {
  /** The file, which names the exercise. */
  path: string;
  exercise: Exercise;
  /** The phrase as a tab, to read. */
  tab: Tab;
  /** The tempo last asked for. */
  tempo: number;
  looping: boolean;
  /** The drums are heard; false is the click alone. */
  groove: boolean;
  /** The rig the input plays through in place of the exercise's; null is its own. */
  rig: string | null;
  /** Passes before the tempo climbs a rung; 0 holds it. */
  climb: number;
  /** The fastest a whole pass has been played, from the log. */
  best: number | null;
  /** The last day it was practiced. */
  last: PracticeDay | null;
  /** Days in a row with any practice. */
  streak: number;
  /** Minutes practiced today, every exercise together. */
  todayMinutes: number;
  /** Ticks in one pass of the loop. */
  loopTicks: number;
  /** What the example's rig left out. */
  exampleMissing: string[];
}

/** What can change about practice besides the exercise and its tempo. */
export interface PracticeOptions {
  looping: boolean;
  groove: boolean;
  /** The rig the input plays through; null is the exercise's own. */
  rig: string | null;
  /** Passes before the tempo climbs a rung; 0 holds it. */
  climb: number;
}

/** A pattern a jam keeps of its own: a pattern without the project's id. */
export type JamPattern = Omit<PatternInfo, "id"> & { id?: number };

/** A jam the Jam view offers. Mirrors `JamInfo` in src-tauri. */
export interface JamInfo {
  name: string;
  genre: string;
  description: string;
  /** Ships with Cantina: read-only. */
  stock: boolean;
  /** The file, which names the jam. */
  path: string;
  tempo: number;
  /** `A minor`. */
  key: string;
}

/** What an exercise teaches. Mirrors `Teaches` in cantina-project. */
export type Teaches = "chord" | "change" | "scale" | "technique" | "riff" | "rhythm";

/** An exercise the practice view offers. Mirrors `ExerciseInfo` in src-tauri. */
export interface ExerciseInfo {
  name: string;
  instrument: Instrument;
  teaches: Teaches;
  style: string;
  /** Ships with Cantina: read-only. */
  stock: boolean;
  /** The file, which names the exercise. */
  path: string;
  /** `A minor`. */
  key: string;
  /** The tempo ladder, in beats per minute. */
  start: number;
  goal: number;
  step: number;
  /** The fastest a whole pass has been played, from the practice log. */
  best: number | null;
  /** The last day it was practiced, `YYYY-MM-DD`. */
  last: string | null;
}

/** A course and how far through it you are. Mirrors `CourseInfo` in src-tauri. */
export interface CourseInfo {
  name: string;
  description: string;
  instrument: Instrument;
  style: string;
  stock: boolean;
  path: string;
  steps: CourseStepInfo[];
  /** The first step not done, by index; null when the course is done. */
  next: number | null;
}

/** One step of a course. Mirrors `StepInfo` in src-tauri. */
export interface CourseStepInfo {
  /** The exercise's id, its file less `.exercise.json`. */
  exercise: string;
  name: string;
  /** Its file, when it is installed. */
  path: string | null;
  goal: number;
  note: string;
  best: number | null;
  done: boolean;
}

/** An exercise practiced on a day. Mirrors `PracticeDay` in cantina-project. */
export interface PracticeDay {
  exercise: string;
  /** `YYYY-MM-DD`. */
  date: string;
  /** The fastest a whole pass was played at; 0 when none finished. */
  tempo: number;
  minutes: number;
}

/** A key: its tonic and whether it is major or minor. Mirrors `Key` in cantina-project. */
export interface JamKey {
  /** A note name: `A`, `F#`, `Bb`. */
  tonic: string;
  mode: "major" | "minor";
}

/** What a jam is. Mirrors `Jam` in cantina-project. */
export interface Jam {
  version: number;
  name: string;
  genre?: string;
  description?: string;
  /** Beats per minute, 30 to 300. */
  tempo: number;
  timeSignature: { numerator: number; denominator: number };
  key: JamKey;
  drums: {
    /** The kit, by id. */
    kit: string;
    /** A stock groove, by its genre; each section plays its pattern. */
    groove?: string | null;
    /** Patterns of the jam's own, by section, in place of the groove's; files have no ids. */
    patterns?: Partial<Record<GrooveSection, JamPattern>>;
    /** The drums' level in dB, −60 to 12. */
    levelDb: number;
  };
  /** The rig the input plays through, by its file's name less `.rig.json`. */
  rig?: string | null;
  /** What plays with the drums; absent is the drums alone. */
  backing?: JamBacking;
}

/** How often a progression's chords are played. Mirrors `Rhythm` in cantina-project. */
export type JamRhythm = "whole" | "half" | "quarter" | "eighth";

/** What plays with a jam's drums. Mirrors `Backing` in cantina-project. */
export type JamBacking =
  | { kind: "none" }
  | {
      kind: "progression";
      /** Roman numerals or chord names, `|` between bars. */
      chords: string;
      instrument: Instrument;
      rhythm: JamRhythm;
      /** The rig the voice plays through, by file name less `.rig.json`. */
      rig?: string | null;
      levelDb: number;
    }
  | { kind: "audio"; file: string; bpm: number; levelDb: number }
  | { kind: "track"; project: string; track: string; levelDb: number };

/** A stock chord progression. Mirrors `PresetInfo` in src-tauri. */
export interface JamPreset {
  genre: string;
  name: string;
  chords: string;
}

/** A project's audio track a jam can play. Mirrors `JamTrackInfo` in src-tauri. */
export interface JamTrack {
  name: string;
  /** The project's tempo, which the jam takes. */
  bpm: number;
  seconds: number;
}

/** Where a jam's section or tempo changes. Mirrors `JamMark` in src-tauri. */
export interface JamMark {
  sample: number;
  tick: number;
  section: GrooveSection;
  bpm: number;
}

/** The open jam. Mirrors `JamStatus` in src-tauri. */
export interface JamStatus {
  setup: Jam;
  /** The sections the drums have a pattern for, in order. */
  sections: GrooveSection[];
  /** Each section's length in bars. */
  bars: Partial<Record<GrooveSection, number>>;
  /** Where the section or tempo changes, and is going to. */
  marks: JamMark[];
  /** The sample an ending stops the jam on, when one is coming. */
  end: number | null;
  barTicks: number;
  /** The tempo last asked for. */
  tempo: number;
  /** Rig pedals left out because they are not installed. */
  missing: string[];
  /** What the backing left out: pedals, files. */
  backingMissing: string[];
  /** The tempo is the backing's, which the jam follows. */
  tempoLocked: boolean;
  /** The stock chord progressions, by genre. */
  presets: JamPreset[];
  /** Minutes of the input kept for Keep that: 1, 2, 5 or 10. An app setting. */
  captureMinutes: number;
  /** The last take kept, waiting to go into a project. */
  kept: JamKept | null;
}

/** A take Keep that wrote. Mirrors `KeptStatus` in src-tauri. */
export interface JamKept {
  seconds: number;
  /** The dry input, in `Jams/Takes`. */
  dry: string;
  /** Through the rig. */
  processed: string;
}

/** Where a kept take goes. Mirrors `KeepInto` in src-tauri. */
export type KeepInto = { into: "open" } | { into: "new"; name: string } | { into: "takes" };

/** A drum track's rows muted or soloed for listening while editing. Mirrors `RowMix` in src-tauri. */
export interface DrumRows {
  muted: string[];
  soloed: string[];
}

/** How hot an instrument's pickups are. */
export type Pickups = "singleCoil" | "humbucker" | "active";

/** What the player plays through. Mirrors `InstrumentProfile` in src-tauri. */
export interface InstrumentProfile {
  instrument: Instrument;
  pickups: Pickups;
  /** The open strings from the lowest, as MIDI notes. */
  tuning: number[];
  /** The input's peak playing hard, in dBFS; null until calibrated. */
  calibratedPeakDb: number | null;
}

/** The profile and the trim it gives every rig's input. */
export interface InstrumentProfileStatus extends InstrumentProfile {
  trimDb: number;
}

/** A rig the picker offers. Mirrors `RigInfo` in src-tauri. */
export interface RigInfo {
  name: string;
  genre: string;
  description: string;
  instrument: Instrument;
  /** Ships with Cantina: read-only. */
  stock: boolean;
  /** The file, which names the rig. */
  path: string;
  /** The pedals by spec, first to last. */
  chain: string[];
}

/** A rig heard on a track in place of its chain. Mirrors `RigAuditionStatus` in src-tauri. */
export interface RigAudition {
  track: number;
  path: string;
  /** Hearing the rig (A) rather than the track's own chain (B). */
  hearingRig: boolean;
  /** The rig's pedals that are not installed and are left out. */
  missing: string[];
}

/** What a saved rig says about itself. Mirrors `RigDraft` in src-tauri. */
export interface RigDraft {
  name: string;
  genre: string;
  description: string;
  instrument: Instrument;
  /** Overwrite a rig of the same name. */
  replace?: boolean;
}

/** The part of a song a groove is for. */
export type GrooveSection = "intro" | "verse" | "chorus" | "bridge" | "fill" | "ending";

/** A stock groove: a read-only pattern file and what it is for. Mirrors `StockGroove` in src-tauri. */
export interface StockGroove {
  name: string;
  /** The file, which `patternImport` copies into the project. */
  path: string;
  stepsPerBeat: number;
  length: number;
  genre: string;
  section: GrooveSection;
  timeSignature: { numerator: number; denominator: number };
  tempo: { min: number; max: number };
  feel: "straight" | "shuffle" | "swing" | "half-time" | "double-time";
}

/** How to practice with a tab. Mirrors `Practice` in src-tauri. */
export interface Practice {
  /** Play every track's tabs through the plucked-string voice, into the track's pedals. */
  voice: boolean;
  /** Silence the recordings on tracks that have a tab; drums and other tracks play on. */
  muteRecordings: boolean;
  /** 0.5 to 1. Below 1 the click, drums and tab voice slow down and recordings fall silent. */
  speed: number;
}

/** The assistant's endpoint and model. Mirrors `AssistantSettings` in src-tauri. */
export interface AssistantSettings {
  /** Nothing shows and nothing is sent until this is on. */
  enabled: boolean;
  /** An OpenAI-compatible API's base URL, such as `https://api.openai.com/v1`. */
  endpoint: string;
  /** The model's id at the endpoint. */
  model: string;
  /** Talks as Tapster, the house droid; off gives plain replies. */
  persona: boolean;
  /** Beeps and chirps on a reply and on an error, in the window only. */
  sounds: boolean;
}

/** The assistant as the shell reports it. Mirrors `AssistantStatus` in src-tauri. */
export interface AssistantStatus extends AssistantSettings {
  /** An API key is saved in the Keychain. The key itself never comes back. */
  hasKey: boolean;
}

/** What kind of thing went wrong talking to the provider. Mirrors `ProblemKind` in src-tauri. */
export type AssistantProblemKind =
  | "off"
  | "badUrl"
  | "badKey"
  | "unknownModel"
  | "rateLimited"
  | "offline"
  | "server"
  | "other";

/** A failed request to the provider, in plain words. Mirrors `Problem` in src-tauri. */
export interface AssistantProblem {
  kind: AssistantProblemKind;
  message: string;
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
  /** The project pattern looping, if it is one. */
  pattern: number | null;
  /** The stock groove's file looping, if it is one. */
  groove: string | null;
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
  /** How far hits stray, for rows without their own. Absent for on the grid. */
  humanize?: Humanize;
  /** Seeds humanize and chance; absent is 0. */
  seed?: number;
}

export interface PatternRow {
  pad: string;
  steps: number[];
  /** This row's own swing, instead of the pattern's. */
  swing?: number;
  /** This row's own humanize, instead of the pattern's. */
  humanize?: Humanize;
  /** Steps that play by chance, ratchet or flam, in step order. */
  marks?: StepMark[];
}

/** How far hits stray: 0 to 1. Mirrors `Humanize` in cantina-project. */
export interface Humanize {
  /** Up to 48 ticks early or late at 1. */
  timing: number;
  /** Up to 30% softer or harder at 1. */
  velocity: number;
}

/** What a step does beyond its velocity. Mirrors `StepMark`; fields left out are plain. */
export interface StepMark {
  step: number;
  /** 0 to 1; absent is 1. */
  chance?: number;
  /** 1 to 4 hits in the step; absent is 1. */
  ratchet?: number;
  /** A grace note just before. */
  flam?: boolean;
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
  /** The loop region, and whether playback loops over it. */
  loop: LoopRegion | null;
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

/** A project in the start dialog's recent list. Mirrors `RecentProject` in src-tauri. */
export interface RecentProject {
  name: string;
  /** When its project.json was last written, in milliseconds since the epoch. */
  saved: number | null;
  /** Moved or deleted since it was open. */
  missing: boolean;
}

/** The tracks a new project starts with. Mirrors `Start` in src-tauri. */
export type ProjectStart = "empty" | "guitar" | "guitarBass" | "guitarDrums";

/** What the start dialog asks of a new project. Mirrors `NewProject` in src-tauri. */
export interface NewProjectOptions {
  sampleRate: number;
  tempo: Tempo;
  start: ProjectStart;
}

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
  /** A drum track's pad mixer, by pad id; a pad not here is as the kit has it. */
  pads: Record<string, PadMix>;
}

/** A clip. Positions are in samples at the project's rate. */
export interface ClipInfo {
  id: number;
  name: string;
  file: string;
  start: number;
  offset: number;
  length: number;
  /** The clip's tab, in brief; none until it is transcribed. */
  tab?: TabSummary | null;
}

/** Guitar or bass. Mirrors `Instrument` in cantina-project. */
export type Instrument = "guitar" | "bass";

/** A clip's tab as the clip list shows it. Mirrors `TabSummary` in the shell. */
export interface TabSummary {
  instrument: Instrument;
  /** Open strings, lowest first, as MIDI notes. */
  tuning: number[];
  /** The capo's fret; 0 for none. */
  capo: number;
  /** Notes in the tab. */
  notes: number;
  /** True when it came from a transcription and can be re-quantized. */
  transcribed: boolean;
}

/** A note in a tab. Mirrors `TabNote` in cantina-project; techniques that are off are left out. */
export interface TabNote {
  /** 1 for the highest-pitched string. */
  string: number;
  /** Counted from the capo. */
  fret: number;
  /** Ticks from the tab's start, 960 per quarter note. */
  start: number;
  length: number;
  velocity?: number;
  bend?: { semitones: number; release?: boolean };
  slide?: boolean;
  hammerOn?: boolean;
  pullOff?: boolean;
  palmMute?: boolean;
  letRing?: boolean;
  vibrato?: boolean;
  harmonic?: boolean;
  dead?: boolean;
  tap?: boolean;
}

/** A clip's whole tab. Mirrors `Tab` in cantina-project. */
export interface Tab {
  name: string;
  instrument: Instrument;
  /** Open strings, lowest first, as MIDI notes. */
  tuning: number[];
  capo?: number;
  /** The tempo the ticks were written against. */
  tempo: Tempo;
  /** The frame of the clip's file where tick 0 falls. */
  origin?: number;
  notes: TabNote[];
}

/** The grid a transcription quantizes to: eighths, sixteenths or their triplets. */
export type TabGrid = "1/8" | "1/16" | "1/8t" | "1/16t";

/** What a transcription is asked for. Mirrors `TabSettings` in the shell. */
export interface TabSettings {
  instrument: Instrument;
  /** Open strings, lowest first, as MIDI notes. */
  tuning: number[];
  capo: number;
  grid: TabGrid;
}

/** A region of the timeline playback can loop over, in samples. Mirrors `LoopRegion` in cantina-project. */
export interface LoopRegion {
  start: number;
  /** The sample just after the loop. */
  end: number;
  /** True while playback loops; the region stays when it is off. */
  on: boolean;
}

/** A pattern clip on a drum track. Positions are in ticks (960 per beat). */
export interface PatternClipInfo {
  id: number;
  pattern: number;
  start: number;
  offset: number;
  length: number;
  /** Bars where another pattern plays: a fill. Absent for none. */
  fill?: PatternFill;
}

/** One pad's place in a drum track's mix. Mirrors `PadMix` in cantina-project. */
export interface PadMix {
  /** −60 to 12 dB. */
  levelDb: number;
  /** −1 to 1, added to the kit's. */
  pan: number;
  /** −12 to 12 semitones. */
  tune: number;
  /** How much of each hit rings, 0.05 to 1. */
  decay: number;
  mute: boolean;
}

/** A pad as the kit has it. */
export const PAD_MIX_DEFAULT: PadMix = { levelDb: 0, pan: 0, tune: 0, decay: 1, mute: false };

/** When a pattern clip plays a fill. Mirrors `Fill` in cantina-project. */
export interface PatternFill {
  /** The pattern played on fill bars. */
  pattern: number;
  /** The last bar of every this many is a fill; 0 for none. */
  every: number;
  /** The clip's last bar is a fill. */
  atEnd: boolean;
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
  /**
   * Run output only: no input device is opened, for playing along through
   * an amp in the room. Recording and Keep that need an input.
   */
  noInput: boolean;
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
 * count-in on that beat; `?tabbed` (with `?take`) gives the first guitar
 * clip a transcribed tab.
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
  noInput: false,
  inputUid: null,
  outputUid: null,
  sampleRate: 48000,
  bufferFrames: 128,
};

const mockParams = inTauri ? null : new URLSearchParams(window.location.search);
if (mockParams?.has("noinput")) mockSettings.noInput = true;
const MOCK_RATE = 48000;
/** The stand-in assistant's fake provider. */
const MOCK_ENDPOINT = "https://droids.example/v1";
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
    p("Blend", "%", 0, 100, 100),
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
  pitch: {
    name: "Pitch",
    params: [
      p("Two down", "%", 0, 100, 0),
      p("Octave down", "%", 0, 100, 100),
      p("Octave up", "%", 0, 100, 0),
      p("Semitones", "", -12, 12, 7),
      p("Shift", "%", 0, 100, 0),
      p("Mix", "%", 0, 100, 50),
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
/** `?tracks=<n>`: how many audio tracks the stand-in project has; two by default. */
const mockTrackCount = Math.max(1, Number(mockParams?.get("tracks")) || 2);

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
    tab: null,
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
/** The stock grooves, read from the repository's `patterns/` at build time. */
const GROOVE_FILES = import.meta.glob<{
  name: string;
  stepsPerBeat: number;
  length: number;
  swing: number;
  rows: PatternRow[];
  groove: Omit<StockGroove, "name" | "path" | "stepsPerBeat" | "length">;
}>("../../../patterns/*/*.pattern.json", { eager: true, import: "default" });

const SECTION_ORDER: GrooveSection[] = ["intro", "verse", "chorus", "bridge", "fill", "ending"];

const mockGrooves: StockGroove[] = Object.entries(GROOVE_FILES)
  .map(([file, g]) => ({
    name: g.name,
    path: `/Applications/Cantina.app/Contents/Resources/${file.replace(/^(\.\.\/)+/, "")}`,
    stepsPerBeat: g.stepsPerBeat,
    length: g.length,
    ...g.groove,
  }))
  .sort(
    (a, b) =>
      a.genre.localeCompare(b.genre) ||
      SECTION_ORDER.indexOf(a.section) - SECTION_ORDER.indexOf(b.section) ||
      a.name.localeCompare(b.name),
  );

/** The stock jams, read from the repository's `jams/` at build time. */
const JAM_FILES = import.meta.glob<Jam>("../../../jams/*.jam.json", {
  eager: true,
  import: "default",
});

function jamInfo(jam: Jam, path: string, stock: boolean): JamInfo {
  return {
    name: jam.name,
    genre: jam.genre ?? "",
    description: jam.description ?? "",
    stock,
    path,
    tempo: jam.backing?.kind === "audio" ? jam.backing.bpm : jam.tempo,
    key: `${jam.key.tonic} ${jam.key.mode}`,
  };
}

const mockStockJamFiles = new Map<string, Jam>(
  Object.entries(JAM_FILES).map(([file, jam]) => [
    `/Applications/Cantina.app/Contents/Resources/${file.replace(/^(\.\.\/)+/, "")}`,
    jam,
  ]),
);
const mockUserJams: { info: JamInfo; file: Jam }[] = [];

function mockJams(): JamInfo[] {
  const stock = [...mockStockJamFiles].map(([path, jam]) => jamInfo(jam, path, true));
  const by = (a: JamInfo, b: JamInfo) =>
    a.genre.toLowerCase().localeCompare(b.genre.toLowerCase()) ||
    a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  return [...stock.sort(by), ...mockUserJams.map((j) => j.info).sort(by)];
}

/** The stock exercises, read from the repository's `exercises/` at build time. */
const EXERCISE_FILES = import.meta.glob<Exercise>("../../../exercises/*.exercise.json", {
  eager: true,
  import: "default",
});

function mockExercises(): ExerciseInfo[] {
  const lower = (s: string) => s.toLowerCase();
  return Object.entries(EXERCISE_FILES)
    .map(([file, e]) => ({
      name: e.name,
      instrument: e.instrument,
      teaches: e.teaches,
      style: e.style,
      stock: true,
      path: `/Applications/Cantina.app/Contents/Resources/${file.replace(/^(\.\.\/)+/, "")}`,
      key: `${e.key.tonic} ${e.key.mode}`,
      start: e.tempo.start,
      goal: e.tempo.goal,
      step: e.tempo.step,
      best: mockBest(file),
      last: mockLastDay(file)?.date ?? null,
    }))
    .sort(
      (a, b) =>
        lower(a.style).localeCompare(lower(b.style)) ||
        a.teaches.localeCompare(b.teaches) ||
        lower(a.name).localeCompare(lower(b.name)),
    );
}

/** A course file as the stand-in reads it. */
interface CourseFile {
  name: string;
  description?: string;
  instrument: Instrument;
  style: string;
  steps: { exercise: string; goal: number; note?: string }[];
}

/** The stock courses, read from the repository's `courses/` at build time. */
const COURSE_FILES = import.meta.glob<CourseFile>("../../../courses/*.course.json", {
  eager: true,
  import: "default",
});

function mockCourses(): CourseInfo[] {
  const exercises = mockExercises();
  return Object.entries(COURSE_FILES).map(([file, c]) => {
    const steps = c.steps.map((s) => {
      const e = exercises.find((x) => mockId(x.path) === s.exercise);
      const best = e?.best ?? null;
      return {
        exercise: s.exercise,
        name: e?.name ?? s.exercise,
        path: e?.path ?? null,
        goal: s.goal,
        note: s.note ?? "",
        best,
        done: best !== null && best >= s.goal,
      };
    });
    const next = steps.findIndex((s) => !s.done);
    return {
      name: c.name,
      description: c.description ?? "",
      instrument: c.instrument,
      style: c.style,
      stock: true,
      path: `/Applications/Cantina.app/Contents/Resources/${file.replace(/^(\.\.\/)+/, "")}`,
      steps,
      next: next < 0 ? null : next,
    };
  });
}

/** A rig file as the stand-in reads it. */
interface RigFile {
  version: number;
  name: string;
  genre?: string;
  description?: string;
  instrument: Instrument;
  inputGainDb?: number;
  chain: {
    pedal: string;
    params?: Record<string, number | boolean | string>;
    bypassed?: boolean;
  }[];
}

/** The stock rigs, read from the repository's `rigs/` at build time. */
const RIG_FILES = import.meta.glob<RigFile>("../../../rigs/*.rig.json", {
  eager: true,
  import: "default",
});

const MOCK_RIGS_DIR = "/Users/you/Music/Cantina/Rigs";

function rigInfo(rig: RigFile, path: string, stock: boolean): RigInfo {
  return {
    name: rig.name,
    genre: rig.genre ?? "",
    description: rig.description ?? "",
    instrument: rig.instrument,
    stock,
    path,
    chain: rig.chain.map((p) => p.pedal),
  };
}

/** Guitar before bass, then by genre and name, as the shell sorts them. */
function sortRigs<T>(rigs: T[], key: (r: T) => RigInfo): T[] {
  return rigs.sort((a, b) => {
    const x = key(a);
    const y = key(b);
    return (
      Number(x.instrument !== "guitar") - Number(y.instrument !== "guitar") ||
      x.genre.toLowerCase().localeCompare(y.genre.toLowerCase()) ||
      x.name.toLowerCase().localeCompare(y.name.toLowerCase())
    );
  });
}

const mockStockRigFiles = new Map<string, RigFile>(
  Object.entries(RIG_FILES).map(([file, rig]) => [
    `/Applications/Cantina.app/Contents/Resources/${file.replace(/^(\.\.\/)+/, "")}`,
    rig,
  ]),
);
const mockStockRigs: RigInfo[] = sortRigs(
  [...mockStockRigFiles].map(([path, rig]) => rigInfo(rig, path, true)),
  (r) => r,
);

/** The user's rigs in the stand-in: `?myrigs` starts with one. */
const mockUserRigs: { info: RigInfo; file: RigFile }[] = [];
if (mockParams?.has("myrigs")) {
  const file: RigFile = {
    version: 1,
    name: "Friday Crunch",
    genre: "Rock",
    description: "My tone for the Friday band: a pushed amp with a touch of delay.",
    instrument: "guitar",
    chain: [
      { pedal: "overdrive", params: { drive: 12 } },
      { pedal: "amp", params: { gain: 20 } },
      { pedal: "cab:2x12-closed" },
      { pedal: "delay", params: { mix: 18 } },
    ],
  };
  mockUserRigs.push({
    info: rigInfo(file, `${MOCK_RIGS_DIR}/friday-crunch.rig.json`, false),
    file,
  });
}

function mockRigFile(path: string): RigFile {
  const rig = mockStockRigFiles.get(path) ?? mockUserRigs.find((r) => r.info.path === path)?.file;
  if (!rig) throw new Error(`there is no rig at ${path}`);
  return rig;
}

function mockUserRig(path: string) {
  const i = mockUserRigs.findIndex((r) => r.info.path === path);
  if (i < 0) throw new Error(`only your own rigs, in ${MOCK_RIGS_DIR}, can be changed`);
  return i;
}

/** A rig's pedals as the stand-in's chain: values set by name, as the shell resolves them. */
function mockRigChain(rig: RigFile): PedalInfo[] {
  const chain: PedalInfo[] = [];
  const input = (rig.inputGainDb ?? 0) + mock.instrumentProfile.trimDb;
  if (Math.abs(input) >= 0.05) {
    const gain = mockPedal(mockNextId++, "gain");
    const first = gain.params[0];
    if (first) first.value = Math.max(-60, Math.min(24, input));
    chain.push(gain);
  }
  for (const p of rig.chain) {
    const pedal = mockPedal(mockNextId++, p.pedal);
    const ids = MOCK_DEFS[p.pedal]?.params.map((d) => d.id) ?? [];
    for (const [key, value] of Object.entries(p.params ?? {})) {
      const k = key.toLowerCase();
      let i = ids.findIndex((id) => id.toLowerCase() === k);
      if (i < 0) i = pedal.params.findIndex((q) => q.name.toLowerCase() === k);
      const param = pedal.params[i];
      if (!param) continue;
      param.value =
        typeof value === "boolean"
          ? Number(value)
          : typeof value === "string"
            ? Math.max(
                0,
                param.choices.findIndex((c) => c.toLowerCase() === value.toLowerCase()),
              )
            : value;
    }
    pedal.bypassed = p.bypassed ?? false;
    chain.push(pedal);
  }
  return chain;
}

/** An id made from a rig's name, as the shell makes its file names. */
function rigSlug(name: string): string {
  const slug = pedalSlug(name);
  return slug === "pedal" ? "rig" : slug;
}

/** A stock groove's pattern by its path, for the stand-in's import. */
function mockGrooveFile(path: string) {
  const file = Object.entries(GROOVE_FILES).find(([f]) =>
    path.endsWith(f.replace(/^(\.\.\/)+/, "")),
  );
  return file?.[1];
}

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
  jam: null,
  drumDesign: null,
  practicing: null,
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
          ? [
              {
                ...mockClip(4, "Guitar 1", 0, 12.4),
                tab: mockParams.has("tabbed")
                  ? {
                      instrument: "guitar",
                      tuning: [40, 45, 50, 55, 59, 64],
                      capo: 0,
                      notes: 46,
                      transcribed: true,
                    }
                  : null,
              },
              mockClip(5, "Guitar 2", 16, 6.1),
            ]
          : [],
        patternClips: [],
        strip: { gainDb: -3, pan: -0.25, mute: false, solo: mockParams?.has("solo") ?? false },
        latencyFrames: mockParams?.has("latency") ? MOCK_LATENCY : 0,
        pads: {},
      },
      ...(mockTrackCount < 2
        ? []
        : [
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
              pads: {},
            },
          ]),
      // `?tracks=<n>` adds guitar tracks up to n in all, for a long list.
      ...Array.from({ length: Math.max(0, mockTrackCount - 2) }, (_, i) => ({
        id: 200 + i,
        name: `Guitar ${i + 2}`,
        armed: false,
        recording: false,
        kit: null,
        chain: [],
        clips: [],
        patternClips: [],
        strip: { gainDb: 0, pan: 0, mute: false, solo: false },
        latencyFrames: 0,
        pads: {},
      })),
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
              pads: {},
            },
          ]
        : []),
    ],
    patterns: mockParams?.has("drums")
      ? [mockPattern(11, "Verse"), { ...mockPattern(12, "Chorus"), swing: 0 }]
      : [],
    master: { gainDb: -1, pan: 0, mute: false, solo: false },
    // `?loop` loops bars 3 to 6.
    loop: mockParams?.has("loop") ? { start: 4 * MOCK_RATE, end: 12 * MOCK_RATE, on: true } : null,
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
  audition: mockParams?.has("audition") ? { track: MOCK_DRUMS, pattern: 11, groove: null } : null,
  grooves: [],
  rigs: [],
  jams: [],
  exercises: [],
  courses: [],
  // `?rigaudition` hears the Metalcore rig on the guitar track, for screenshots.
  // `?calibrated=-12` starts the profile calibrated at that peak, for screenshots.
  instrumentProfile: (() => {
    const peak = mockParams?.get("calibrated");
    const profile: InstrumentProfile = {
      instrument: mockParams?.get("instrument") === "bass" ? "bass" : "guitar",
      pickups: "humbucker",
      tuning:
        mockParams?.get("instrument") === "bass" ? [28, 33, 38, 43] : [40, 45, 50, 55, 59, 64],
      calibratedPeakDb: peak ? Number(peak) : null,
    };
    return { ...profile, trimDb: profileTrim(profile) };
  })(),
  rigAudition: mockParams?.has("rigaudition")
    ? {
        track: 1,
        path: mockStockRigs.find((r) => r.name === "Metalcore")?.path ?? "",
        hearingRig: mockParams.get("rigaudition") !== "b",
        missing: [],
      }
    : null,
  // `?rowmute=snare` mutes a row of the drum track, for screenshots.
  drumRows: mockParams?.has("rowmute")
    ? { [MOCK_DRUMS]: { muted: [mockParams.get("rowmute") ?? "snare"], soloed: [] } }
    : {},
  // `?practice` turns the voice on at 75%, for screenshots.
  practice: mockParams?.has("practice")
    ? { voice: true, muteRecordings: true, speed: 0.75 }
    : { voice: false, muteRecordings: false, speed: 1 },
  analysisSource: null,
  metronome: {
    on: mockParams?.has("metronome") ?? false,
    sound: mockParams?.get("metronome") || "Standard",
    levelDb: -6,
    sounds: ["Standard", "Woodblock", "Cowbell", "Drum", "Hi-hat"],
  },
  countIn: !(mockParams?.has("nocountin") ?? false),
  openLastAtLaunch: false,
  assistant: mockParams?.has("assistant")
    ? {
        enabled: true,
        endpoint: MOCK_ENDPOINT,
        model: "droid-large",
        hasKey: true,
        persona: !mockParams.has("plain"),
        sounds: false,
      }
    : { enabled: false, endpoint: "", model: "", hasKey: false, persona: true, sounds: false },

  library: {
    pedalsDir: "/Users/you/Music/Cantina/Pedals",
    kitsDir: "/Users/you/Music/Cantina/Kits",
    patternsDir: "/Users/you/Music/Cantina/Patterns",
    rigsDir: "/Users/you/Music/Cantina/Rigs",
    jamsDir: "/Users/you/Music/Cantina/Jams",
    exercisesDir: "/Users/you/Music/Cantina/Exercises",
    coursesDir: "/Users/you/Music/Cantina/Courses",
    groovesDir: "/Users/you/Music/Cantina/Grooves",
    grooves: [],
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
          {
            kind: "rig",
            name: "loud.rig.json",
            path: "/Users/you/Music/Cantina/Rigs/loud.rig.json",
            reason:
              'chain 1 (amp): amp has no parameter "volume"; it has gain, bass, mid, treble, presence, level',
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
  const i = mockSettings.noInput ? undefined : mockDevice(mockSettings.inputUid, true);
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
        pads: structuredClone(t.pads),
      })),
      loop: mock.project.loop ? { ...mock.project.loop } : null,
      patterns: mock.project.patterns.map((p) => ({
        ...p,
        rows: p.rows.map((r) => structuredClone(r)),
      })),
      master: { ...mock.project.master },
    },
    audition: mock.audition ? { ...mock.audition } : null,
    practice: { ...mock.practice },
    grooves: mockGrooves,
    rigs: [...mockStockRigs, ...mockUserRigs.map((r) => r.info)].map((r) => ({
      ...r,
      chain: [...r.chain],
    })),
    rigAudition: mock.rigAudition
      ? { ...mock.rigAudition, missing: [...mock.rigAudition.missing] }
      : null,
    instrumentProfile: { ...mock.instrumentProfile, tuning: [...mock.instrumentProfile.tuning] },
    drumRows: structuredClone(mock.drumRows),
    jam: mockJam ? mockJamStatus(mockJam) : null,
    practicing: mockPractice && mockJam ? mockPracticeStatus(mockPractice, mockJam) : null,
    drumDesign: mockDesign ? mockDesignStatus(mockDesign) : null,
    jams: mockJams(),
    exercises: mockExercises(),
    courses: mockCourses(),
    metronome: { ...mock.metronome, sounds: [...mock.metronome.sounds] },
    countIn: mock.countIn,
    openLastAtLaunch: mock.openLastAtLaunch,
    assistant: { ...mock.assistant },
    sampleRate: mockSettings.sampleRate,
    bufferFrames: mockSettings.bufferFrames,
    inputUid: mockSettings.inputUid,
    outputUid: mockSettings.outputUid,
    inputName: mock.running ? (i?.name ?? null) : null,
    latency: mockSettings.noInput ? { ...mock.latency, inputFrames: 0 } : mock.latency,
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

// ----- the assistant's connection -----

/*
 * The stand-in has a fake provider at MOCK_ENDPOINT with two models and a
 * key of anything but "bad". An endpoint with "bad" in it does not answer,
 * one without "/v1" has no API, and any other model is unknown.
 */
const MOCK_MODELS = ["droid-large", "droid-small"];
let mockKey: string | null = mockParams?.has("assistant") ? "sk-stand-in" : null;

function mockProblem(kind: AssistantProblemKind, message: string): AssistantProblem {
  return { kind, message };
}

/** Rejects as the shell would, or resolves with the stand-in's model list. */
function mockModels(): string[] {
  const a = mock.assistant;
  if (!a.enabled) throw mockProblem("off", "The assistant is off. Turn it on in Settings.");
  if (!/^https?:\/\/\S+/.test(a.endpoint)) {
    throw mockProblem(
      "badUrl",
      `"${a.endpoint}" is not a web address. It should start with https://.`,
    );
  }
  if (a.endpoint.includes("bad")) {
    throw mockProblem(
      "badUrl",
      `Could not connect to ${a.endpoint}. Check the URL and your connection.`,
    );
  }
  if (!a.endpoint.includes("/v1")) {
    throw mockProblem(
      "badUrl",
      "Nothing answered at that URL. It should be the API's base, usually ending in /v1.",
    );
  }
  if (mockKey === "bad" || !mockKey) {
    throw mockProblem("badKey", "The endpoint refused the API key. Check it, or paste a new one.");
  }
  return [...MOCK_MODELS];
}

/** Turns the assistant on or off and sets its endpoint and model. An app setting, not undoable. */
export async function assistantConfigure(settings: AssistantSettings): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("assistant_configure", { settings });
  mock.assistant = {
    enabled: settings.enabled,
    endpoint: settings.endpoint.trim(),
    model: settings.model.trim(),
    persona: settings.persona,
    sounds: settings.sounds,
    hasKey: settings.enabled && mockKey !== null,
  };
  mockPublish();
  return mockStatus();
}

/** Saves the API key in the Keychain, or removes it when empty. It never comes back. */
export async function assistantSetKey(key: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("assistant_set_key", { key });
  mockKey = key.trim() || null;
  mock.assistant = { ...mock.assistant, hasKey: mock.assistant.enabled && mockKey !== null };
  mockPublish();
  return mockStatus();
}

/** The models the endpoint offers. Rejects with an `AssistantProblem`. */
export async function assistantModels(): Promise<string[]> {
  if (inTauri) return invoke<string[]>("assistant_models");
  await new Promise((r) => window.setTimeout(r, 300));
  return mockModels();
}

/** Checks the endpoint, key and model. Rejects with an `AssistantProblem`. */
export async function assistantTest(): Promise<void> {
  if (inTauri) return invoke<void>("assistant_test");
  await new Promise((r) => window.setTimeout(r, 500));
  const models = mockModels();
  if (!mock.assistant.model) throw mockProblem("unknownModel", "Pick a model to talk to.");
  if (!models.includes(mock.assistant.model)) {
    throw mockProblem(
      "unknownModel",
      `The endpoint has no model called "${mock.assistant.model}". Pick one from the list.`,
    );
  }
}

/** A rejection from an assistant command as a problem, whatever shape it came in. */
export function asProblem(e: unknown): AssistantProblem {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as AssistantProblem;
  return { kind: "other", message: String(e) };
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
  return `${status.inputName ?? "No input"} → ${status.outputName ?? "?"} · ${status.sampleRate / 1000} kHz · ${status.bufferFrames} frames${comp}`;
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

/**
 * The recent projects, newest first, each with when it was last saved;
 * one moved or deleted since is marked missing.
 */
export async function projectRecent(): Promise<RecentProject[]> {
  if (inTauri) return invoke<RecentProject[]>("project_recent");
  return mockRecent.map((r) => ({ ...r }));
}

const MOCK_NOW = Date.now();
const MOCK_DAY = 24 * 60 * 60 * 1000;
// `?start=none` has no recent projects, for the dialog's first launch.
const mockRecent: RecentProject[] =
  mockParams?.get("start") === "none"
    ? []
    : [
        { name: "Untitled", saved: MOCK_NOW - 20 * 60 * 1000, missing: false },
        { name: "Blues in A", saved: MOCK_NOW - MOCK_DAY - 3 * 60 * 60 * 1000, missing: false },
        { name: "Old demo", saved: null, missing: true },
        { name: "Riff ideas", saved: MOCK_NOW - 9 * MOCK_DAY, missing: false },
      ];

/**
 * Creates a project and opens it: the stock one, or one shaped by the
 * start dialog's options (its rate, tempo and tracks), all in one step
 * with nothing to undo.
 */
export async function projectNew(name: string, options?: NewProjectOptions): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_new", { name, options: options ?? null });
  name = name.trim();
  if (!name) throw new Error("a project needs a name");
  if (mockProjects.includes(name)) throw new Error(`there is already a project called ${name}`);
  const start = options?.start ?? "guitar";
  const kit = mockKits[0]?.id;
  if (start === "guitarDrums" && !kit) throw new Error("no drum kits are installed");
  const audio = (id: number, track: string, chain: PedalInfo[]): TrackStatus => ({
    id,
    name: track,
    armed: false,
    recording: false,
    kit: null,
    chain,
    clips: [],
    patternClips: [],
    strip: { ...UNITY },
    latencyFrames: 0,
    pads: {},
  });
  const guitar = () =>
    audio(1, options ? "Guitar" : "Track 1", [mockPedal(2, "gain"), mockPedal(3, "overdrive")]);
  const tracks: TrackStatus[] =
    start === "empty"
      ? []
      : start === "guitarBass"
        ? [guitar(), audio(4, "Bass", [])]
        : start === "guitarDrums" && kit
          ? [guitar(), { ...audio(4, "Drums", []), kit: mockKitStatus(kit) }]
          : [guitar()];
  mockProjects.push(name);
  mockRecent.unshift({ name, saved: Date.now(), missing: false });
  if (options) mockSettings.sampleRate = options.sampleRate;
  mock.project = {
    ...mock.project,
    name,
    dir: `/Users/you/Music/Cantina/${name}.cantina`,
    dirty: false,
    undoLabel: null,
    redoLabel: null,
    sampleRate: options?.sampleRate ?? mock.project.sampleRate,
    tempo: options ? { ...options.tempo } : mock.project.tempo,
    tracks,
    patterns: [],
    master: { ...UNITY },
    loop: null,
  };
  mockUndo.length = 0;
  mockRedo.length = 0;
  mockLast = structuredClone(mock.project);
  return mockStatus();
}

export async function projectOpen(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_open", { name });
  const recent = mockRecent.findIndex((r) => r.name === name);
  if (mockRecent[recent]?.missing) throw new Error(`${name} has been moved or deleted`);
  if (recent >= 0) mockRecent.unshift(...mockRecent.splice(recent, 1));
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
 * Moves a project, not the open one, to the Trash, where Finder can put
 * it back, and drops it from the recent list.
 */
export async function projectDelete(name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("project_delete", { name });
  if (name === mock.project.name) {
    throw new Error(`${name} is open: open another project before deleting it`);
  }
  const at = mockProjects.indexOf(name);
  if (at < 0) throw new Error(`there is no project called ${name}`);
  mockProjects.splice(at, 1);
  const recent = mockRecent.findIndex((r) => r.name === name);
  if (recent >= 0) mockRecent.splice(recent, 1);
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

/** Opens one of the Help menu's links, or a guide Settings points to, in the browser. Nothing in the stand-in. */
export async function openLink(
  which: "docs" | "shortcuts" | "releases" | "issue" | "assistant",
): Promise<void> {
  if (inTauri) await invoke("open_link", { which });
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
    pads: {},
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
    // A drum track starts 12 dB down for headroom, as the shell's does.
    strip: { ...UNITY, gainDb: -12 },
    latencyFrames: 0,
    pads: {},
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

/** Plays a pad of a drum track now: it sounds at the next audio block. Not an edit. */
export async function padHit(track: number, pad: string, velocity: number): Promise<void> {
  if (inTauri) return invoke("pad_hit", { track, pad, velocity });
  if (!mockTrack(track).kit) throw new Error("not a drum track");
}

/** Sets a pad's mix on a drum track; null (or the kit's own) puts it back. One undoable edit. */
export async function padMixSet(
  track: number,
  pad: string,
  mix: PadMix | null,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pad_mix_set", { track, pad, mix });
  const t = mockTrack(track);
  if (!t.kit) throw new Error(`${t.name} is not a drum track`);
  const same = !mix || JSON.stringify(mix) === JSON.stringify(PAD_MIX_DEFAULT);
  if (same) delete t.pads[pad];
  else t.pads[pad] = { ...mix };
  return mockEdit("change pad mix");
}

/** Sets, or with null removes, a pattern clip's fill. One undoable edit. */
export async function patternClipSetFill(
  id: number,
  fill: PatternFill | null,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_clip_set_fill", { id, fill });
  const { clip } = mockPatternClipAt(id);
  if (fill) {
    mockPatternById(fill.pattern);
    clip.fill = { ...fill };
  } else {
    delete clip.fill;
  }
  return mockEdit(fill ? "set fill" : "remove fill");
}

/**
 * Writes a fill from a pattern clip's own pattern, on its track's kit, as
 * a new pattern, and has the clip play it: one undoable edit. The same
 * seed writes the same fill.
 */
export async function patternClipGenerateFill(id: number, seed: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_clip_generate_fill", { id, seed });
  const { clip } = mockPatternClipAt(id);
  const base = mockPatternById(clip.pattern);
  // The stand-in's fill: the groove, then a snare roll over the last beat.
  const bar = base.stepsPerBeat * 4;
  const from = bar - base.stepsPerBeat;
  const rows = base.rows.map((r) => ({
    pad: r.pad,
    steps: Array.from({ length: bar }, (_, s) =>
      s >= from && r.pad !== "kick" ? 0 : (r.steps[s % r.steps.length] ?? 0),
    ),
  }));
  const snare = rows.find((r) => r.pad === "snare");
  for (let s = from; s < bar; s++) {
    if (snare) snare.steps[s] = 0.5 + (0.5 * (s - from + 1)) / (bar - from);
  }
  const taken = new Set(mock.project.patterns.map((p) => p.name));
  let name = `${base.name} fill`;
  for (let n = 2; taken.has(name); n++) name = `${base.name} fill ${n}`;
  const fill = {
    id: mockNextId++,
    name,
    stepsPerBeat: base.stepsPerBeat,
    length: bar,
    swing: base.swing,
    rows,
  };
  mock.project.patterns.push(fill);
  clip.fill = { every: 4, atEnd: true, ...clip.fill, pattern: fill.id };
  void seed;
  return mockEdit(`add pattern ${name}`);
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
  mock.audition = track !== null && pattern !== null ? { track, pattern, groove: null } : null;
  return mockStatus();
}

/** Replaces a pattern whole, as one undoable edit: what every grid gesture sends. */
export async function patternReplace(pattern: PatternInfo): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("pattern_replace", { pattern });
  const index = mock.project.patterns.findIndex((p) => p.id === pattern.id);
  if (index < 0) throw new Error(`no pattern ${pattern.id}`);
  if (pattern.length < 1 || pattern.length > 128) throw new Error("a pattern has 1 to 128 steps");
  for (const r of pattern.rows) {
    if (r.steps.length !== pattern.length) throw new Error(`row ${r.pad} has the wrong length`);
  }
  mock.project.patterns[index] = structuredClone(pattern);
  return mockEdit("edit pattern");
}

/** Mutes and solos rows of a drum track's patterns, for listening while editing. Not an edit. */
export async function drumRowsSet(track: number, rows: DrumRows): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_rows_set", { track, rows });
  if (rows.muted.length === 0 && rows.soloed.length === 0) delete mock.drumRows[track];
  else mock.drumRows[track] = structuredClone(rows);
  return mockStatus();
}

/**
 * Loops a stock groove's file on a drum track to hear it before adding it,
 * at the project's tempo on the track's kit: from the next bar while
 * playing, at once (and playing) from a stop. Both null stops.
 */
export async function grooveAudition(
  track: number | null,
  path: string | null,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("groove_audition", { track, path });
  if (track === null || path === null) {
    if (mock.audition?.groove) mock.audition = null;
    return mockStatus();
  }
  if (!mockGrooveFile(path)) throw new Error(`no groove ${path}`);
  mock.audition = { track, pattern: null, groove: path };
  mockTransport.playing = true;
  return mockStatus();
}

/**
 * Hears the rig at `path` on an audio track's live input in place of its
 * chain, without changing the project; both null stops and puts the
 * chain back. The swap fades the track out and in, so it never clicks.
 */
export async function rigAudition(
  track: number | null,
  path: string | null,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_audition", { track, path });
  if (track === null || path === null) {
    mock.rigAudition = null;
    return mockStatus();
  }
  if (mockTrack(track).kit) throw new Error("a rig goes on an audio track");
  mockRigFile(path);
  mock.rigAudition = { track, path, hearingRig: true, missing: [] };
  return mockStatus();
}

/** While auditioning, hears the rig (A) or the track's own chain (B). */
/**
 * Sets what the player plays through. Rigs applied or auditioned from
 * now on trim their input from it. An app setting, not undoable.
 */
export async function instrumentProfileSet(profile: InstrumentProfile): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("instrument_profile_set", { profile });
  const peak = profile.calibratedPeakDb;
  if (peak !== null && !(Number.isFinite(peak) && peak >= -90 && peak <= 6)) {
    throw new Error(`a calibration of ${peak} dBFS is not a level an input gives`);
  }
  mock.instrumentProfile = {
    ...profile,
    tuning: [...profile.tuning],
    trimDb: profileTrim(profile),
  };
  return mockStatus();
}

export async function rigAb(hearRig: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_ab", { hearRig });
  if (!mock.rigAudition) throw new Error("no rig is being auditioned");
  mock.rigAudition.hearingRig = hearRig;
  return mockStatus();
}

/** Replaces an audio track's chain with a rig's, as one undoable edit. */
export async function rigApply(track: number, path: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_apply", { track, path });
  const t = mockTrack(track);
  if (t.kit) throw new Error("a rig goes on an audio track");
  t.chain = mockRigChain(mockRigFile(path));
  mock.rigAudition = null;
  return mockEdit("apply rig");
}

/** Writes a track's chain to the Rigs library folder as a rig. */
export async function rigSave(track: number, rig: RigDraft): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_save", { track, rig });
  const name = rig.name.trim();
  if (!name) throw new Error("a rig needs a name");
  const t = mockTrack(track);
  if (t.chain.length === 0) throw new Error(`${t.name} has no pedals to save`);
  const path = `${MOCK_RIGS_DIR}/${rigSlug(name)}.rig.json`;
  const at = mockUserRigs.findIndex((r) => r.info.path === path);
  if (at >= 0 && !rig.replace) throw new Error(`there is already a rig called ${name}`);
  // A Gain pedal first is the rig's input gain, less the profile's trim.
  const [first, ...rest] = t.chain;
  const leading = first?.spec === "gain" && !first.bypassed;
  const gain = leading ? (first.params[0]?.value ?? 0) - mock.instrumentProfile.trimDb : 0;
  const file: RigFile = {
    version: 1,
    name,
    genre: rig.genre.trim(),
    description: rig.description.trim(),
    instrument: rig.instrument,
    inputGainDb: Math.max(-24, Math.min(24, Math.round(gain * 10) / 10)),
    chain: (leading ? rest : t.chain).map((p) => ({
      pedal: p.spec,
      params: Object.fromEntries(p.params.map((q) => [q.name.toLowerCase(), q.value])),
      bypassed: p.bypassed,
    })),
  };
  const entry = { info: rigInfo(file, path, false), file };
  if (at >= 0) mockUserRigs[at] = entry;
  else mockUserRigs.push(entry);
  sortRigs(mockUserRigs, (r) => r.info);
  mock.notice = `Saved the rig ${name}`;
  mock.noticeSeq += 1;
  return mockStatus();
}

/** Renames one of the user's rigs, and its file to match. */
export async function rigRename(path: string, name: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_rename", { path, name });
  const trimmed = name.trim();
  if (!trimmed) throw new Error("a rig needs a name");
  const i = mockUserRig(path);
  const to = `${MOCK_RIGS_DIR}/${rigSlug(trimmed)}.rig.json`;
  if (to !== path && mockUserRigs.some((r) => r.info.path === to)) {
    throw new Error(`there is already a rig file called ${rigSlug(trimmed)}.rig.json`);
  }
  const old = mockUserRigs[i];
  if (!old) throw new Error(`there is no rig at ${path}`);
  const file = { ...old.file, name: trimmed };
  mockUserRigs[i] = { info: rigInfo(file, to, false), file };
  sortRigs(mockUserRigs, (r) => r.info);
  if (mock.rigAudition?.path === path) mock.rigAudition.path = to;
  return mockStatus();
}

/** Deletes one of the user's rigs. */
export async function rigDelete(path: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_delete", { path });
  mockUserRigs.splice(mockUserRig(path), 1);
  return mockStatus();
}

/** Copies a rig file into the Rigs library folder after checking it. */
export async function rigImport(path: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("rig_import", { path });
  const stem = (path.split("/").pop() ?? "rig").replace(/\.rig\.json$/, "");
  const name = stem.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  const file: RigFile = {
    version: 1,
    name,
    genre: "Rock",
    description: "A rig someone shared.",
    instrument: "guitar",
    chain: [{ pedal: "overdrive" }, { pedal: "amp" }, { pedal: "cab:2x12-closed" }],
  };
  let n = 1;
  let to = `${MOCK_RIGS_DIR}/${rigSlug(name)}.rig.json`;
  while (mockUserRigs.some((r) => r.info.path === to)) {
    n += 1;
    to = `${MOCK_RIGS_DIR}/${rigSlug(`${name} ${n}`)}.rig.json`;
  }
  mockUserRigs.push({ info: rigInfo(file, to, false), file });
  sortRigs(mockUserRigs, (r) => r.info);
  mock.notice = `Added the rig ${to.split("/").pop()} to your rigs`;
  mock.noticeSeq += 1;
  return mockStatus();
}

/** Writes a copy of the rig at `path` to `to`, for sharing. */
export async function rigExport(path: string, to: string): Promise<void> {
  if (inTauri) return invoke("rig_export", { path, to });
  mockRigFile(path);
}

/**
 * Sets how to practice: hear the tabs, mute the recordings under them, and
 * the speed from 50% to 100%. Not an edit.
 */
export async function practiceSet(practice: Practice): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("practice_set", { practice });
  if (!(practice.speed >= 0.5 && practice.speed <= 1)) {
    throw new Error(`a practice speed is 50% to 100%, not ${Math.round(practice.speed * 100)}%`);
  }
  if (practice.speed < 1 && mockTransport.recording) {
    throw new Error("stop recording before slowing down");
  }
  mock.practice = { ...practice };
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

/** Sets the loop region and whether playback loops, or clears it with null. */
export async function loopSet(region: LoopRegion | null): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("loop_set", { region });
  if (region && region.end <= region.start) {
    throw new Error(`a loop from ${region.start} to ${region.end} has no length`);
  }
  mock.project.loop = region ? { ...region } : null;
  return mockEdit(region ? (region.on ? "loop" : "loop off") : "clear loop");
}

/** Bypasses every pedal on a track, or turns them all on, as one edit. */
export async function chainBypass(track: number, on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_bypass", { track, on });
  for (const pedal of mockTrack(track).chain) pedal.bypassed = on;
  return mockEdit(on ? "bypass pedal" : "turn pedal on");
}

/** Turns Record's count-in on or off. An app setting, not undoable. */
export async function countInSet(on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("count_in_set", { on });
  mock.countIn = on;
  return mockStatus();
}

/** Whether launch opens the last project rather than the start dialog. */
export async function openLastAtLaunchSet(on: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("open_last_at_launch_set", { on });
  mock.openLastAtLaunch = on;
  return mockStatus();
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

/**
 * Transcribes a clip into a tab, off the audio thread, as one undoable
 * edit. Progress arrives through `onRenderProgress` as `transcribe`;
 * `renderCancel` stops it and leaves the project as it was.
 */
export async function clipTranscribe(id: number, settings: TabSettings): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_transcribe", { id, settings });
  const { clip } = mockClipAt(id);
  await mockRender("transcribe");
  // The stand-in hears about two notes a second.
  const notes = Math.max(1, Math.round((clip.length / MOCK_RATE) * 2));
  clip.tab = {
    instrument: settings.instrument,
    tuning: settings.tuning,
    capo: settings.capo,
    notes,
    transcribed: true,
  };
  return mockEdit("set tab");
}

/**
 * The stand-in keeps a clip's whole tab beside its summary, made up on
 * first look, so undo and redo, which copy the project, take it along.
 */
type MockTabSummary = TabSummary & { full?: Tab };

function mockFullTab(clip: ClipInfo): Tab {
  const summary = clip.tab as MockTabSummary;
  summary.full ??= mockTab(clip);
  return summary.full;
}

/**
 * The stand-in's tab for a clip: a two-bar riff that uses every
 * technique the format has, and a triplet, repeated for the clip.
 */
function mockTab(clip: ClipInfo): Tab {
  const summary = clip.tab as TabSummary;
  const strings = summary.tuning.length;
  const riff: TabNote[] = [
    { string: 6, fret: 0, start: 0, length: 240, palmMute: true },
    { string: 6, fret: 0, start: 240, length: 240, palmMute: true },
    { string: 6, fret: 0, start: 480, length: 240, palmMute: true },
    { string: 6, fret: 3, start: 720, length: 240 },
    { string: 5, fret: 5, start: 720, length: 240 },
    { string: 4, fret: 5, start: 720, length: 240 },
    { string: 6, fret: 0, start: 960, length: 240, palmMute: true },
    { string: 6, fret: 0, start: 1200, length: 240, palmMute: true },
    { string: 6, fret: 5, start: 1440, length: 480, letRing: true },
    { string: 5, fret: 7, start: 1440, length: 480, letRing: true },
    { string: 4, fret: 7, start: 1440, length: 480, letRing: true },
    { string: 3, fret: 5, start: 1920, length: 240 },
    { string: 3, fret: 7, start: 2160, length: 240, hammerOn: true },
    { string: 3, fret: 5, start: 2400, length: 240, pullOff: true },
    { string: 4, fret: 7, start: 2640, length: 240, slide: true },
    { string: 4, fret: 9, start: 2880, length: 480 },
    { string: 2, fret: 8, start: 3360, length: 480, bend: { semitones: 2, release: true } },
    { string: 1, fret: 12, start: 3840, length: 960, harmonic: true },
    { string: 3, fret: 0, start: 4800, length: 240, dead: true },
    { string: 3, fret: 0, start: 5040, length: 240, dead: true },
    { string: 3, fret: 7, start: 5280, length: 480, vibrato: true },
    { string: 1, fret: 15, start: 5760, length: 320, tap: true },
    { string: 1, fret: 12, start: 6080, length: 320, pullOff: true },
    { string: 2, fret: 13, start: 6400, length: 320 },
    { string: 2, fret: 12, start: 6720, length: 960, bend: { semitones: 1 }, vibrato: true },
  ];
  const tempo = mock.project.tempo;
  const ticks = (clip.length / MOCK_RATE) * (tempo.bpm / 60) * 960;
  const notes: TabNote[] = [];
  for (let at = 0; at + 7680 <= ticks; at += 7680) {
    for (const n of riff) {
      notes.push({ ...n, start: n.start + at, string: Math.min(n.string, strings) });
    }
  }
  return {
    name: clip.name,
    instrument: summary.instrument,
    tuning: summary.tuning,
    capo: summary.capo,
    tempo: { ...tempo },
    origin: clip.offset,
    notes,
  };
}

/** A clip's whole tab, for the tab view; null when it has none. */
export async function clipTab(id: number): Promise<Tab | null> {
  if (inTauri) return invoke<Tab | null>("clip_tab", { id });
  const { clip } = mockClipAt(id);
  if (!clip.tab) return null;
  return structuredClone(mockFullTab(clip));
}

/** The stand-in's `EditTabNotes`: checked as the engine checks it. */
function mockTabEdit(id: number, remove: number[], add: TabNote[]): EngineStatus {
  const { clip } = mockClipAt(id);
  if (!clip.tab) throw new Error(`${clip.name} has no tab`);
  if (remove.length === 0 && add.length === 0) return mockStatus();
  const tab = mockFullTab(clip);
  const gone = new Set(remove);
  if (gone.size !== remove.length) throw new Error("a note is removed twice");
  for (const i of remove) {
    if (i < 0 || i >= tab.notes.length) throw new Error(`there is no note ${i + 1}`);
  }
  for (const n of add) {
    if (n.string < 1 || n.string > tab.tuning.length) {
      throw new Error(`strings run from 1 to ${tab.tuning.length}`);
    }
    if (n.fret < 0 || n.fret > 30) throw new Error(`fret ${n.fret} is past 30`);
    if (n.length <= 0) throw new Error("a note needs a length");
  }
  tab.notes = sortTabNotes([...tab.notes.filter((_, i) => !gone.has(i)), ...add]);
  clip.tab.notes = tab.notes.length;
  const label =
    remove.length === 0
      ? add.length === 1
        ? "add note"
        : "add notes"
      : add.length === 0
        ? remove.length === 1
          ? "delete note"
          : "delete notes"
        : "edit tab";
  return mockEdit(label);
}

/** Edits a tab's notes: takes away those at `remove` (indices) and adds `add`, as one undoable edit. */
export async function clipTabEdit(
  id: number,
  remove: number[],
  add: TabNote[],
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_tab_edit", { id, remove, add });
  return mockTabEdit(id, remove, add);
}

/**
 * Builds a transcribed tab's notes again on another grid, without
 * transcribing again: the selected notes (indices), or all of them.
 */
export async function clipTabRequantize(
  id: number,
  grid: TabGrid,
  selection: number[] = [],
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_tab_requantize", { id, grid, selection });
  const { clip } = mockClipAt(id);
  if (!clip.tab) throw new Error(`${clip.name} has no tab`);
  if (!clip.tab.transcribed) {
    throw new Error(`${clip.name}'s tab was not transcribed, so there is nothing to re-quantize`);
  }
  // The stand-in heard nothing: it snaps the notes it has to the grid.
  const step = { "1/8": 480, "1/16": 240, "1/8t": 320, "1/16t": 160 }[grid];
  const tab = mockFullTab(clip);
  const chosen = selection.length > 0 ? selection : tab.notes.map((_, i) => i);
  const add = chosen.map((i) => {
    const n = tab.notes[i] as TabNote;
    const start = Math.round(n.start / step) * step;
    return { ...n, start, length: Math.max(step, Math.round(n.length / step) * step) };
  });
  return mockTabEdit(id, chosen, add);
}

/** Fits the selected notes (indices), or all of them, to the fretboard again. */
export async function clipTabRefit(id: number, selection: number[] = []): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("clip_tab_refit", { id, selection });
  const { clip } = mockClipAt(id);
  if (!clip.tab) throw new Error(`${clip.name} has no tab`);
  // The stand-in's fretboard: the lowest fret that plays the same pitch.
  const tab = mockFullTab(clip);
  const chosen = selection.length > 0 ? selection : tab.notes.map((_, i) => i);
  const remove: number[] = [];
  const add: TabNote[] = [];
  for (const i of chosen) {
    const n = tab.notes[i] as TabNote;
    if (n.dead || n.harmonic) continue;
    const pitch = tabPitch(tab, n.string, n.fret);
    let best = n;
    for (let s = 1; s <= tab.tuning.length; s++) {
      const fret = pitch - tabPitch(tab, s, 0);
      if (fret >= 0 && fret <= 24 && fret < best.fret) best = { ...n, string: s, fret };
    }
    if (best !== n) {
      remove.push(i);
      add.push(best);
    }
  }
  return mockTabEdit(id, remove, add);
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

// ----- jams -----

const JAM_PPQ = 960;
/** The stand-in's jam: the setup and the schedule, as the shell keeps them. */
let mockJam: {
  setup: Jam;
  sections: { tick: number; section: GrooveSection }[];
  tempos: { tick: number; bpm: number }[];
  end: number | null;
} | null = null;

function mockJamPatterns(setup: Jam): Partial<Record<GrooveSection, number>> {
  const bar = (JAM_PPQ * 4 * setup.timeSignature.numerator) / setup.timeSignature.denominator;
  const bars: Partial<Record<GrooveSection, number>> = {};
  for (const g of mockGrooves.filter((g) => g.genre === setup.drums.groove)) {
    bars[g.section] = ((g.length / g.stepsPerBeat) * JAM_PPQ) / bar;
  }
  // Not a stock genre: one of the user's grooves, by name, by kind of section.
  const mine = mockUserGrooves.find((g) => g.file.name === setup.drums.groove)?.file;
  for (const s of mine?.sections ?? []) {
    if (!s.kind || bars[s.kind] !== undefined) continue;
    bars[s.kind] = (patternTicks(s.pattern) * passes(s).length) / bar;
  }
  for (const [section, p] of Object.entries(setup.drums.patterns ?? {})) {
    if (p) bars[section as GrooveSection] = ((p.length / p.stepsPerBeat) * JAM_PPQ) / bar;
  }
  return bars;
}

function mockJamBarTicks(setup: Jam): number {
  return (JAM_PPQ * 4 * setup.timeSignature.numerator) / setup.timeSignature.denominator;
}

/** Seconds at a tick, through the stand-in's tempo changes. */
function mockJamSeconds(tempos: { tick: number; bpm: number }[], tick: number): number {
  let seconds = 0;
  for (let i = 0; i < tempos.length; i++) {
    const from = tempos[i];
    if (!from || from.tick >= tick) break;
    const to = Math.min(tempos[i + 1]?.tick ?? tick, tick);
    seconds += ((to - from.tick) / JAM_PPQ) * (60 / from.bpm);
  }
  return seconds;
}

function mockJamTick(tempos: { tick: number; bpm: number }[], seconds: number): number {
  let tick = 0;
  let at = 0;
  for (let i = 0; i < tempos.length; i++) {
    const t = tempos[i];
    if (!t) break;
    const next = tempos[i + 1];
    const span = next ? mockJamSeconds(tempos, next.tick) - at : Number.POSITIVE_INFINITY;
    if (seconds - at <= span) return t.tick + ((seconds - at) * t.bpm * JAM_PPQ) / 60;
    at += span;
    tick = next?.tick ?? tick;
  }
  return tick;
}

const JAM_ORDER: GrooveSection[] = ["intro", "verse", "chorus", "bridge", "fill", "ending"];

function mockJamStatus(jam: NonNullable<typeof mockJam>): JamStatus {
  const bars = mockJamPatterns(jam.setup);
  const ticks = [...new Set([...jam.sections, ...jam.tempos].map((m) => m.tick))].sort(
    (a, b) => a - b,
  );
  const at = <T extends { tick: number }>(list: T[], tick: number) =>
    [...list].reverse().find((m) => m.tick <= tick) ?? list[0];
  return {
    setup: structuredClone(jam.setup),
    sections: JAM_ORDER.filter((s) => bars[s] !== undefined),
    bars,
    marks: ticks
      .filter((t) => jam.end === null || t < jam.end)
      .map((tick) => ({
        tick,
        sample: Math.round(mockJamSeconds(jam.tempos, tick) * MOCK_RATE),
        section: at(jam.sections, tick)?.section ?? "verse",
        bpm: at(jam.tempos, tick)?.bpm ?? jam.setup.tempo,
      })),
    end: jam.end === null ? null : Math.round(mockJamSeconds(jam.tempos, jam.end) * MOCK_RATE),
    barTicks: mockJamBarTicks(jam.setup),
    tempo: jam.tempos.at(-1)?.bpm ?? jam.setup.tempo,
    missing: [],
    backingMissing: [],
    tempoLocked: jam.setup.backing?.kind === "audio" || jam.setup.backing?.kind === "track",
    presets: MOCK_PRESETS.map((p) => ({ ...p })),
    captureMinutes: mockCaptureMinutes,
    kept: mockKept ? { ...mockKept } : null,
  };
}

let mockCaptureMinutes = 5;

function mockTrackTemplate(id: number, name: string): TrackStatus {
  return {
    id,
    name,
    armed: false,
    recording: false,
    kit: null,
    chain: [],
    clips: [],
    patternClips: [],
    strip: { ...UNITY },
    latencyFrames: 0,
    pads: {},
  };
}
// `?kept` has a take waiting, for screenshots.
let mockKept: JamKept | null = mockParams?.has("kept")
  ? {
      seconds: 94.5,
      dry: "/Users/you/Music/Cantina/Jams/Takes/Jam-3.wav",
      processed: "/Users/you/Music/Cantina/Jams/Takes/Jam-3 processed.wav",
    }
  : null;

/**
 * Keep that: writes the last minutes of the jam's input, dry and through
 * the rig, to a take in `Jams/Takes`, for `jamKeepPlace` to put in a project.
 */
export async function jamKeep(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_keep");
  if (!mockJam) throw new Error("open a jam first");
  if (!mock.running) throw new Error("start the engine first");
  if (mockSettings.noInput) {
    throw new Error("there is no input to keep; the jam is running without one");
  }
  const n = Math.floor(Math.random() * 90) + 2;
  mockKept = {
    seconds: Math.min(mockCaptureMinutes * 60, 30 + mockTransport.position),
    dry: `/Users/you/Music/Cantina/Jams/Takes/Jam-${n}.wav`,
    processed: `/Users/you/Music/Cantina/Jams/Takes/Jam-${n} processed.wav`,
  };
  return mockStatus();
}

/**
 * Puts the take kept into the open project or a new one, as one undoable
 * edit (the take on a track with the jam's rig, the processed take muted,
 * and the drums that played under it), or leaves it in `Jams/Takes`.
 */
export async function jamKeepPlace(into: KeepInto): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_keep_place", { into });
  if (!mockKept) throw new Error("keep a take first");
  const kept = mockKept;
  mockKept = null;
  if (into.into === "takes") return mockStatus();
  if (into.into === "new") await projectNew(into.name);
  const id = () => mockNextId++;
  const take = mockTrackTemplate(id(), "Jam take");
  take.clips = [mockClip(id(), "Jam take", 0, kept.seconds)];
  const processed = mockTrackTemplate(id(), "Jam take, processed");
  processed.clips = [mockClip(id(), "Jam take, processed", 0, kept.seconds)];
  processed.strip = { ...processed.strip, mute: true };
  mock.project.tracks.push(take, processed);
  mock.notice = `Kept ${Math.floor(kept.seconds / 60)}:${(kept.seconds % 60).toFixed(1).padStart(4, "0")} of jamming in ${mock.project.name}`;
  mock.noticeSeq += 1;
  return mockEdit("keep jam take");
}

/**
 * Saves the jam as it is now to `~/Music/Cantina/Jams` as `name`, with a
 * genre and a line on it; one of that name is replaced only when asked.
 */
export async function jamSave(
  name: string,
  genre: string,
  description: string,
  replace = false,
): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_save", { name, genre, description, replace });
  if (!mockJam) throw new Error("open a jam first");
  if (!name.trim()) throw new Error("a jam needs a name");
  const path = `/Users/you/Music/Cantina/Jams/${rigSlug(name)}.jam.json`;
  const at = mockUserJams.findIndex((j) => j.info.path === path);
  if (at >= 0 && !replace) throw new Error(`you already have a jam called ${name.trim()}`);
  mockJam.setup.name = name.trim();
  mockJam.setup.genre = genre.trim();
  mockJam.setup.description = description.trim();
  const file = structuredClone(mockJam.setup);
  const entry = { info: jamInfo(file, path, false), file };
  if (at >= 0) mockUserJams[at] = entry;
  else mockUserJams.push(entry);
  return mockStatus();
}

/** Opens a jam file, stock or the user's, in place of the jam playing. */
export async function jamLoad(path: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_load", { path });
  const jam = mockStockJamFiles.get(path) ?? mockUserJams.find((j) => j.info.path === path)?.file;
  if (!jam) throw new Error(`no jam at ${path}`);
  return jamSet(structuredClone(jam));
}

/** Sets how many minutes of input a jam keeps: 1, 2, 5 or 10. New rings start empty. */
export async function jamCaptureSet(minutes: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_capture_set", { minutes });
  if (![1, 2, 5, 10].includes(minutes)) throw new Error(`the capture keeps 1, 2, 5 or 10 minutes`);
  mockCaptureMinutes = minutes;
  return mockStatus();
}

/** The stock progressions, as the shell lists them (cantina-project's `PRESETS`). */
const MOCK_PRESETS: JamPreset[] = [
  {
    genre: "Blues",
    name: "12-bar blues",
    chords: "I7 | I7 | I7 | I7 | IV7 | IV7 | I7 | I7 | V7 | IV7 | I7 | V7",
  },
  {
    genre: "Blues",
    name: "Quick-change blues",
    chords: "I7 | IV7 | I7 | I7 | IV7 | IV7 | I7 | I7 | V7 | IV7 | I7 | V7",
  },
  {
    genre: "Blues",
    name: "Minor blues",
    chords: "i7 | i7 | i7 | i7 | iv7 | iv7 | i7 | i7 | bVI7 | V7 | i7 | V7",
  },
  { genre: "Rock", name: "I–bVII–IV", chords: "I | bVII | IV | I" },
  { genre: "Rock", name: "I–IV–V", chords: "I | IV | V | IV" },
  { genre: "Pop", name: "I–V–vi–IV", chords: "I | V | vi | IV" },
  { genre: "Pop", name: "vi–IV–I–V", chords: "vi | IV | I | V" },
  { genre: "Punk", name: "Power chords", chords: "I5 | IV5 | V5 | IV5" },
  { genre: "Metal", name: "Aeolian", chords: "i5 | bVI5 | bVII5 | i5" },
  { genre: "Metal", name: "Phrygian", chords: "i5 | i5 | bII5 | i5" },
  { genre: "Grunge", name: "i–bIII–bVII–IV", chords: "i | bIII | bVII | IV" },
  { genre: "Funk", name: "Dorian vamp", chords: "i7 | i7 | IV7 | IV7" },
  { genre: "Funk", name: "One-chord groove", chords: "I9 | I9 | I9 | IV9" },
  { genre: "Jazz", name: "ii–V–I", chords: "ii7 | V7 | Imaj7 | Imaj7" },
  { genre: "Jazz", name: "Turnaround", chords: "Imaj7 | vi7 | ii7 | V7" },
  { genre: "Reggae", name: "I–IV", chords: "I | IV | I | V" },
  { genre: "Hip-hop", name: "Minor loop", chords: "i7 | bVImaj7 | iv7 | V7" },
  { genre: "Country", name: "I–IV–I–V", chords: "I | IV | I | V" },
];

/** The audio tracks of a project a jam can play as its backing. */
export async function jamTracks(project: string): Promise<JamTrack[]> {
  if (inTauri) return invoke<JamTrack[]>("jam_tracks", { project });
  if (project === mock.project.name) {
    return mock.project.tracks
      .filter((t) => !t.kit && t.clips.length > 0)
      .map((t) => ({
        name: t.name,
        bpm: mock.project.tempo.bpm,
        seconds: Math.max(...t.clips.map((c) => (c.start + c.length) / MOCK_RATE)),
      }));
  }
  return [
    { name: "Rhythm", bpm: 96, seconds: 32 },
    { name: "Bass", bpm: 96, seconds: 32 },
  ];
}

function mockJamRestart(jam: NonNullable<typeof mockJam>, first?: GrooveSection) {
  const bars = mockJamPatterns(jam.setup);
  const has = JAM_ORDER.filter((s) => bars[s] !== undefined);
  const start =
    first ?? (has.includes("intro") ? "intro" : has.includes("verse") ? "verse" : has[0]);
  jam.sections = [];
  jam.tempos = [{ tick: 0, bpm: jam.setup.tempo }];
  jam.end = null;
  if (start) mockJamSwitch(jam, 0, start);
}

function mockJamSwitch(jam: NonNullable<typeof mockJam>, at: number, section: GrooveSection) {
  const bars = mockJamPatterns(jam.setup);
  const length = (bars[section] ?? 1) * mockJamBarTicks(jam.setup);
  const back =
    [...jam.sections]
      .reverse()
      .filter((m) => m.tick < at)
      .find((m) => ["verse", "chorus", "bridge"].includes(m.section))?.section ?? "verse";
  jam.sections = jam.sections.filter((m) => m.tick < at);
  jam.sections.push({ tick: at, section });
  jam.end = null;
  if (section === "intro" || section === "fill")
    jam.sections.push({ tick: at + length, section: back });
  if (section === "ending") jam.end = at + length;
}

/** The stand-in jam's beat, through its tempo changes, as the pump works it out. */
function mockJamBeat(jam: NonNullable<typeof mockJam>, seconds: number) {
  const tick = mockJamTick(jam.tempos, seconds);
  const beats = tick / JAM_PPQ;
  const whole = Math.floor(beats);
  const perBar = jam.setup.timeSignature.numerator;
  const bpm = [...jam.tempos].reverse().find((t) => t.tick <= tick)?.bpm ?? jam.setup.tempo;
  return { bpm, phase: beats - whole, bar: Math.floor(whole / perBar), beat: whole % perBar };
}

function mockJamNextBar(jam: NonNullable<typeof mockJam>): number {
  const bar = mockJamBarTicks(jam.setup);
  return Math.ceil(mockJamTick(jam.tempos, mockTransport.position + 0.03) / bar) * bar;
}

function mockJamDefault(): Jam {
  return {
    version: 1,
    name: "Jam",
    tempo: 100,
    timeSignature: { numerator: 4, denominator: 4 },
    key: { tonic: "E", mode: "minor" },
    drums: { kit: mockKits[0]?.id ?? "studio", groove: "Rock", levelDb: 0 },
    rig: "classic-rock",
  };
}

let mockJamLast: Jam | null = null;
let mockProjectPosition = 0;

/** Opens jam mode: the last jam is heard in place of the project, which it never edits. */
export async function jamOpen(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_open");
  if (mockTransport.recording) throw new Error("stop recording before jamming");
  // Practice gives way to a jam.
  if (mockPractice) await jamClose();
  if (!mockJam) {
    mockProjectPosition = mockTransport.position;
    mockTransport.playing = false;
    mockTransport.position = 0;
    mockJam = { setup: mockJamLast ?? mockJamDefault(), sections: [], tempos: [], end: null };
    mockJamRestart(mockJam);
    // `?jambacking` gives it a bass playing a blues, for screenshots.
    if (mockParams?.has("jambacking")) {
      mockJam.setup.backing = {
        kind: "progression",
        chords: "I7 | IV7 | I7 | V7",
        instrument: "bass",
        rhythm: "quarter",
        rig: "bass-clean",
        levelDb: -2,
      };
    }
    // `?jamgroove=<name>` plays one of the stand-in's grooves (with `?grooves`).
    const named = mockParams?.get("jamgroove");
    const mine = mockUserGrooves.find((g) => g.file.name === named)?.file;
    if (mine) {
      mockJam.setup.drums = { ...mockJam.setup.drums, groove: mine.name, patterns: {} };
      mockJam.setup.timeSignature = { ...mine.timeSignature };
      mockJam.setup.tempo = mine.tempo;
      mockJamRestart(mockJam);
    }
    // `?jam=playing` has it rolling with a chorus and a new tempo coming,
    // for screenshots; `?position` says where.
    if (mockParams?.get("jam") === "playing") {
      const bar = mockJamBarTicks(mockJam.setup);
      mockJamSwitch(mockJam, 6 * bar, "chorus");
      mockJam.tempos.push({ tick: 8 * bar, bpm: 112 });
      mockTransport.playing = true;
      mockTransport.position = Number(mockParams.get("position") ?? 9);
    }
  }
  return mockStatus();
}

/** Leaves jam mode, or practice: the project is heard again, its playhead where it was. */
export async function jamClose(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_close");
  if (mockJam) {
    mockJam = null;
    mockPractice = null;
    mockTransport.playing = false;
    mockTransport.countIn = 0;
    mockTransport.position = mockProjectPosition;
  }
  return mockStatus();
}

/** Changes the jam's groove, kit, tempo, key or rig; remembered for next time. */
export async function jamSet(setup: Jam): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_set", { setup });
  if (!mockJam) throw new Error("open a jam first");
  if (!setup.drums.groove && Object.keys(setup.drums.patterns ?? {}).length === 0) {
    throw new Error("the drums have neither a groove nor patterns");
  }
  const groove = mockGrooves.find((g) => g.genre === setup.drums.groove && g.section === "verse");
  const next = structuredClone(setup);
  if (groove) next.timeSignature = { ...groove.timeSignature };
  const mine = mockUserGrooves.find((g) => g.file.name === setup.drums.groove)?.file;
  if (!groove && mine) next.timeSignature = { ...mine.timeSignature };
  if (next.backing?.kind === "audio") next.tempo = next.backing.bpm;
  if (next.backing?.kind === "track") {
    const project = next.backing.project;
    next.tempo = project === mock.project.name ? mock.project.tempo.bpm : 96;
  }
  const rolling = mockTransport.playing || mockTransport.countIn > 0;
  const tempoChanged = next.tempo !== (mockJam.tempos.at(-1)?.bpm ?? mockJam.setup.tempo);
  mockJam.setup = next;
  if (!rolling) mockJamRestart(mockJam);
  else if (tempoChanged) {
    const at = mockJamNextBar(mockJam);
    mockJam.tempos = mockJam.tempos.filter((t) => t.tick < at);
    mockJam.tempos.push({ tick: at, bpm: next.tempo });
  }
  mockJamLast = structuredClone(next);
  return mockStatus();
}

/** Plays the jam from the top, after a count-in when count-in is on. */
export async function jamPlay(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_play");
  if (!mockJam) throw new Error("open a jam first");
  if (mockPractice) mockPracticeSettle();
  else mockJam.setup.tempo = mockJam.tempos.at(-1)?.bpm ?? mockJam.setup.tempo;
  const first = mockJam.sections[0]?.section;
  mockJamRestart(mockJam, mockTransport.playing ? undefined : first);
  mockTransport.position = 0;
  const beats = mockPractice
    ? (mockPractice.exercise.countIn ?? 1) * mockJam.setup.timeSignature.numerator
    : MOCK_COUNT_BEATS;
  if (mockPractice) {
    mockPractice.played = true;
    mockPracticePlan(mockPractice, mockJam);
  }
  const counting = mock.countIn && beats > 0;
  mockTransport.countIn = counting ? beats * (60 / mockJam.setup.tempo) : 0;
  mockTransport.playing = !counting;
  return mockStatus();
}

/** Stops the jam and goes back to the top. */
export async function jamStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_stop");
  mockPracticeSettle();
  mockTransport.playing = false;
  mockTransport.countIn = 0;
  mockTransport.position = 0;
  if (mockJam) mockJamRestart(mockJam);
  return mockStatus();
}

/** Plays a section from the next bar; stopped, the jam starts with it. */
export async function jamSection(section: GrooveSection): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_section", { section });
  if (!mockJam) throw new Error("open a jam first");
  if (mockJamPatterns(mockJam.setup)[section] === undefined) {
    throw new Error(`the drums have no ${section}`);
  }
  if (mockTransport.playing || mockTransport.countIn > 0) {
    mockJamSwitch(mockJam, mockJamNextBar(mockJam), section);
  } else {
    mockJamRestart(mockJam, section);
  }
  return mockStatus();
}

/** Sets the jam's tempo: playing, from the next bar; stopped, from the top. */
export async function jamTempo(bpm: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("jam_tempo", { bpm });
  if (!mockJam) throw new Error("open a jam first");
  const kind = mockJam.setup.backing?.kind;
  if (kind === "audio" || kind === "track") {
    throw new Error("the backing plays at its own tempo; the jam follows it");
  }
  const tempo = Math.round(Math.min(300, Math.max(30, bpm)) * 10) / 10;
  if (mockTransport.playing || mockTransport.countIn > 0) {
    const at = mockJamNextBar(mockJam);
    mockJam.tempos = mockJam.tempos.filter((t) => t.tick < at);
    mockJam.tempos.push({ tick: at, bpm: tempo });
  } else {
    mockJam.setup.tempo = tempo;
    const first = mockJam.sections[0]?.section;
    mockJamRestart(mockJam, first);
  }
  if (mockPractice)
    mockPracticeLast = { ...mockPracticeOptions(mockPractice), path: mockPractice.path, tempo };
  else mockJamLast = { ...structuredClone(mockJam.setup), tempo };
  return mockStatus();
}

// ----- practice -----

/** The stand-in's practice: the exercise and how it is played, over `mockJam`. */
let mockPractice: {
  path: string;
  exercise: Exercise;
  looping: boolean;
  groove: boolean;
  rig: string | null;
  climb: number;
  played: boolean;
} | null = null;
let mockPracticeLast: (PracticeOptions & { path: string; tempo: number }) | null = null;

function mockPracticeOptions(p: NonNullable<typeof mockPractice>): PracticeOptions {
  return { looping: p.looping, groove: p.groove, rig: p.rig, climb: p.climb };
}

/** The stand-in's practice log, as `Practice/log.json` keeps it. `?practicelog` fills it. */
const mockLog: PracticeDay[] = mockParams?.has("practicelog")
  ? (() => {
      const day = (back: number) => {
        const d = new Date();
        d.setDate(d.getDate() - back);
        return mockDate(d);
      };
      return [
        { exercise: "a-to-d", date: day(3), tempo: 70, minutes: 9 },
        { exercise: "a-to-d", date: day(1), tempo: 80, minutes: 12 },
        { exercise: "pentatonic-box-1", date: day(1), tempo: 85, minutes: 6.5 },
        { exercise: "a-to-d", date: day(0), tempo: 75, minutes: 4 },
        { exercise: "downpicking-endurance", date: day(2), tempo: 210, minutes: 20 },
        { exercise: "downpicked-power-chords", date: day(1), tempo: 180, minutes: 15 },
        { exercise: "power-chords-up-the-neck", date: day(0), tempo: 120, minutes: 8 },
      ];
    })()
  : [];

function mockDate(d: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}

/** An exercise's name in the log: its file less `.exercise.json`. */
function mockId(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.exercise\.json$/, "");
}

function mockBest(path: string): number | null {
  const id = mockId(path);
  const tempos = mockLog.filter((d) => d.exercise === id && d.tempo > 0).map((d) => d.tempo);
  return tempos.length > 0 ? Math.max(...tempos) : null;
}

function mockLastDay(path: string): PracticeDay | null {
  const id = mockId(path);
  return [...mockLog].reverse().find((d) => d.exercise === id) ?? null;
}

function mockStreak(): number {
  const days = new Set(mockLog.map((d) => d.date));
  const d = new Date();
  if (!days.has(mockDate(d))) d.setDate(d.getDate() - 1);
  let count = 0;
  while (days.has(mockDate(d))) {
    count++;
    d.setDate(d.getDate() - 1);
  }
  return count;
}

/** Plans the stand-in's climb from the top: a rung every `climb` passes. */
function mockPracticePlan(p: NonNullable<typeof mockPractice>, jam: NonNullable<typeof mockJam>) {
  const start = jam.tempos[0] ?? { tick: 0, bpm: jam.setup.tempo };
  jam.tempos = [start];
  if (p.climb <= 0) return;
  const every = loopTicks(p.exercise) * p.climb;
  const { goal, step } = p.exercise.tempo;
  let bpm = start.bpm;
  for (let at = every; step > 0 && bpm < goal - 1e-6; at += every) {
    bpm = Math.round(Math.min(goal, bpm + step) * 10) / 10;
    jam.tempos.push({ tick: at, bpm });
  }
}

/** After the stand-in's practice rolled: log it and carry on at its tempo. */
function mockPracticeSettle() {
  const p = mockPractice;
  const jam = mockJam;
  if (!p || !jam || !p.played) return;
  p.played = false;
  const tick = mockJamTick(jam.tempos, mockTransport.position);
  const period = loopTicks(p.exercise);
  const at = (t: number) =>
    [...jam.tempos].reverse().find((m) => m.tick <= t)?.bpm ?? jam.setup.tempo;
  let fastest = 0;
  for (let k = 0; (k + 1) * period <= tick; k++) {
    fastest = Math.max(fastest, Math.min(at(k * period), at((k + 1) * period - 1)));
  }
  jam.setup.tempo = at(tick);
  if (mockTransport.position < 1) return;
  const id = mockId(p.path);
  const date = mockDate(new Date());
  const day = mockLog.find((d) => d.exercise === id && d.date === date);
  if (day) {
    day.tempo = Math.max(day.tempo, fastest);
    day.minutes += mockTransport.position / 60;
  } else {
    mockLog.push({ exercise: id, date, tempo: fastest, minutes: mockTransport.position / 60 });
  }
}

function mockExercise(path: string): Exercise {
  const found = Object.entries(EXERCISE_FILES).find(([file]) =>
    path.endsWith(file.replace(/^(\.\.\/)+/, "")),
  );
  if (!found) throw new Error(`there is no exercise at ${path}`);
  return structuredClone(found[1]);
}

function mockPracticeStatus(
  p: NonNullable<typeof mockPractice>,
  jam: NonNullable<typeof mockJam>,
): PracticeStatus {
  return {
    path: p.path,
    exercise: structuredClone(p.exercise),
    tab: exerciseTab(p.exercise),
    tempo: jam.tempos.at(-1)?.bpm ?? jam.setup.tempo,
    looping: p.looping,
    groove: p.groove && Boolean(p.exercise.drums),
    rig: p.rig,
    loopTicks: loopTicks(p.exercise),
    exampleMissing: [],
    climb: p.climb,
    best: mockBest(p.path),
    last: mockLastDay(p.path),
    streak: mockStreak(),
    todayMinutes: mockLog
      .filter((d) => d.date === mockDate(new Date()))
      .reduce((m, d) => m + d.minutes, 0),
  };
}

/** Builds the stand-in jam an exercise plays as: its tempo, key, groove's verse and rig. */
function mockPracticeJam(p: NonNullable<typeof mockPractice>, tempo: number) {
  const e = p.exercise;
  const setup: Jam = {
    version: 1,
    name: e.name,
    tempo,
    timeSignature: e.timeSignature ?? { numerator: 4, denominator: 4 },
    key: { ...e.key },
    drums: {
      kit: e.drums?.kit ?? mockKits[0]?.id ?? "studio",
      groove: e.drums?.groove ?? "Rock",
      levelDb: e.drums?.levelDb ?? 0,
    },
    rig: p.rig ?? e.rig ?? null,
  };
  const jam = { setup, sections: [], tempos: [], end: null };
  mockJamRestart(jam, "verse");
  return jam;
}

/**
 * Opens practice: the exercise at `path`, or the last one at the tempo it
 * was left at, heard in place of the project, which practice never edits.
 * Play, stop, tempo and leaving are the jam's: `jamPlay`, `jamStop`,
 * `jamTempo` and `jamClose`.
 */
export async function practiceOpen(path?: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("practice_open", { path: path ?? null });
  if (mockTransport.recording) throw new Error("stop recording before practicing");
  const last = mockPracticeLast;
  const chosen = path ?? last?.path ?? mockExercises()[0]?.path;
  if (!chosen) throw new Error("there are no exercises to practice");
  const exercise = mockExercise(chosen);
  const same = last?.path === chosen ? last : null;
  if (!mockJam) {
    mockProjectPosition = mockTransport.position;
    mockTransport.playing = false;
    mockTransport.position = 0;
  }
  mockTransport.playing = false;
  mockTransport.countIn = 0;
  mockTransport.position = 0;
  mockPractice = {
    path: chosen,
    exercise,
    looping: same?.looping ?? true,
    groove: same?.groove ?? true,
    rig: same?.rig ?? null,
    climb: last?.climb ?? 4,
    played: false,
  };
  mockJam = mockPracticeJam(mockPractice, same?.tempo ?? exercise.tempo.start);
  // `?exercise=playing` has it rolling in its second pass, for screenshots.
  if (mockParams?.get("exercise") === "playing" && !path) {
    mockPractice.played = true;
    mockPracticePlan(mockPractice, mockJam);
    mockTransport.playing = true;
    mockTransport.position = Number(mockParams.get("position") ?? 6.5);
  }
  mockPracticeLast = {
    ...mockPracticeOptions(mockPractice),
    path: chosen,
    tempo: mockJam.setup.tempo,
  };
  return mockStatus();
}

/** Practices another exercise, from its start tempo. */
export async function practiceLoad(path: string): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("practice_load", { path });
  if (!mockPractice) return practiceOpen(path);
  const exercise = mockExercise(path);
  mockPracticeSettle();
  mockTransport.playing = false;
  mockTransport.countIn = 0;
  mockTransport.position = 0;
  mockPractice = { ...mockPractice, path, exercise, rig: null, played: false };
  mockJam = mockPracticeJam(mockPractice, exercise.tempo.start);
  mockPracticeLast = { ...mockPracticeOptions(mockPractice), path, tempo: exercise.tempo.start };
  return mockStatus();
}

/** Turns practice's loop or groove on or off, or picks the rig the input plays through. */
export async function practiceOptions(options: PracticeOptions): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("practice_options", { options });
  if (!mockPractice || !mockJam) throw new Error("open practice first");
  mockPractice = { ...mockPractice, ...options };
  mockJam.setup.rig = options.rig ?? mockPractice.exercise.rig ?? null;
  mockPracticeLast = {
    ...options,
    path: mockPractice.path,
    tempo: mockJam.tempos.at(-1)?.bpm ?? mockJam.setup.tempo,
  };
  return mockStatus();
}

// ----- the drum designer -----

/** A groove file: a whole drum part. Mirrors `GrooveFile` in cantina-project. */
export interface GrooveFile extends DrumPart {
  version: number;
  name: string;
  genre?: string;
  description?: string;
  tempo: number;
  timeSignature: { numerator: number; denominator: number };
}

/** The skill's example groove, which `?grooves` puts in the stand-in's library. */
const EXAMPLE_GROOVES = import.meta.glob<GrooveFile>(
  "../../../.claude/skills/cantina-author/examples/*.groove.json",
  { eager: true, import: "default" },
);

/** The stand-in's groove files in `~/Music/Cantina/Grooves`. */
const mockUserGrooves: { path: string; file: GrooveFile }[] = mockParams?.has("grooves")
  ? Object.entries(EXAMPLE_GROOVES).map(([f, file]) => ({
      path: `/Users/you/Music/Cantina/Grooves/${f.split("/").pop() ?? "groove.groove.json"}`,
      file: structuredClone(file),
    }))
  : [];

function mockGrooveInfo(path: string, g: GrooveFile): GrooveInfo {
  const bar = (960 * 4 * g.timeSignature.numerator) / g.timeSignature.denominator;
  const ticks = partLayout(g).reduce((end, s) => Math.max(end, s.start + s.length), 0);
  return {
    name: g.name,
    genre: g.genre ?? "",
    description: g.description ?? "",
    path,
    tempo: g.tempo,
    timeSignature: { ...g.timeSignature },
    kit: g.kit,
    sections: g.sections.map((s) => ({ name: s.name, kind: s.kind ?? null })),
    bars: Math.ceil(ticks / bar),
  };
}

function mockSyncGrooves() {
  mock.library.grooves = mockUserGrooves
    .map((g) => mockGrooveInfo(g.path, g.file))
    .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
}
mockSyncGrooves();

/** A pattern file, a stock groove's or the library's, for the designer's palette. Its id is 0. */
export async function patternFileRead(path: string): Promise<PatternInfo> {
  if (inTauri) return invoke<PatternInfo>("pattern_file_read", { path });
  const g = mockGrooveFile(path);
  if (g) {
    return {
      id: 0,
      name: g.name,
      stepsPerBeat: g.stepsPerBeat,
      length: g.length,
      swing: g.swing,
      rows: structuredClone(g.rows),
    };
  }
  const name = (path.split("/").pop() ?? "Pattern").replace(/\.pattern\.json$/, "");
  return { ...mockPattern(0, name), id: 0 };
}

/** A groove file of the user's, for the designer to open. */
export async function grooveFileRead(path: string): Promise<GrooveFile> {
  if (inTauri) return invoke<GrooveFile>("groove_file_read", { path });
  const g = mockUserGrooves.find((x) => x.path === path);
  if (!g) throw new Error(`no groove at ${path}`);
  return structuredClone(g.file);
}

/**
 * Saves the designer's part to ~/Music/Cantina/Grooves as `name`, with the
 * project's tempo and time signature, where the palette and the jam's
 * groove picker find it.
 */
export async function drumDesignSaveGroove(
  name: string,
  genre: string,
  description: string,
  replace: boolean,
): Promise<EngineStatus> {
  if (inTauri) {
    return invoke<EngineStatus>("drum_design_save_groove", { name, genre, description, replace });
  }
  if (!mockDesign) throw new Error("open the drum designer first");
  if (!name.trim()) throw new Error("a groove needs a name");
  const path = `/Users/you/Music/Cantina/Grooves/${rigSlug(name)}.groove.json`;
  const at = mockUserGrooves.findIndex((g) => g.path === path);
  if (at >= 0 && !replace) throw new Error(`you already have a groove called ${name.trim()}`);
  const { numerator, denominator } = mock.project.tempo;
  const file: GrooveFile = {
    version: 1,
    name: name.trim(),
    ...(genre.trim() ? { genre: genre.trim() } : {}),
    ...(description.trim() ? { description: description.trim() } : {}),
    tempo: mock.project.tempo.bpm,
    timeSignature: { numerator, denominator },
    ...structuredClone(mockDesign.part),
  };
  if (at >= 0) mockUserGrooves[at] = { path, file };
  else mockUserGrooves.push({ path, file });
  mockSyncGrooves();
  return mockStatus();
}

/** The stand-in's drum designer, as the shell keeps it. */
let mockDesign: {
  track: number;
  part: DrumPart;
  base: DrumPart;
  plays: DesignPlays;
  projectPosition: number;
} | null = null;

function mockBarTicks(): number {
  const { numerator, denominator } = mock.project.tempo;
  return ((960 * 4) / denominator) * numerator;
}

/** What the stand-in's designer plays: as the shell has it, changes landing at once. */
function mockDesignPlan(d: NonNullable<typeof mockDesign>): {
  spans: PlaySpan[];
  loopStart: number;
  loopEnd: number;
} {
  const bar = mockBarTicks();
  if (d.plays.kind === "section") {
    const name = d.plays.name;
    const section = d.part.sections.find((s) => s.name === name);
    if (!section) return { spans: [], loopStart: 0, loopEnd: 0 };
    // A whole round of its variations.
    const ticks = Math.max(1, patternTicks(section.pattern) * passes(section).length);
    const every = (section.fill?.every ?? 0) * bar;
    const length = Math.ceil(Math.max(every, ticks) / ticks) * ticks;
    return {
      spans: [{ start: 0, length, section: name, step: null }],
      loopStart: 0,
      loopEnd: length,
    };
  }
  const layout = partLayout(d.part);
  const total = layout.reduce((end, s) => Math.max(end, s.start + s.length), 0);
  const skip = layout[d.plays.from]?.start ?? 0;
  const at = skip > 0 ? total - skip : 0;
  const span = (s: (typeof layout)[number], start: number): PlaySpan => ({
    start,
    length: s.length,
    section: d.part.sections[s.section]?.name ?? "",
    step: s.step,
  });
  const spans = [
    ...(skip > 0 ? layout.filter((s) => s.start >= skip).map((s) => span(s, s.start - skip)) : []),
    ...layout.map((s) => span(s, at + s.start)),
  ];
  return { spans, loopStart: at, loopEnd: at + total };
}

function mockDesignStatus(d: NonNullable<typeof mockDesign>): DrumDesignStatus {
  const edit = mockKitEdit;
  return {
    placed: mockPlaced.has(d.track),
    trackClips: mock.project.tracks.find((t) => t.id === d.track)?.patternClips.length ?? 0,
    pads: edit
      ? edit.kit.pads.map((p) => ({ id: p.id, name: p.name }))
      : mockKitStatus(d.part.kit).pads,
    kitEdit: edit
      ? {
          from: edit.from,
          kit: structuredClone(edit.kit),
          recipe: edit.recipe ? structuredClone(edit.recipe) : null,
          changed: JSON.stringify(edit.kit) !== JSON.stringify(edit.base),
        }
      : null,
    track: d.track,
    part: structuredClone(d.part),
    plays: { ...d.plays },
    changed: JSON.stringify(d.part) !== JSON.stringify(d.base),
    barTicks: mockBarTicks(),
    ...mockDesignPlan(d),
  };
}

/**
 * Opens the drum designer on a drum track: a copy of its part is heard in
 * place of the project, which stays as it is until the part is placed.
 */
export async function drumDesignOpen(track: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_open", { track });
  if (mockDesign?.track === track) return mockStatus();
  if (mockDesign) throw new Error("the drum designer is open on another track");
  if (mockJam) throw new Error("leave the jam before designing drums");
  if (mockTransport.recording) throw new Error("stop recording before designing drums");
  const t = mockTrack(track);
  if (!t.kit) throw new Error(`${t.name} is not a drum track`);
  const part =
    structuredClone(mockPlaced.get(track)?.part) ??
    partFromTrack(t.kit.id, t.patternClips, mock.project.patterns);
  mockDesign = {
    track,
    part,
    base: structuredClone(part),
    plays:
      part.order.length > 0
        ? { kind: "order", from: 0 }
        : { kind: "section", name: part.sections[0]?.name ?? "" },
    projectPosition: mockTransport.position,
  };
  mock.audition = null;
  mockTransport.playing = false;
  mockTransport.position = 0;
  // `?variations` gives the first section a B with a ghost, a flam and a
  // ratchet, and the velocity lane something to show.
  if (mockParams?.has("variations")) {
    const first = part.sections[0];
    if (first) {
      const varied = addVariation(first, 0);
      const b = varied.variations?.[0];
      const snare = b?.rows.find((r) => r.pad === "snare");
      if (snare) {
        snare.steps[6] = 0.3;
        snare.steps[14] = 0.3;
        snare.marks = [{ step: 12, flam: true }];
      }
      const hat = b?.rows.find((r) => r.pad === "hh-closed");
      if (hat) {
        hat.steps = hat.steps.map((_, i) =>
          i % 2 === 0 ? ([1, 0.5, 0.8, 0.5][(i / 2) % 4] ?? 0) : 0,
        );
        hat.marks = [{ step: 14, ratchet: 3 }];
      }
      part.sections[0] = varied;
      mockDesign.base = structuredClone(part);
    }
  }
  // `?workbench=open` opens the kit workbench with the snare tuned down.
  if (mockParams?.get("workbench") === "open") {
    const edit = mockKitEditStart();
    const snare = edit.kit.pads.find((p) => p.id === "snare");
    if (snare) {
      snare.tune = -2;
      snare.decay = 0.6;
    }
  }
  // `?drumdesign=playing` has it rolling, for screenshots; `?position` says where.
  if (mockParams?.get("drumdesign") === "playing") {
    mockTransport.playing = true;
    mockTransport.position = Number(mockParams.get("position") ?? 0.6);
  }
  return mockStatus();
}

/** Closes the drum designer: the project is heard again, its playhead where it was. */
export async function drumDesignClose(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_close");
  mockKitEdit = null;
  if (mockDesign) {
    mockTransport.playing = false;
    mockTransport.position = mockDesign.projectPosition;
    mockDesign = null;
  }
  return mockStatus();
}

/** Replaces the designer's part, heard at once. */
export async function drumDesignSet(part: DrumPart): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_set", { part });
  if (!mockDesign) throw new Error("open the drum designer first");
  if (part.sections.length === 0) throw new Error("the part has no sections");
  const names = new Set(part.sections.map((s) => s.name));
  if (names.size !== part.sections.length) throw new Error("two sections have the same name");
  for (const [i, o] of part.order.entries()) {
    if (!names.has(o.section)) {
      throw new Error(
        `step ${i + 1} of the order plays ${o.section}, which the part does not have`,
      );
    }
  }
  mockDesign.part = structuredClone(part);
  const plays = mockDesign.plays;
  const keep =
    plays.kind === "order" ? plays.from < partLayout(part).length : names.has(plays.name);
  if (!keep) {
    mockDesign.plays =
      part.order.length > 0
        ? { kind: "order", from: 0 }
        : { kind: "section", name: part.sections[0]?.name ?? "" };
  }
  return mockStatus();
}

/**
 * Plays a section, or the order from one of its steps, on loop in the
 * designer. While it rolls the shell lands the change on the next bar
 * line; the stand-in changes at once.
 */
export async function drumDesignPlays(plays: DesignPlays): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_plays", { plays });
  if (!mockDesign) throw new Error("open the drum designer first");
  if (plays.kind === "section" && !mockDesign.part.sections.some((s) => s.name === plays.name)) {
    throw new Error(`the part has no section ${plays.name}`);
  }
  if (plays.kind === "order" && mockDesign.part.order.length === 0) {
    throw new Error("the order is empty");
  }
  if (plays.kind === "order" && plays.from >= partLayout(mockDesign.part).length) {
    throw new Error(`the order has no step ${plays.from + 1}`);
  }
  if (JSON.stringify(plays) !== JSON.stringify(mockDesign.plays)) {
    mockDesign.plays = { ...plays };
    mockTransport.position = 0;
  }
  return mockStatus();
}

/**
 * Plays the designer's part from the top of its loop; with `countIn`, and
 * count-in on, after a bar of clicks with the timeline held.
 */
export async function drumDesignPlay(countIn = false): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_play", { countIn });
  if (!mockDesign) throw new Error("open the drum designer first");
  mockTransport.position = 0;
  const counting = countIn && mock.countIn;
  mockTransport.countIn = counting ? mock.project.tempo.numerator * mockBeatSeconds() : 0;
  mockTransport.playing = !counting;
  return mockStatus();
}

/** Stops the designer's part and goes back to the top. */
export async function drumDesignStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_stop");
  mockTransport.playing = false;
  mockTransport.position = 0;
  return mockStatus();
}

// ----- placing a part -----

/** What the stand-in's designer placed, by track: as `PlacedPart` in cantina-project. */
const mockPlaced = new Map<
  number,
  { part: DrumPart; start: number; clips: number[]; patterns: number[] }
>();

/**
 * Places the designer's part on its track as one undoable edit: in place
 * of what was placed before (or the track's clips, the first time), or
 * after the track's last clip, on the next bar.
 */
export async function drumDesignPlace(mode: PlaceMode): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_place", { mode });
  if (!mockDesign) throw new Error("open the drum designer first");
  const d = mockDesign;
  const t = mockTrack(d.track);
  const spans = partLayout(d.part);
  if (spans.length === 0) throw new Error("the order is empty: there is nothing to place");
  const bar = mockBarTicks();
  const before = mockPlaced.get(d.track);
  const replaced =
    mode === "append"
      ? []
      : before
        ? t.patternClips.filter((c) => before.clips.includes(c.id))
        : [...t.patternClips];
  const start =
    mode === "append"
      ? Math.ceil(Math.max(0, ...t.patternClips.map((c) => c.start + c.length)) / bar) * bar
      : (before?.start ?? Math.min(...replaced.map((c) => c.start), Infinity));
  const at = Number.isFinite(start) ? start : 0;
  t.patternClips = t.patternClips.filter((c) => !replaced.some((r) => r.id === c.id));
  if (before && mode === "replace") {
    const played = new Set(
      mock.project.tracks.flatMap((x) =>
        x.patternClips.flatMap((c) => [c.pattern, ...(c.fill ? [c.fill.pattern] : [])]),
      ),
    );
    mock.project.patterns = mock.project.patterns.filter(
      (p) => !before.patterns.includes(p.id) || played.has(p.id),
    );
  }
  const ids = d.part.sections.map((s) => {
    const id = mockNextId++;
    mock.project.patterns.push({
      ...structuredClone(s.pattern),
      id,
      name: `${t.name} ${s.name}`,
    });
    return id;
  });
  const clips = spans.map((span) => {
    const section = d.part.sections[span.section];
    const fill = section?.fill;
    const fillAt = fill ? d.part.sections.findIndex((x) => x.name === fill.section) : -1;
    const id = mockNextId++;
    t.patternClips.push({
      id,
      pattern: ids[span.section] ?? 0,
      start: at + span.start,
      offset: 0,
      length: span.length,
      ...(fill && fillAt >= 0
        ? { fill: { pattern: ids[fillAt] ?? 0, every: fill.every, atEnd: fill.atEnd } }
        : {}),
    });
    return id;
  });
  t.patternClips.sort((a, b) => a.start - b.start);
  if (t.kit && t.kit.id !== d.part.kit) t.kit = mockKitStatus(d.part.kit);
  mockPlaced.set(d.track, { part: structuredClone(d.part), start: at, clips, patterns: ids });
  d.base = structuredClone(d.part);
  return mockEdit("place drum part");
}

// ----- the kit workbench -----

/** The stock kit recipes, read from the repository's `recipes/` at build time. */
const RECIPE_FILES = import.meta.glob<KitRecipe>("../../../recipes/*.recipe.json", {
  eager: true,
  import: "default",
});

/** The stand-in's workbench kit. */
let mockKitEdit: {
  from: string;
  kit: KitFile;
  base: KitFile;
  recipe: KitRecipe | null;
} | null = null;

function mockKitFile(id: string): KitFile {
  const status = mockKitStatus(id);
  return {
    version: 2,
    name: status.name,
    pads: status.pads.map((p) => ({
      id: p.id,
      name: p.name,
      ...(p.id.startsWith("hh-") ? { choke: "hat", pan: 0.25 } : {}),
      layers: [{ file: `${p.id}.wav` }],
    })),
  };
}

function mockKitEditStart(): NonNullable<typeof mockKitEdit> {
  if (!mockDesign) throw new Error("open the drum designer first");
  if (mockKitEdit) return mockKitEdit;
  const from = mockDesign.part.kit;
  const recipe =
    Object.entries(RECIPE_FILES).find(([f]) => f.endsWith(`/${from}.recipe.json`))?.[1] ?? null;
  const kit = recipe
    ? {
        version: 2,
        name: recipe.name,
        pads: recipe.pads.map((p) => ({
          id: p.id,
          name: p.name ?? p.id,
          ...(p.gainDb ? { gainDb: p.gainDb } : {}),
          ...(p.pan ? { pan: p.pan } : {}),
          ...(p.choke ? { choke: p.choke } : {}),
          layers: [{ file: `${p.id}-1.wav` }],
        })),
      }
    : mockKitFile(from);
  mockKitEdit = {
    from,
    kit,
    base: structuredClone(kit),
    recipe: recipe ? structuredClone(recipe) : null,
  };
  return mockKitEdit;
}

/** Opens the designer's kit workbench on the part's kit: a copy to change, heard as it changes. */
export async function drumDesignKitOpen(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_open");
  mockKitEditStart();
  return mockStatus();
}

/** Sets the workbench kit's pads: level, pan, tune, decay and choke group. */
export async function drumDesignKitSet(kit: KitFile): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_set", { kit });
  const edit = mockKitEditStart();
  for (const p of kit.pads) {
    if ((p.tune ?? 0) < -12 || (p.tune ?? 0) > 12) throw new Error(`pad ${p.id} is tuned too far`);
    const decay = p.decay ?? 1;
    if (decay < 0.05 || decay > 1) throw new Error(`pad ${p.id} has a decay outside 0.05 to 1`);
  }
  edit.kit = structuredClone(kit);
  return mockStatus();
}

/** Gives a workbench pad new samples: a WAV file, or another kit's pad. */
export async function drumDesignKitSwap(pad: string, source: PadSource): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_swap", { pad, source });
  const edit = mockKitEditStart();
  const p = edit.kit.pads.find((x) => x.id === pad);
  if (!p) throw new Error(`the kit has no pad ${pad}`);
  p.layers = [
    {
      file:
        source.from === "file"
          ? (source.path.split("/").pop() ?? `${pad}.wav`)
          : `${pad}-${source.kit}-1.wav`,
    },
  ];
  edit.recipe = null;
  return mockStatus();
}

/** Makes the workbench kit again from its recipe. */
export async function drumDesignKitRecipe(recipe: KitRecipe): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_recipe", { recipe });
  const edit = mockKitEditStart();
  edit.recipe = structuredClone(recipe);
  return mockStatus();
}

/** Puts the part's kit back as it was; the workbench closes. */
export async function drumDesignKitReset(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_reset");
  mockKitEdit = null;
  return mockStatus();
}

/** Saves the workbench kit as a kit of the user's in ~/Music/Cantina/Kits; the part plays it. */
export async function drumDesignKitSave(name: string, replace: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("drum_design_kit_save", { name, replace });
  const edit = mockKitEditStart();
  if (!name.trim()) throw new Error("a kit needs a name");
  const id = rigSlug(name);
  const there = mockKits.find((k) => k.id === id);
  if (there && !replace) throw new Error(`you already have a kit called ${name.trim()}`);
  if (!there) mockKits.push({ id, name: name.trim() });
  if (mockDesign) mockDesign.part.kit = id;
  void edit;
  mockKitEdit = null;
  return mockStatus();
}

/** Plays a pad of the kit the drum designer hears. */
export async function drumDesignAudition(pad: string, velocity: number): Promise<void> {
  if (inTauri) return invoke<void>("drum_design_audition", { pad, velocity });
}

/** The pads of an installed kit, for swapping from. */
export async function kitPads(kit: string): Promise<PadInfo[]> {
  if (inTauri) return invoke<PadInfo[]>("kit_pads", { kit });
  return mockKitStatus(kit).pads;
}

/** Asks for a WAV file to put on a pad. */
export async function chooseSamplePath(): Promise<string | null> {
  if (!inTauri) return "/Users/you/Samples/Big snare.wav";
  const chosen = await openDialog({
    multiple: false,
    filters: [{ name: "WAV audio", extensions: ["wav", "wave"] }],
  });
  return typeof chosen === "string" ? chosen : null;
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
  if (mock.practice.speed < 1) throw new Error("set the practice speed back to 100% to record");
  if (mock.running && mockSettings.noInput) {
    throw new Error("there is no input to record; pick one in Settings");
  }
  mockTransport.countIn = mock.countIn ? MOCK_COUNT_BEATS * mockBeatSeconds() : 0;
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
  const groove = mockGrooveFile(path);
  mock.project.patterns.push(
    groove
      ? {
          id: mockNextId++,
          name,
          stepsPerBeat: groove.stepsPerBeat,
          length: groove.length,
          swing: groove.swing,
          rows: groove.rows.map((r) => ({ pad: r.pad, steps: [...r.steps] })),
        }
      : { ...mockPattern(mockNextId++, name) },
  );
  return mockEdit("import pattern");
}

/** What a tab exports as. */
export type TabExportFormat = "musicxml" | "ascii" | "midi" | "pdf";

const TAB_FILTERS: Record<TabExportFormat, { name: string; extensions: string[] }> = {
  musicxml: { name: "MusicXML", extensions: ["musicxml"] },
  ascii: { name: "Text", extensions: ["txt"] },
  midi: { name: "MIDI", extensions: ["mid"] },
  pdf: { name: "PDF", extensions: ["pdf"] },
};

/** Asks where to write a tab export; null when the user cancels. */
export async function chooseTabExportPath(
  name: string,
  format: TabExportFormat,
): Promise<string | null> {
  const filter = TAB_FILTERS[format];
  const file = `${name || "Tab"}.${filter.extensions[0]}`;
  if (!inTauri) return `/Users/you/Music/${file}`;
  return saveDialog({ defaultPath: file, filters: [filter] });
}

/** Writes a clip's tab as MusicXML, ASCII tab or MIDI; resolves to the path written. */
export async function clipTabExport(
  id: number,
  format: Exclude<TabExportFormat, "pdf">,
  path: string,
): Promise<string> {
  if (inTauri) return invoke<string>("clip_tab_export", { id, format, path });
  const { clip } = mockClipAt(id);
  if (!clip.tab) throw new Error(`${clip.name} has no tab`);
  return path;
}

/** Writes a tab's PDF, drawn from the page view; resolves to the path written. */
export async function tabPdfWrite(path: string, bytes: Uint8Array): Promise<string> {
  if (inTauri) return invoke<string>("tab_pdf_write", { path, bytes: Array.from(bytes) });
  return path;
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

const RIG_FILTER = [{ name: "Rig", extensions: ["json"] }];

/** Asks where to write a rig file; null when the user cancels. */
export async function chooseRigExportPath(name: string): Promise<string | null> {
  const file = `${rigSlug(name)}.rig.json`;
  if (!inTauri) return `/Users/you/Desktop/${file}`;
  return saveDialog({ defaultPath: file, filters: RIG_FILTER });
}

/** Asks for a rig file to import; null when the user cancels. */
export async function chooseRigImport(): Promise<string | null> {
  if (!inTauri) return "/Users/you/Downloads/shared-tone.rig.json";
  const chosen = await openDialog({ multiple: false, filters: RIG_FILTER });
  return typeof chosen === "string" ? chosen : null;
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
  // `?padmixer` sets a few pads of the drum track, for screenshots.
  if (mockParams?.has("padmixer")) {
    const drums = mock.project.tracks.find((t) => t.kit);
    if (drums) {
      drums.pads = {
        kick: { levelDb: 2, pan: 0, tune: -2, decay: 0.7, mute: false },
        snare: { levelDb: -3.5, pan: -0.15, tune: 1, decay: 1, mute: false },
        "hh-open": { levelDb: 0, pan: 0, tune: 0, decay: 1, mute: true },
      };
    }
  }
  // `?feel` humanizes the first pattern and marks a few steps, for screenshots.
  if (mockParams?.has("feel")) {
    const [p] = mock.project.patterns;
    if (p) {
      p.humanize = { timing: 0.35, velocity: 0.5 };
      p.seed = 7;
      const at = (pad: string) => p.rows.find((r) => r.pad === pad);
      const hats = at("hh-closed");
      if (hats)
        hats.marks = [
          { step: 6, chance: 0.5 },
          { step: 10, chance: 0.75 },
        ];
      const snare = at("snare");
      if (snare)
        snare.marks = [
          { step: 4, flam: true },
          { step: 15, ratchet: 3 },
        ];
      const kick = at("kick");
      if (kick) kick.swing = 0.6;
    }
  }
  // `?fill` has the drum track's first clip play a fill, for screenshots.
  if (mockParams?.has("fill")) {
    const drums = mock.project.tracks.find((t) => t.kit);
    const [first] = drums?.patternClips ?? [];
    const other = mock.project.patterns.find((p) => p.id !== first?.pattern);
    if (first && other) first.fill = { pattern: other.id, every: 4, atEnd: true };
  }
  window.setInterval(() => {
    t += 1 / 60;
    if (mockTransport.playing) mockTransport.position += mock.practice.speed / 60;
    if (mockDesign && mockTransport.playing) {
      const { loopStart, loopEnd } = mockDesignPlan(mockDesign);
      const seconds = 60 / mock.project.tempo.bpm / 960;
      const [from, to] = [loopStart * seconds, loopEnd * seconds];
      if (to > from && mockTransport.position >= to) {
        mockTransport.position = from + ((mockTransport.position - to) % (to - from));
      }
    }
    if (mockTransport.countIn > 0 && !mockParams?.has("countin")) {
      mockTransport.countIn = Math.max(0, mockTransport.countIn - 1 / 60);
      mockTransport.playing = mockTransport.countIn === 0;
    }
    const anySolo = mock.project.tracks.some((t) => t.strip.solo);
    const meters = mock.running
      ? [
          // With no input the IN meter reads silence.
          mockSettings.noInput ? 0 : 0.4 + 0.3 * Math.abs(Math.sin(t * 3.7)),
          mockSettings.noInput ? 0 : 0.18 + 0.12 * Math.sin(t * 2.1),
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
    const beat = !mockTransport.playing
      ? null
      : mockJam
        ? mockJamBeat(mockJam, mockTransport.position)
        : mockBeat(mockTransport.position, bpm, numerator);
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
  what: "export" | "bounce" | "transcribe";
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

let mockCanceled = false;

/** The stand-in's render: progress in steps, a little slower than instant. */
async function mockRender(what: RenderProgress["what"]): Promise<void> {
  mockCanceled = false;
  for (let i = 1; i <= 10; i++) {
    await new Promise((r) => setTimeout(r, 60));
    if (mockCanceled) throw new Error("canceled");
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

/** Stops the running export, bounce or transcription; its promise fails with "canceled". */
export async function renderCancel(): Promise<void> {
  if (inTauri) return invoke("render_cancel");
  mockCanceled = true;
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
