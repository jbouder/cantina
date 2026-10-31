import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./engine";

/**
 * Sends uncaught errors and rejected promises to the shell, which prints
 * them on its stderr. The web view has no console anyone can see in a
 * built app, so this is how a broken UI tells the terminal.
 */
export function reportErrorsToShell() {
  if (!inTauri) return;
  const send = (level: string, message: string) => {
    invoke("ui_log", { level, message }).catch(() => {});
  };
  window.addEventListener("error", (e) =>
    send("error", `${e.message} (${e.filename}:${e.lineno})`),
  );
  window.addEventListener("unhandledrejection", (e) =>
    send("error", `unhandled: ${String(e.reason)}`),
  );
  document.addEventListener("visibilitychange", () =>
    send("info", `visibility ${document.visibilityState}`),
  );
  send("info", `ui loaded, visibility ${document.visibilityState}`);
}
