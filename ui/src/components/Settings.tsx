import type { SkinPreview } from "@cantina/skins";
import { useEffect, useRef, useState } from "react";
import {
  appVersion,
  type EngineStatus,
  latencyWarningSet,
  ms,
  openLastAtLaunchSet,
  routeLabel,
  type UpdateInfo,
  updateCheck,
} from "../lib/engine";
import { useReduceFlashing } from "../lib/motion";
import { SKINS, useSkin } from "../skins";
import { AssistantSettings } from "./AssistantSettings";
import { Button } from "./Button";
import { DevicePicker } from "./DevicePicker";
import { Toggle } from "./Field";
import { InstrumentSettings } from "./InstrumentSettings";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  status: EngineStatus;
  /** True while a start or stop is in flight. */
  busy: boolean;
  onToggleEngine: () => void;
  onClose: () => void;
  onStatusChange: (status: EngineStatus) => void;
  /** A newer release was found: the window offers to install it. */
  onUpdate: (update: UpdateInfo) => void;
  /** Check for a release as the sheet opens, for Cantina ▸ Check for Updates. */
  checkForUpdates?: boolean;
  /** A section to scroll to as the sheet opens. */
  section?: "assistant" | "instrument" | null;
};

const WARN_CHOICES = [5, 10, 20, 50, 100];

/**
 * A skin in miniature, in its own colors whatever skin is on: its frame
 * with the title stripe, a display with its ink, and a key.
 */
function SkinSwatch({ preview }: { preview: SkinPreview }) {
  return (
    <span
      aria-hidden="true"
      className="relative inline-grid h-4 w-7 shrink-0 grid-cols-[1fr_6px] gap-px overflow-hidden rounded-[2px] p-0.5 pt-1"
      style={{ background: preview.frame }}
    >
      <span className="absolute inset-x-0 top-0 h-[2px]" style={{ background: preview.title }} />
      <span className="grid place-items-center" style={{ background: preview.lcd }}>
        <span className="h-px w-3/4" style={{ background: preview.lcdInk }} />
      </span>
      <span style={{ background: preview.key }} />
    </span>
  );
}

/** The running version and a button that asks the release feed for a newer one. */
function Updates({
  onUpdate,
  checkNowOnOpen,
}: {
  onUpdate: (update: UpdateInfo) => void;
  checkNowOnOpen: boolean;
}) {
  const [version, setVersion] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    appVersion().then(setVersion, () => setVersion(null));
  }, []);
  const checkNow = () => {
    setChecking(true);
    setResult(null);
    updateCheck()
      .then(
        (found) => {
          if (found) {
            setResult(`Cantina ${found.version} is out.`);
            onUpdate(found);
          } else {
            setResult("This is the newest version.");
          }
        },
        (e) => setResult(`Could not check: ${e}`),
      )
      .finally(() => setChecking(false));
  };
  // Cantina ▸ Check for Updates opens the sheet and checks at once.
  const checkOnOpen = useRef(checkNowOnOpen);
  useEffect(() => {
    if (checkOnOpen.current) {
      checkOnOpen.current = false;
      checkNow();
    }
  });
  return (
    <section aria-labelledby="updates-label" className="grid gap-2">
      <h3 id="updates-label" className="caption m-0">
        Updates
        <small className="hint block text-ink-dim">
          Cantina looks for a new release each time it opens.
        </small>
      </h3>
      <div className="flex items-center gap-2">
        <Lcd aria-live="polite" className="min-w-0 flex-1 text-readout">
          {result ?? (version ? `Version ${version}` : "Version unknown")}
        </Lcd>
        <Button disabled={checking} onClick={checkNow} className="shrink-0">
          {checking ? "Checking…" : "Check for updates"}
        </Button>
      </div>
    </section>
  );
}

/**
 * The settings sheet: the engine, audio devices, the latency warning, the
 * skin picker, Reduce flashing, what launch opens, the assistant and
 * updates.
 */
