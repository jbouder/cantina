import { useState } from "react";
import { type UpdateInfo, updateInstall } from "../lib/engine";
import { Button } from "./Button";
import { Toast } from "./Toast";

type Props = {
  update: UpdateInfo;
  /** Unsaved edits: installing restarts the app, so they are saved first. */
  dirty: boolean;
  /** A take is being recorded; a restart now would cut it off. */
  recording: boolean;
  onDismiss: () => void;
};

/**
 * A toast that says a newer Cantina is out, with Install and restart and
 * Later.
 * Installing waits for the project to be saved and for recording to
 * stop, since the app restarts into the new version.
 */
export function UpdateBanner({ update, dirty, recording, onDismiss }: Props) {
  const [progress, setProgress] = useState<number | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const installing = progress !== undefined;
  const blocked = recording
    ? "Stop recording first"
    : dirty
      ? "Save the project first: installing restarts Cantina"
      : null;

  const install = () => {
    setError(null);
    setProgress(null);
    updateInstall(setProgress).catch((e) => {
      setProgress(undefined);
      setError(String(e));
    });
  };

  return (
    <Toast
      role="status"
      warn={error !== null}
      actions={
        installing ? null : (
          <>
            <Button tier="quiet" onClick={onDismiss}>
              Later
            </Button>
            <Button
              tier="primary"
              disabled={blocked !== null}
              title={blocked ?? "Download, install and restart into the new version"}
              onClick={install}
            >
              Install and restart
            </Button>
          </>
        )
      }
    >
      {error
        ? `The update did not install: ${error}`
        : installing
          ? progress === null
            ? `Downloading Cantina ${update.version}…`
            : `Downloading Cantina ${update.version}… ${Math.round(progress * 100)}%`
          : `Cantina ${update.version} is out.${update.notes ? ` ${update.notes}` : ""}`}
      {blocked && !installing && !error ? (
        <span className="block text-ink-dim text-label">{blocked}.</span>
      ) : null}
    </Toast>
  );
}
