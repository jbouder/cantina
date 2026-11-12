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
  /** Whether a new project can start now: not while recording. */
  canStartNew: boolean;
  /** File ▸ New Project: the start dialog, with every option. */
  onNewProject: () => void;
  onOpenProjects: () => void;
  onOpenExport: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The project cluster in the toolbar under the title: the open project's name, whether
 * it has unsaved edits, Undo, Redo, Save, New, the picker and Export.
 */
export function ProjectBar({
  project,
  canStartNew,
  onNewProject,
  onOpenProjects,
  onOpenExport,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  return (
    <>
      <Lcd
        aria-live="polite"
        title={`${project.name} · ${project.sampleRate / 1000} kHz`}
        className="w-64 min-w-24 shrink truncate text-readout"
      >
        {project.name}
        {project.dirty ? (
          <span role="img" aria-label="unsaved edits">
            {" "}
            •
          </span>
        ) : null}
      </Lcd>
      <Button
        aria-label={project.undoLabel ? `Undo ${project.undoLabel}` : "Undo"}
        title={project.undoLabel ? `Undo ${project.undoLabel} (⌘Z)` : "Nothing to undo"}
        disabled={!project.undoLabel}
        onClick={() => run(projectUndo())}
        square
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
        square
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
          <path d="M10 3l5 4.5L10 12V9H6a3 3 0 0 0 0 6h3v2H6A5 5 0 0 1 6 7h4z" />
        </svg>
      </Button>
      <Button title="Save (⌘S)" disabled={!project.dirty} onClick={() => run(projectSave())}>
        Save
      </Button>
      <Button
        aria-haspopup="dialog"
        title="New project (⌘N)"
        disabled={!canStartNew}
        onClick={onNewProject}
      >
        New…
      </Button>
      <Button aria-haspopup="dialog" onClick={onOpenProjects}>
        Projects…
      </Button>
      <Button aria-haspopup="dialog" title="Export a mixdown (⌘E)" onClick={onOpenExport}>
        Export…
      </Button>
    </>
  );
}
