import {
  type EngineStatus,
  projectDiscardRecovery,
  projectRecover,
  type RecoveryInfo,
} from "../lib/engine";
import { Button } from "./Button";
import { Toast } from "./Toast";

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
    <Toast
      role="alertdialog"
      actions={
        <>
          <Button onClick={() => run(projectDiscardRecovery())}>Discard</Button>
          <Button tier="primary" onClick={() => run(projectRecover())}>
            Recover
          </Button>
        </>
      }
    >
      {n} unsaved edit{n === 1 ? "" : "s"} from the last session, up to "{recovery.last}". Recover
      them?
    </Toast>
  );
}
