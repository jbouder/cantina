import { EngineFrames, getScene, Stage } from "@cantina/scenes";
import { useEffect, useRef } from "react";
import { onAnalysis } from "../lib/engine";
import { reducesFlashing, useReduceFlashing } from "../lib/motion";

type Props = {
  scene: string;
  className?: string;
  onError: (message: string | null) => void;
};

/**
 * A visualizer scene drawn from the engine's analysis frames. Changing
 * `scene` crossfades to the new one; the old keeps drawing until it has
 * faded out.
 */
export function SceneCanvas({ scene, className = "", onError }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<Stage | null>(null);
  const [reduceFlashing] = useReduceFlashing();
  const errors = useRef(onError);
  errors.current = onError;

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
        reduceFlashing: reducesFlashing(),
        retroLook: false,
        palette: "auto",
        options: {},
      },
      { onError: (message) => errors.current(message) },
    );
    stage.current = s;
    s.start();
    return () => {
      stopAnalysis();
      s.destroy();
      stage.current = null;
    };
  }, []);

  // Scenes read the context every frame, so a change applies at once.
  useEffect(() => {
    if (stage.current) stage.current.rc.reduceFlashing = reduceFlashing;
  }, [reduceFlashing]);

  useEffect(() => {
    errors.current(null);
    stage.current?.setScene(scene).catch((e) => errors.current(String(e)));
  }, [scene]);

  return (
    <div
      ref={host}
      role="img"
      aria-label={`${getScene(scene)?.name ?? scene} scene`}
      className={`relative overflow-hidden [&_.viz-layer]:absolute [&_.viz-layer]:inset-0 [&_.viz-layer]:size-full [&_.viz-layer]:transition-opacity [&_.viz-layer]:duration-[400ms] ${className}`}
    />
  );
}
