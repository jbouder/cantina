import { getScene } from "@cantina/scenes";
import type { Window as ShellWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { chainSelect, type EngineStatus, inTauri, pedalSetBypass } from "../lib/engine";
import {
  closeStageWindow,
  LISTED_SCENES,
  openStageWindow,
  stageWindowOpen,
  stepScene,
  useScene,
} from "../lib/stage";
import { Button } from "./Button";
import { Led } from "./Led";
import { SceneCanvas } from "./SceneCanvas";

type Props = {
  status: EngineStatus;
  /** Opens with the pedalboard showing, from `?board` in a browser. */
  initialBoard?: boolean;
  onExit: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The stomps a number key reaches. */
const STOMPS = 9;

/**
 * Performance mode: the stage full screen, the engine untouched under
 * it. A translucent pedalboard comes and goes over the scene for
 * switching chains and stomping pedals; scenes change by keyboard with a
 * crossfade; the stage can go to a second display.
 *
 * Keys: Left and Right change scene, B shows the board, Up and Down
 * switch chains, 1 to 9 stomp the live chain's pedals, D sends the stage
 * to the other display, Escape leaves.
 */
export function Performance({ status, initialBoard = false, onExit, onStatus, onError }: Props) {
  const scene = useScene();
  const [board, setBoard] = useState(initialBoard);
  const [problem, setProblem] = useState<string | null>(null);
  const [external, setExternal] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  const chains = status.project.tracks.filter((t) => !t.kit);
  // The chain the input plays through: the armed audio track.
  const live = chains.find((t) => t.armed) ?? null;

  const report = (action: Promise<EngineStatus>) =>
    action.then(onStatus, (e) => onError(String(e)));

  const toggleExternal = async () => {
    try {
      if (await stageWindowOpen()) {
        await closeStageWindow();
        setExternal(false);
      } else if (await openStageWindow()) {
        setExternal(true);
      } else {
        flash("Connect a second display to show the stage on it.");
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const hintTimer = useRef(0);
  const flash = (text: string) => {
    setHint(text);
    window.clearTimeout(hintTimer.current);
    hintTimer.current = window.setTimeout(() => setHint(null), 1800);
  };

  // The window goes full screen for as long as performance mode lasts.
  useEffect(() => {
    if (!inTauri) return;
    let shell: ShellWindow | null = null;
    import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
      shell = getCurrentWindow();
      shell.setFullscreen(true).catch(() => {});
    });
    return () => {
      shell?.setFullscreen(false).catch(() => {});
    };
  }, []);

  useEffect(() => {
    stageWindowOpen().then(setExternal, () => {});
  }, []);

  // The latest state for the key handler, which is installed once.
  const keys = useRef({ chains, live, onExit, report, toggleExternal, flash });
  keys.current = { chains, live, onExit, report, toggleExternal, flash };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const k = keys.current;
      const stomp = Number(e.key);
      if (e.key === "Escape") {
        k.onExit();
      } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        stepScene(e.key === "ArrowRight" ? 1 : -1);
      } else if (e.key === "b" || e.key === "B") {
        setBoard((b) => !b);
      } else if (e.key === "d" || e.key === "D") {
        k.toggleExternal();
      } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        if (k.chains.length === 0) return;
        const at = k.live ? k.chains.indexOf(k.live) : -1;
        const step = e.key === "ArrowDown" ? 1 : -1;
        const next = k.chains[(at + step + k.chains.length) % k.chains.length];
        if (next) {
          k.report(chainSelect(next.id));
          k.flash(`Playing through ${next.name}`);
        }
      } else if (stomp >= 1 && stomp <= STOMPS) {
        const pedal = k.live?.chain[stomp - 1];
        if (!pedal) return;
        k.report(pedalSetBypass(pedal.id, !pedal.bypassed));
        k.flash(`${pedal.name} ${pedal.bypassed ? "on" : "off"}`);
      } else {
        return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const sceneName = getScene(scene)?.name ?? scene;
  const sceneAt = LISTED_SCENES.findIndex((s) => s.id === scene);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Performance"
      className="fixed inset-0 z-30 flex flex-col bg-lcd-bg text-sheet-ink"
    >
      {external ? (
        <div className="flex flex-1 items-center justify-center text-ink-dim">
          The stage is on the other display.
        </div>
      ) : (
        <SceneCanvas scene={scene} onError={setProblem} className="flex-1" />
      )}

      <div className="pointer-events-none absolute inset-x-0 top-0 flex items-start justify-between gap-3 p-4">
        <div className="pointer-events-auto flex items-center gap-1 rounded-control bg-scrim p-1">
          <Button aria-label="Previous scene" onClick={() => stepScene(-1)} className="px-2">
            ‹
          </Button>
          <span
            aria-live="polite"
            className="min-w-40 px-2 text-center font-lcd text-base text-lcd-fg"
          >
            {sceneName}
            <span className="ml-2 text-ink-dim text-xs">
              {sceneAt + 1}/{LISTED_SCENES.length}
            </span>
          </span>
          <Button aria-label="Next scene" onClick={() => stepScene(1)} className="px-2">
            ›
          </Button>
        </div>
        <div className="pointer-events-auto flex gap-1 rounded-control bg-scrim p-1">
          <Button aria-pressed={board} onClick={() => setBoard((b) => !b)}>
            Pedalboard
          </Button>
          {inTauri ? (
            <Button aria-pressed={external} onClick={toggleExternal}>
              Second display
            </Button>
          ) : null}
          <Button onClick={onExit}>Leave</Button>
        </div>
      </div>

      {hint || problem ? (
        <div
          role="status"
          className="pointer-events-none absolute inset-x-0 top-20 mx-auto w-fit rounded-control bg-scrim px-4 py-2 font-lcd text-lg text-lcd-fg"
        >
          {problem ?? hint}
        </div>
      ) : null}

      {board ? (
        <section
          aria-label="Pedalboard"
          className="surface-panel absolute inset-x-4 bottom-4 grid gap-3 p-3 opacity-90"
        >
          <div className="flex flex-wrap items-center gap-1">
            <span className="mr-2 font-bold text-[11px] uppercase tracking-[0.1em]">Chain</span>
            {chains.length === 0 ? (
              <span className="text-ink-dim text-sm">Add a guitar track to play through.</span>
            ) : (
              chains.map((t) => (
                <Button
                  key={t.id}
                  variant={t.armed ? "transport" : "panel"}
                  aria-pressed={t.armed}
                  onClick={() => report(chainSelect(t.id))}
                >
                  {t.name}
                </Button>
              ))
            )}
            <span className="ml-auto text-ink-dim text-xs">
              ↑ ↓ chains · 1–9 stomp · ← → scenes
            </span>
          </div>
          {live ? (
            <ul
              aria-label={`${live.name} pedals`}
              className="m-0 flex list-none flex-wrap gap-2 p-0"
            >
              {live.chain.map((pedal, i) => (
                <li key={pedal.id}>
                  <button
                    type="button"
                    aria-pressed={!pedal.bypassed}
                    aria-label={`${pedal.name}${i < STOMPS ? `, key ${i + 1}` : ""}`}
                    onClick={() => report(pedalSetBypass(pedal.id, !pedal.bypassed))}
                    className="surface-pbutton grid min-h-20 min-w-28 place-items-center gap-2 border-0 px-3 py-2"
                  >
                    <Led on={!pedal.bypassed} label={pedal.name} />
                    <span className="font-bold text-[11px] uppercase tracking-[0.06em]">
                      {pedal.name}
                    </span>
                    {i < STOMPS ? <span className="text-ink-dim text-xs">{i + 1}</span> : null}
                  </button>
                </li>
              ))}
              {live.chain.length === 0 ? (
                <li className="text-ink-dim text-sm">{live.name} has no pedals.</li>
              ) : null}
            </ul>
          ) : chains.length > 0 ? (
            <p className="m-0 text-ink-dim text-sm">Pick a chain to play through.</p>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}
