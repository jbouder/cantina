import { useId, useState } from "react";
import { type EngineStatus, type Jam, jamSave } from "../lib/engine";
import { Button } from "./Button";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  setup: Jam;
  /** The names of the user's jams, to warn before replacing one. */
  mine: string[];
  onClose: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * Saves the jam as it is now, groove, backing, tempo, key and rig, to
 * `~/Music/Cantina/Jams/<name>.jam.json`, where it joins the Jams list.
 */
export function SaveJamSheet({ setup, mine, onClose, onStatus, onError }: Props) {
  const ids = useId();
  const [name, setName] = useState(setup.name === "Jam" ? "" : setup.name);
  const [genre, setGenre] = useState(setup.genre ?? "");
  const [description, setDescription] = useState(setup.description ?? "");
  const clash = mine.some((m) => m.toLowerCase() === name.trim().toLowerCase());
  const save = () =>
    jamSave(name, genre, description, clash).then(
      (s) => {
        onStatus(s);
        onClose();
      },
      (e) => onError(String(e)),
    );
  return (
    <Sheet
      title="Save jam"
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
          value={name}
          placeholder="Blues in A"
          onChange={(e) => setName(e.target.value)}
        />
        <TextInput
          id={`${ids}-genre`}
          label="Genre"
          value={genre}
          placeholder="Blues"
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
            ? `Replaces your jam called ${name.trim()}.`
            : "Saved in Jams in your Cantina folder; stock jams stay as they are."}
        </p>
      </form>
    </Sheet>
  );
}
