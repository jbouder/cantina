import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  track: TrackStatus;
  /** The pedal chain in order. */
  chain: PedalInfo[];
  /** Specs `chainAdd` accepts. */
  pedalKinds: string[];
}

/** The one track. Mirrors `TrackStatus` in src-tauri. */
export interface TrackStatus {
  /** Armed tracks monitor their input and can record. */
  armed: boolean;
  recording: boolean;
  playing: boolean;
  take: TakeInfo | null;
}

export interface TakeInfo {
  path: string;
  name: string;
  seconds: number;
}

/** A pedal in the chain. Mirrors `PedalInfo` in src-tauri. */
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
  output: Level;
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
 * starts it with the stream open.
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

let mockNextPedal = 3;
const mockTransport = { position: 0, playing: false, recording: false };

const mock: EngineStatus = {
  running: mockParams?.has("running") ?? false,
  sampleRate: 48000,
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
  track: {
    armed: mockParams?.has("armed") ?? false,
    recording: mockParams?.has("recording") ?? false,
    playing: mockParams?.has("playing") ?? false,
    take: mockParams?.has("take")
      ? { path: "/takes/take-1761912000.wav", name: "take-1761912000", seconds: 12.4 }
      : null,
  },
  chain: [mockPedal(1, "gain"), mockPedal(2, "overdrive")],
  pedalKinds: ["gain", "overdrive"],
};
mockTransport.playing = mock.track.playing || mock.track.recording;
mockTransport.recording = mock.track.recording;

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
    track: { ...mock.track, playing: mockTransport.playing, recording: mockTransport.recording },
    chain: mock.chain.map((p) => ({ ...p, params: p.params.map((q) => ({ ...q })) })),
    sampleRate: mockSettings.sampleRate,
    bufferFrames: mockSettings.bufferFrames,
    inputUid: mockSettings.inputUid,
    outputUid: mockSettings.outputUid,
    inputName: mock.running ? (i?.name ?? null) : null,
    outputName: mock.running ? (o?.name ?? null) : null,
  };
}

const statusListeners = new Set<(s: EngineStatus) => void>();
function mockPublish() {
  const s = mockStatus();
  for (const l of statusListeners) l(s);
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

export async function trackArm(armed: boolean): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_arm", { armed });
  mock.track.armed = armed;
  return mockStatus();
}

export async function trackRecord(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_record");
  if (!mock.track.armed) throw new Error("arm the track before recording");
  mockTransport.position = 0;
  mockTransport.playing = true;
  mockTransport.recording = true;
  return mockStatus();
}

export async function trackPlay(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_play");
  mockTransport.position = 0;
  mockTransport.playing = true;
  return mockStatus();
}

export async function trackStop(): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("track_stop");
  if (mockTransport.recording) {
    mock.track.take = {
      path: "/takes/take-mock.wav",
      name: "take-mock",
      seconds: mockTransport.position,
    };
  }
  mockTransport.playing = false;
  mockTransport.recording = false;
  return mockStatus();
}

export async function chainAdd(spec: string, index?: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_add", { spec, index: index ?? null });
  mock.chain.splice(index ?? mock.chain.length, 0, mockPedal(mockNextPedal++, spec));
  return mockStatus();
}

export async function chainRemove(id: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_remove", { id });
  mock.chain = mock.chain.filter((p) => p.id !== id);
  return mockStatus();
}

export async function chainMove(id: number, to: number): Promise<EngineStatus> {
  if (inTauri) return invoke<EngineStatus>("chain_move", { id, to });
  const from = mock.chain.findIndex((p) => p.id === id);
  const [pedal] = mock.chain.splice(from, 1);
  if (pedal) mock.chain.splice(Math.min(to, mock.chain.length), 0, pedal);
  return mockStatus();
}

/** Sets a knob. Not echoed back as a status: the UI already shows the value. */
export async function chainSetParam(id: number, param: number, value: number): Promise<void> {
  if (inTauri) return invoke("chain_set_param", { id, param, value });
  const p = mock.chain.find((p) => p.id === id)?.params[param];
  if (p) p.value = value;
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
 * Meter frames at display rate. One channel is opened per page and shared
 * by every listener, since the shell keeps a channel until it fails.
 */
const meterListeners = new Set<(f: MeterFrame) => void>();
let meterChannelOpen = false;

function openMeterChannel() {
  if (meterChannelOpen) return;
  meterChannelOpen = true;
  if (inTauri) {
    const channel = new Channel<MeterFrame>();
    channel.onmessage = (frame) => {
      for (const l of meterListeners) l(frame);
    };
    invoke("subscribe_meters", { channel });
    return;
  }
  // The stand-in: a slow swell with a peak that leads the RMS.
  let t = 0;
  window.setInterval(() => {
    t += 1 / 60;
    if (mockTransport.playing) mockTransport.position += 1 / 60;
    const levels = mock.running
      ? {
          input: {
            rms: 0.18 + 0.12 * Math.sin(t * 2.1),
            peak: 0.4 + 0.3 * Math.abs(Math.sin(t * 3.7)),
          },
          output: {
            rms: 0.3 + 0.15 * Math.sin(t * 2.1 + 0.4),
            peak: 0.55 + 0.35 * Math.abs(Math.sin(t * 3.7 + 0.4)),
          },
        }
      : { input: SILENT, output: SILENT };
    const frame: MeterFrame = {
      ...levels,
      position: mockParams?.has("take") && !mockTransport.playing ? 0 : mockTransport.position,
      playing: mockTransport.playing,
      recording: mockTransport.recording,
    };
    for (const l of meterListeners) l(frame);
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
