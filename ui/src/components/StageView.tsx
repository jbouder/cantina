import { useId, useState } from "react";
import { analysisSetSource, type EngineStatus } from "../lib/engine";
import { LISTED_SCENES, setScene, useScene } from "../lib/stage";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { SceneCanvas } from "./SceneCanvas";
import { Select } from "./Select";
import { Sheet } from "./Sheet";

type Props = {
  status: EngineStatus;
  onClose: () => void;
  onPerform: () => void;
  onStatus: (s: EngineStatus) => void;
};

/**
 * The stage: a visualizer scene drawn from the engine's analysis frames,
 * with the scene and the signal it listens to picked underneath, and the
 * way into performance mode.
 */
export function StageView({ status, onClose, onPerform, onStatus }: Props) {
  const ids = useId();
  const scene = useScene();
  const [problem, setProblem] = useState<string | null>(null);
  const source = status.analysisSource;

  return (
    <Sheet title="Stage" onClose={onClose} wide>
      <SceneCanvas
        scene={scene}
        onError={setProblem}
        className="aspect-video w-full rounded-control bg-lcd-bg"
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
        {LISTED_SCENES.map((s) => (
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
      <div className="flex items-center justify-between gap-3">
        <small className="text-ink-dim text-label">P plays full screen from anywhere.</small>
        <Button tier="primary" onClick={onPerform}>
          Perform
        </Button>
      </div>
    </Sheet>
  );
}
