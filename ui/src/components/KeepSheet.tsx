import { useId, useState } from "react";
import { type EngineStatus, type JamKept, jamKeepPlace } from "../lib/engine";
import { Button } from "./Button";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  kept: JamKept;
  /** The open project's name. */
  project: string;
  onClose: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/** `m:ss` for a length in seconds. */
export function minutes(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

/**
 * Where the take Keep that wrote goes: into the open project, a new one,
 * or left in `Jams/Takes`. In a project it is one undoable edit: the take
 * on a track with the jam's rig, the take through the rig muted beside
 * it, and the drums that played under it.
 */
export function KeepSheet({ kept, project, onClose, onStatus, onError }: Props) {
  const ids = useId();
  const [name, setName] = useState("Jam");
  const place = (into: Parameters<typeof jamKeepPlace>[0]) =>
    jamKeepPlace(into).then(
      (s) => {
        onStatus(s);
        onClose();
      },
      (e) => onError(String(e)),
    );
  return (
    <Sheet title="Keep that" onClose={() => place({ into: "takes" })}>
      <p className="m-0 text-sm">
        Kept the last {minutes(kept.seconds)} of your playing, dry and through the rig, in{" "}
        <span className="font-lcd">Jams/Takes</span>. Put it in a project with the drums that played
        under it?
      </p>
      <Button variant="transport" onClick={() => place({ into: "open" })}>
        Into {project}
      </Button>
      <form
        className="grid grid-cols-[1fr_auto] items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) place({ into: "new", name: name.trim() });
        }}
      >
        <TextInput
          id={`${ids}-name`}
          label="A new project"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Button type="submit" disabled={!name.trim()}>
          Create
        </Button>
      </form>
      <Button onClick={() => place({ into: "takes" })}>Just keep the files</Button>
    </Sheet>
  );
}
