import { type EngineStatus, routeLabel } from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";

type Props = {
  status: EngineStatus | null;
  /** True while a start or stop is in flight. */
  busy: boolean;
  onToggle: () => void;
};

/**
 * The engine key in the toolbar under the title: its power LED shows the
 * audio stream running, and a press starts or stops it. The route and
 * rates are its tooltip; Settings spells them out.
 */
export function EngineControl({ status, busy, onToggle }: Props) {
  const running = status?.running ?? false;
  return (
    <Button
      aria-pressed={running}
      disabled={status === null || busy}
      onClick={onToggle}
      title={status ? routeLabel(status) : undefined}
      variant={running ? "transport" : "panel"}
      size="md"
    >
      <Led on={running} label="Engine" />
      {busy ? "…" : "Engine"}
    </Button>
  );
}
