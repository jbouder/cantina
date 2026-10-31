import { type FormEvent, useEffect, useState } from "react";
import { type EngineStatus, projectList, projectNew, projectOpen } from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  current: string;
  dirty: boolean;
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
};

/**
 * The project picker: every project in the projects folder, and a field
 * to start a new one. Opening a project with unsaved edits in the current
 * one needs a second click, since the edits are lost.
 */
export function Projects({ current, dirty, onClose, onStatus }: Props) {
  const [names, setNames] = useState<string[] | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);

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

  const create = (e: FormEvent) => {
    e.preventDefault();
    finish(projectNew(name));
  };

  return (
    <Sheet title="Projects" onClose={onClose}>
      {error ? (
        <Lcd role="alert" error className="text-base">
          {error}
        </Lcd>
      ) : null}
      <ul className="surface-lcd m-0 grid max-h-64 list-none gap-0.5 overflow-y-auto p-1">
        {names === null ? (
          <li className="px-2 py-1 font-lcd text-lg">...</li>
        ) : names.length === 0 ? (
          <li className="px-2 py-1 font-lcd text-lg">No projects yet</li>
        ) : (
          names.map((n) => (
            <li key={n}>
              <button
                type="button"
                aria-current={n === current ? "true" : undefined}
                onClick={() => open(n)}
                className={`flex min-h-11 w-full items-center border-0 bg-transparent px-2 text-left font-lcd text-lg text-lcd-fg ${n === current ? "bg-select" : ""}`}
              >
                <span className="lcd-text flex-1 truncate">{n}</span>
                {confirm === n ? (
                  <span className="lcd-text text-base text-lcd-error">
                    Click again to drop unsaved edits
                  </span>
                ) : null}
              </button>
            </li>
          ))
        )}
      </ul>
      <form onSubmit={create} className="grid gap-2">
        <TextInput
          id="new-project-name"
          label="New project"
          hint="Saved in the Cantina folder in Music"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoComplete="off"
        />
        <div className="flex justify-end">
          <Button type="submit" variant="transport" disabled={!name.trim()}>
            Create
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
