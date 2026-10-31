import { type EngineStatus, type ProjectStatus, projectSave } from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";

type Props = {
  project: ProjectStatus;
  onOpenProjects: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The open project's name, whether it has unsaved edits, Save and the picker. */
export function ProjectBar({ project, onOpenProjects, onStatus, onError }: Props) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Lcd aria-live="polite" className="min-w-0 flex-1 truncate">
        {project.name}
        {project.dirty ? (
          <span role="img" aria-label="unsaved edits" title="Unsaved edits">
            {" "}
            •
          </span>
        ) : null}
        <span> · {project.sampleRate / 1000} kHz</span>
      </Lcd>
      <Button
        variant="transport"
        disabled={!project.dirty}
        onClick={() => projectSave().then(onStatus, (e) => onError(String(e)))}
      >
        Save
      </Button>
      <Button aria-haspopup="dialog" onClick={onOpenProjects}>
        Projects…
      </Button>
    </div>
  );
}
