import {
  type EngineStatus,
  type ProjectStatus,
  projectRedo,
  projectSave,
  projectUndo,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";

type Props = {
  project: ProjectStatus;
  onOpenProjects: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The open project's name, whether it has unsaved edits, Undo, Redo, Save and the picker. */
export function ProjectBar({ project, onOpenProjects, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
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
        aria-label={project.undoLabel ? `Undo ${project.undoLabel}` : "Undo"}
        title={project.undoLabel ? `Undo ${project.undoLabel} (⌘Z)` : "Nothing to undo"}
        disabled={!project.undoLabel}
        onClick={() => run(projectUndo())}
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
          <path d="M6 3L1 7.5 6 12v-3h4a3 3 0 0 1 0 6H7v2h3a5 5 0 0 0 0-10H6z" />
        </svg>
      </Button>
      <Button
        aria-label={project.redoLabel ? `Redo ${project.redoLabel}` : "Redo"}
        title={project.redoLabel ? `Redo ${project.redoLabel} (⇧⌘Z)` : "Nothing to redo"}
        disabled={!project.redoLabel}
        onClick={() => run(projectRedo())}
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
          <path d="M10 3l5 4.5L10 12V9H6a3 3 0 0 0 0 6h3v2H6A5 5 0 0 1 6 7h4z" />
        </svg>
      </Button>
      <Button variant="transport" disabled={!project.dirty} onClick={() => run(projectSave())}>
        Save
      </Button>
      <Button aria-haspopup="dialog" onClick={onOpenProjects}>
        Projects…
      </Button>
    </div>
  );
}
