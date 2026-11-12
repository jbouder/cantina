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
 * The audio key, first in the transport: its light shows the audio
 * stream running, its caption says Audio on or Audio off, and a press
 * turns it on or off. The route and rates are its tooltip; Settings
 * spells them out.
 */
export function EngineControl({ status, busy, onToggle }: Props) {
  const running = status?.running ?? false;
  return (
    <Button
      aria-label="Audio"
      aria-pressed={running}
      disabled={status === null || busy}
      onClick={onToggle}
      title={
        status
          ? `Audio ${running ? "on" : "off"}: ${routeLabel(status)}. Click to turn it ${running ? "off" : "on"}`
          : "Audio"
      }
      className="px-2"
    >
      <Led on={running} label="Audio on" />
      <span className="@4xl:inline hidden">{busy ? "…" : running ? "Audio on" : "Audio off"}</span>
    </Button>
  );
}
