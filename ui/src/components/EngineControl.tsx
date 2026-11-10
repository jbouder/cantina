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
      aria-label="Engine"
      aria-pressed={running}
      disabled={status === null || busy}
      onClick={onToggle}
      title={status ? `Engine: ${routeLabel(status)}` : "Engine"}
      className="px-2"
    >
      <Led on={running} label="Engine" />
      <span className="@4xl:inline hidden">{busy ? "…" : "Engine"}</span>
    </Button>
  );
}
