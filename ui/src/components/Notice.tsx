import { Button } from "./Button";

type Props = {
  message: string;
  onDismiss: () => void;
};

/** Something the engine did on its own that the player should know about. */
export function Notice({ message, onDismiss }: Props) {
  return (
    <div
      role="status"
      className="surface-lcd flex items-center gap-3 px-3 py-2 font-lcd text-lcd-error text-lg leading-tight"
    >
      <span className="flex-1">{message}</span>
      <Button onClick={onDismiss} className="shrink-0">
        OK
      </Button>
    </div>
  );
}
