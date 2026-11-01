import { type EngineStatus, type Level, routeLabel } from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";
import { Meter } from "./Meter";

type Props = {
  status: EngineStatus | null;
  /** True while a start or stop is in flight. */
  busy: boolean;
  input: Level;
  output: Level;
  onToggle: () => void;
};

/**
 * The engine cluster in the title bar: a key with the power LED that
 * starts and stops the audio stream, and the in and out meters. The
 * route and rates are its tooltip; Settings spells them out.
 */
export function EngineControl({ status, busy, input, output, onToggle }: Props) {
  const running = status?.running ?? false;
  return (
    <>
      <Button
        aria-pressed={running}
        disabled={status === null || busy}
        onClick={onToggle}
        title={status ? routeLabel(status) : undefined}
        variant={running ? "transport" : "panel"}
      >
        <Led on={running} label="Engine" />
        {busy ? "…" : "Engine"}
      </Button>
      <Meter label="In" level={input} mini />
      <Meter label="Out" level={output} mini />
    </>
  );
}
