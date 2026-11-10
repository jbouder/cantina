import { type FormEvent, useEffect, useState } from "react";
import { type EngineStatus, projectDuplicate, projectRename } from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

/** What the sheet does with the name. */
export type ProjectNameMode = "rename" | "duplicate";

type Props = {
  mode: ProjectNameMode;
  current: string;
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
};

const COPY = {
  rename: {
    title: "Rename project",
    hint: "Saves the project and renames its folder",
    action: "Rename",
    start: (name: string) => name,
  },
  duplicate: {
    title: "Duplicate project",
    hint: "A copy of the project as it is now; the original stays as last saved",
    action: "Duplicate",
    start: (name: string) => `${name} copy`,
  },
};

/** Asks for a project's new name, for File ▸ Rename and File ▸ Duplicate. */
export function ProjectName({ mode, current, onClose, onStatus }: Props) {
  const copy = COPY[mode];
  const [name, setName] = useState(() => copy.start(current));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const unchanged = name.trim() === current || !name.trim();
  // The sheet focuses its close key first; the name is what you came for.
  useEffect(() => {
    document.getElementById("project-name")?.focus();
  }, []);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (unchanged) return;
    setBusy(true);
    setError(null);
    (mode === "rename" ? projectRename(name) : projectDuplicate(name))
      .then(
        (s) => {
          onStatus(s);
          onClose();
        },
        (err) => setError(String(err)),
      )
      .finally(() => setBusy(false));
  };

  return (
    <Sheet
      title={copy.title}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            type="submit"
            form="project-name-form"
            tier="primary"
            disabled={unchanged || busy}
          >
            {copy.action}
          </Button>
        </>
      }
    >
      {error ? (
        <Lcd role="alert" error className="text-readout">
          {error}
        </Lcd>
      ) : null}
      <form id="project-name-form" onSubmit={submit} className="grid gap-2">
        <TextInput
          id="project-name"
          label="Name"
          hint={copy.hint}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onFocus={(e) => e.currentTarget.select()}
          autoComplete="off"
        />
      </form>
    </Sheet>
  );
}
