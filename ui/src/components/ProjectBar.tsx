import { useRef, useState } from "react";
import { ACTIONS, type ActionId, type Bindings } from "../lib/actions";
import { type EngineStatus, type ProjectStatus, projectRedo, projectUndo } from "../lib/engine";
import { formatShortcut } from "../lib/shortcuts";
import { Button } from "./Button";
import { ContextMenu, type MenuItem } from "./ContextMenu";

/** The project menu: File's project commands, in sentence case, with separators before some. */
const PROJECT_MENU: { id: ActionId; label: string; separated?: boolean }[] = [
  { id: "newProject", label: "New project…" },
  { id: "openProject", label: "Open…" },
  { id: "save", label: "Save" },
  { id: "renameProject", label: "Rename…", separated: true },
  { id: "duplicateProject", label: "Duplicate…" },
  { id: "export", label: "Export mixdown…", separated: true },
  { id: "revealProject", label: "Show in Finder" },
];

type Props = {
  project: ProjectStatus;
  /** The app's actions, so the menu runs and enables them as File does. */
  actions: Bindings;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The project cluster in the toolbar under the title: the open project's
 * name, with a dot for unsaved edits, as a key that opens the project
 * menu (new, open, save, rename, duplicate, export, show in Finder, all
 * also in File with their shortcuts), then Undo and Redo.
 */
export function ProjectBar({ project, actions, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const key = useRef<HTMLButtonElement>(null);
  const items: MenuItem[] = PROJECT_MENU.map(({ id, label, separated }) => {
    const accelerator = ACTIONS[id].accelerator;
    return {
      label,
      ...(accelerator ? { hint: formatShortcut(accelerator) } : {}),
      ...(separated ? { separated } : {}),
      disabled: !actions[id].enabled,
      onSelect: () => actions[id].run(),
    };
  });
  return (
    <>
      <button
        ref={key}
        type="button"
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        aria-label={`${project.name}${project.dirty ? ", unsaved edits" : ""}: project menu`}
        title={`${project.name} · ${project.sampleRate / 1000} kHz: new, open, save, rename, export`}
        onClick={() => {
          const r = key.current?.getBoundingClientRect();
          setMenu(menu || !r ? null : { x: r.left, y: r.bottom + 2 });
        }}
        className="surface-lcd flex min-h-8 w-64 min-w-24 shrink cursor-pointer items-center gap-2 border-0 px-2 text-left font-lcd text-lcd-fg text-readout"
      >
        <span aria-live="polite" className="lcd-text min-w-0 flex-1 truncate">
          {project.name}
          {project.dirty ? <span aria-hidden="true"> •</span> : null}
        </span>
        <svg aria-hidden="true" viewBox="0 0 16 16" className="size-2.5 shrink-0 fill-current">
          <path d="M3 6l5 5 5-5z" />
        </svg>
      </button>
      {menu ? (
        <ContextMenu
          label="Project"
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={items}
        />
      ) : null}
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
    </>
  );
}
