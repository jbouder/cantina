import { type FormEvent, useEffect, useState } from "react";
import {
  type EngineStatus,
  projectDelete,
  projectList,
  projectNew,
  projectOpen,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  current: string;
  dirty: boolean;
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
  /** A project to open, from File ▸ Open Recent: opened at once, or asked about when there are unsaved edits. */
  opening?: string | null;
};

/**
 * The project picker: every project in the projects folder, and a field
 * to start a new one. Opening a project with unsaved edits in the current
 * one needs a second click, since the edits are lost. Delete moves a
 * project, any but the open one, to the Trash, also on a second click.
 */
export function Projects({ current, dirty, onClose, onStatus, opening }: Props) {
  const [names, setNames] = useState<string[] | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(() => (opening && dirty ? opening : null));
  const [deleting, setDeleting] = useState<string | null>(null);

  useEffect(() => {
    projectList().then(setNames, (e) => setError(String(e)));
  }, []);

  const finish = (action: Promise<EngineStatus>) =>
    action.then(
      (s) => {
        onStatus(s);
        onClose();
      },
      (e) => setError(String(e)),
    );

  const open = (project: string) => {
    if (project === current) {
      onClose();
      return;
    }
    if (dirty && confirm !== project) {
      setConfirm(project);
      return;
    }
    finish(projectOpen(project));
  };

  const remove = (project: string) => {
    if (deleting !== project) {
      setDeleting(project);
      return;
    }
    setDeleting(null);
    projectDelete(project).then(
      (s) => {
        onStatus(s);
        setNames((n) => n?.filter((x) => x !== project) ?? null);
      },
      (e) => setError(String(e)),
    );
  };

  const create = (e: FormEvent) => {
    e.preventDefault();
    finish(projectNew(name));
  };

  return (
    <Sheet
      title="Projects"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form="new-project-form" tier="primary" disabled={!name.trim()}>
            Create
          </Button>
        </>
      }
    >
      {error ? (
        <Lcd role="alert" error className="text-readout">
          {error}
        </Lcd>
      ) : null}
      <ul className="surface-lcd m-0 grid max-h-64 list-none gap-0.5 overflow-y-auto p-1">
        {names === null ? (
          <li className="px-2 py-1 font-lcd text-readout">...</li>
        ) : names.length === 0 ? (
          <li className="px-2 py-1 font-lcd text-readout">No projects yet</li>
        ) : (
          names.map((n) => (
            <li key={n} className="flex items-center gap-1">
              <button
                type="button"
                aria-current={n === current ? "true" : undefined}
                onClick={() => open(n)}
                className={`flex min-h-9 min-w-0 flex-1 items-center border-0 bg-transparent px-2 text-left font-lcd text-readout ${n === current ? "bg-selected text-selected-ink" : "text-lcd-fg"}`}
              >
                <span className="lcd-text flex-1 truncate">{n}</span>
                {confirm === n ? (
                  <span className="lcd-text text-readout text-lcd-error">
                    Click again to drop unsaved edits
                  </span>
                ) : null}
              </button>
              {n === current ? null : (
                <Button
                  tier="quiet"
                  title={
                    deleting === n
                      ? `Click again to move ${n} to the Trash`
                      : `Move ${n} to the Trash`
                  }
                  onClick={() => remove(n)}
                  onBlur={() => setDeleting((d) => (d === n ? null : d))}
                >
                  {deleting === n ? "Move to Trash" : "Delete"}
                </Button>
              )}
            </li>
          ))
        )}
      </ul>
      <form id="new-project-form" onSubmit={create} className="grid gap-2">
        <TextInput
          id="new-project-name"
          label="New project"
          hint="Saved in the Cantina folder in Music"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoComplete="off"
        />
      </form>
    </Sheet>
  );
}