export function Settings({
  checkForUpdates,
  status,
  busy,
  onToggleEngine,
  onClose,
  onStatusChange,
  onUpdate,
  section,
}: Props) {
  const [skin, setSkin] = useSkin();
  const [reduceFlashing, setReduceFlashing] = useReduceFlashing();
  const { latency } = status;
  const choices = WARN_CHOICES.includes(latency.warnMs)
    ? WARN_CHOICES
    : [...WARN_CHOICES, latency.warnMs].sort((a, b) => a - b);
  return (
    <Sheet title="Settings" onClose={onClose}>
      <section aria-labelledby="audio-heading" className="grid gap-2">
        <h3 id="audio-heading" className="caption m-0">
          Audio
        </h3>
        <div className="flex items-center gap-2">
          <Lcd aria-live="polite" className="min-w-0 flex-1 text-readout">
            {routeLabel(status)}
          </Lcd>
          <Button
            aria-pressed={status.running}
            disabled={busy}
            onClick={onToggleEngine}
            className="shrink-0"
          >
            {busy ? "…" : status.running ? "Turn audio off" : "Turn audio on"}
          </Button>
        </div>
        <DevicePicker onStatusChange={onStatusChange} />
        <Select
          id="latency-warn"
          label="Warn when plugins delay playback by more than"
          hint={
            status.running
              ? `Now ${ms(latency.compensationFrames, status.sampleRate)}; the devices add ${ms(latency.inputFrames, status.sampleRate)} in and ${ms(latency.outputFrames, status.sampleRate)} out.`
              : "Live monitoring is never delayed to match other tracks."
          }
          value={latency.warnMs}
          onChange={(e) => latencyWarningSet(Number(e.target.value)).then(onStatusChange)}
        >
          {choices.map((c) => (
            <option key={c} value={c}>
              {c} ms
            </option>
          ))}
        </Select>
      </section>
      <InstrumentSettings
        status={status}
        onStatusChange={onStatusChange}
        reveal={section === "instrument"}
      />
      <section aria-labelledby="skin-label" className="grid gap-2">
        <h3 id="skin-label" className="caption m-0">
          Skin
          <small className="hint block text-ink-dim">
            Changes the whole studio's look. Remembered for next time.
          </small>
        </h3>
        <div role="radiogroup" aria-labelledby="skin-label" className="flex flex-wrap gap-1">
          {SKINS.map((s) => (
            <Button
              key={s.id}
              role="radio"
              aria-checked={skin === s.id}
              onClick={() => setSkin(s.id)}
            >
              <SkinSwatch preview={s.preview} />
              {s.name}
            </Button>
          ))}
        </div>
      </section>
      <section aria-labelledby="motion-label" className="grid gap-2">
        <h3 id="motion-label" className="caption m-0">
          Motion
        </h3>
        <Toggle
          id="reduce-flashing"
          label="Reduce flashing"
          hint="Stills the stage's flashes and the skin's beat pulse. Your system's reduce motion setting stills the pulse too."
          checked={reduceFlashing}
          onChange={setReduceFlashing}
        />
      </section>
      <section aria-labelledby="launch-label" className="grid gap-2">
        <h3 id="launch-label" className="caption m-0">
          Launch
        </h3>
        <Toggle
          id="open-last"
          label="Open the last project at launch"
          hint="Off, Cantina opens with the start dialog: a new project, a recent one, or a jam."
          checked={status.openLastAtLaunch}
          onChange={(on) => openLastAtLaunchSet(on).then(onStatusChange)}
        />
      </section>
      <AssistantSettings
        assistant={status.assistant}
        onStatusChange={onStatusChange}
        reveal={section === "assistant"}
      />
      <Updates onUpdate={onUpdate} checkNowOnOpen={checkForUpdates ?? false} />
    </Sheet>
  );
}
