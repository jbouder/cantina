import { useEffect, useState } from "react";
import { followScene, useScene } from "../lib/stage";
import { SceneCanvas } from "./SceneCanvas";

/**
 * The whole page in the stage window on a second display: the scene and
 * nothing else, following the scene the studio picks.
 */
export function StageWindow() {
  const scene = useScene();
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    followScene().then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  return (
    <main className="fixed inset-0 bg-lcd-bg">
      <SceneCanvas scene={scene} onError={setProblem} className="size-full" />
      {problem ? (
        <p role="alert" className="absolute inset-x-0 bottom-4 text-center text-lcd-error">
          {problem}
        </p>
      ) : null}
    </main>
  );
}
