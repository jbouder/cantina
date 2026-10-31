import {
  type EngineStatus,
  projectDiscardRecovery,
  projectRecover,
  type RecoveryInfo,
} from "../lib/engine";
import { Button } from "./Button";

type Props = {
  recovery: RecoveryInfo;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The journal found edits a previous session never saved: replay or drop them. */
export function Recovery({ recovery, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const n = recovery.edits;
  return (
    <div
      role="alertdialog"
      aria-labelledby="recovery-text"
      className="surface-lcd flex flex-wrap items-center gap-3 px-3 py-2 font-lcd text-lg leading-tight"
    >
      <span id="recovery-text" className="lcd-text flex-1">
        {n} unsaved edit{n === 1 ? "" : "s"} from the last session, up to "{recovery.last}". Recover
        them?
      </span>
      <Button variant="transport" onClick={() => run(projectRecover())}>
        Recover
      </Button>
      <Button onClick={() => run(projectDiscardRecovery())}>Discard</Button>
    </div>
  );
}
