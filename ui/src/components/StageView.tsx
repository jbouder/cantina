import { DEFAULT_SCENE, EngineFrames, getScene, SCENES, Stage } from "@cantina/scenes";
import { useEffect, useId, useRef, useState } from "react";
import { analysisSetSource, type EngineStatus, onAnalysis } from "../lib/engine";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  status: EngineStatus;
  /** The scene to open on, from `?scene=` in a browser. */
  initialScene?: string | undefined;
  onClose: () => void;
  onStatus: (s: EngineStatus) => void;
};

/** The scenes a person can pick: every one but those reachable by id only. */
const LISTED = SCENES.filter((s) => !s.hidden);

/**
 * The stage: a visualizer scene drawn from the engine's analysis frames,
 * with the scene and the signal it listens to picked underneath.
 */
export function StageView({ status, initialScene, onClose, onStatus }: Props) {
  const ids = useId();
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<Stage | null>(null);
  const [scene, setScene] = useState(() =>
    initialScene && getScene(initialScene) ? initialScene : DEFAULT_SCENE.gpu,
  );
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const frames = new EngineFrames();
    const stopAnalysis = onAnalysis((a) => frames.push(a));
    const s = new Stage(
      el,
      frames,
      {
        sampleRate: 48_000,
        quality: 1,
        reduceFlashing: false,
        retroLook: false,
        palette: "auto",
        options: {},
      },
      { onError: setProblem },
    );
    stage.current = s;
    s.start();
    return () => {
      stopAnalysis();
      s.destroy();
      stage.current = null;
    };
  }, []);

  useEffect(() => {
    setProblem(null);
    stage.current?.setScene(scene).catch((e) => setProblem(String(e)));
  }, [scene]);

  const source = status.analysisSource;

  return (
    <Sheet title="Stage" onClose={onClose} wide>
      <div
        ref={host}
        role="img"
        aria-label={`${getScene(scene)?.name ?? scene} scene`}
        className="relative aspect-video w-full overflow-hidden rounded-control bg-lcd-bg [&_.viz-layer]:absolute [&_.viz-layer]:inset-0 [&_.viz-layer]:size-full [&_.viz-layer]:transition-opacity [&_.viz-layer]:duration-[400ms]"
      />
      {problem ? (
        <Lcd role="alert" error>
          {problem}
        </Lcd>
      ) : null}
      <Select
        id={`${ids}-scene`}
        label="Scene"
        value={scene}
        onChange={(e) => setScene(e.target.value)}
      >
        {LISTED.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
          </option>
        ))}
      </Select>
      <Select
        id={`${ids}-source`}
        label="Listens to"
        hint="The signal the scene draws from"
        value={source === null ? "master" : String(source)}
        onChange={(e) => {
          const value = e.target.value;
          analysisSetSource(value === "master" ? null : Number(value)).then(onStatus, (err) =>
            setProblem(String(err)),
          );
        }}
      >
        <option value="master">Master</option>
        {status.project.tracks.map((t) => (
          <option key={t.id} value={t.id}>
            {t.name}
          </option>
        ))}
      </Select>
    </Sheet>
  );
}
