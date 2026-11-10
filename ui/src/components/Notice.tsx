import { Toast } from "./Toast";

type Props = {
  message: string;
  /** An error is announced at once; a notice waits its turn. */
  error?: boolean;
  onDismiss: () => void;
};

/**
 * Something the player should know about: what the engine did on its own
 * (a device went away, a plugin is missing), a warning, or an error.
 */
export function Notice({ message, error = false, onDismiss }: Props) {
  return (
    <Toast role={error ? "alert" : "status"} warn onDismiss={onDismiss}>
      {message}
    </Toast>
  );
}
