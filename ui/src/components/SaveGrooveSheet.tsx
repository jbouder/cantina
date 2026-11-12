import { useId, useState } from "react";
import { drumDesignSaveGroove, type EngineStatus } from "../lib/engine";
import { Button } from "./Button";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  /** The name to start from: the groove last opened or saved, if any. */
  name: string;
  /** The names of the user's grooves, to warn before replacing one. */
  mine: string[];
  onClose: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * Saves the drum designer's whole part (sections, order, kit, and the
 * project's tempo and time signature) to
 * `~/Music/Cantina/Grooves/<name>.groove.json`, where the palette and the
 * jam's groove picker find it.
 */
export function SaveGrooveSheet({ name: start, mine, onClose, onStatus, onError }: Props) {
  const ids = useId();
  const [name, setName] = useState(start);
  const [genre, setGenre] = useState("");
  const [description, setDescription] = useState("");
  const clash = mine.some((m) => m.toLowerCase() === name.trim().toLowerCase());
  const save = () =>
    drumDesignSaveGroove(name, genre, description, clash).then(
      (s) => {
        onStatus(s);
        onClose();
      },
      (e) => onError(String(e)),
    );
  return (
    <Sheet
      title="Save as groove"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form={`${ids}-form`} tier="primary" disabled={!name.trim()}>
            {clash ? "Replace" : "Save"}
          </Button>
        </>
      }
    >
      <form
        id={`${ids}-form`}
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) save();
        }}
      >
        <TextInput
          id={`${ids}-name`}
          label="Name"
          hint="Jams pick it by this name"
          value={name}
          placeholder="Late train"
          onChange={(e) => setName(e.target.value)}
        />
        <TextInput
          id={`${ids}-genre`}
          label="Genre"
          value={genre}
          placeholder="Rock"
          onChange={(e) => setGenre(e.target.value)}
        />
        <TextInput
          id={`${ids}-description`}
          label="Description"
          value={description}
          placeholder="A line on what it is for"
          onChange={(e) => setDescription(e.target.value)}
        />
        <p className="m-0 text-ink-dim text-label">
          {clash
            ? `Replaces your groove called ${name.trim()}.`
            : "The whole part: its sections, order and kit, at this project's tempo and time signature. Saved in Grooves in your Cantina folder."}
        </p>
      </form>
    </Sheet>
  );
}
