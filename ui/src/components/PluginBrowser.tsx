import { useCallback, useEffect, useState } from "react";
import {
  chainAdd,
  type EngineStatus,
  type PluginEntry,
  type PluginFormat,
  type PluginList,
  type PluginsStatus,
  pluginList,
  pluginRescan,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";

type Props = {
  /** The track a chosen plugin is added to. */
  track: number;
  trackName: string;
  plugins: PluginsStatus;
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
};

const FORMAT_NAMES: Record<PluginFormat, string> = { au: "Audio Units", clap: "CLAP" };

/** Entries grouped by format, then vendor, each group sorted by name. */
function group(entries: PluginEntry[]) {
  const formats = new Map<PluginFormat, Map<string, PluginEntry[]>>();
  for (const entry of entries) {
    const vendors = formats.get(entry.format) ?? new Map<string, PluginEntry[]>();
    formats.set(entry.format, vendors);
    const vendor = entry.vendor || "Unknown vendor";
    vendors.set(vendor, [...(vendors.get(vendor) ?? []), entry]);
  }
  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  return (["au", "clap"] as const)
    .filter((f) => formats.has(f))
    .map((format) => ({
      format,
      vendors: [...(formats.get(format) ?? new Map<string, PluginEntry[]>())]
        .sort(([a], [b]) => a.localeCompare(b, undefined, { sensitivity: "base" }))
        .map(([vendor, list]) => ({ vendor, plugins: [...list].sort(byName) })),
    }));
}

function ago(seconds: number) {
  const minutes = Math.round((Date.now() / 1000 - seconds) / 60);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.round(hours / 24)} days ago`;
}

/**
 * The installed plugins the scanner found, grouped by format and vendor,
 * each with a button that adds it to the chosen track's chain. Rescan
 * looks at new and changed bundles; a full rescan looks at everything.
 */
export function PluginBrowser({ track, trackName, plugins, onClose, onStatus }: Props) {
  const [list, setList] = useState<PluginList | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    pluginList().then(setList, (e) => setError(String(e)));
  }, []);

  // Fetch on open and again each time a scan finishes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: scannedAt is the trigger
  useEffect(refresh, [refresh, plugins.scannedAt]);

  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => setError(String(e)));

  const groups = list ? group(list.plugins) : [];
  const summary = plugins.scanning
    ? "Scanning…"
    : list === null
      ? "..."
      : list.scannedAt === 0
        ? "Not scanned yet"
        : `${list.plugins.length} plugin${list.plugins.length === 1 ? "" : "s"} · scanned ${ago(list.scannedAt)}`;

  return (
    <Sheet title="Plugins" onClose={onClose}>
      <div className="flex flex-wrap items-center gap-2">
        <Lcd aria-live="polite" className="min-w-0 flex-1 text-base">
          {summary}
        </Lcd>
        <Button disabled={plugins.scanning} onClick={() => run(pluginRescan(false))}>
          Rescan
        </Button>
        <Button
          disabled={plugins.scanning}
          onClick={() => run(pluginRescan(true))}
          title="Examine every plugin again, not just new and changed ones"
        >
          Full rescan
        </Button>
      </div>
      {error ? (
        <Lcd role="alert" error className="text-base">
          {error}
        </Lcd>
      ) : null}
      <p className="m-0 text-ink-dim text-xs">
        Add puts a plugin at the end of {trackName}'s chain. Plugins are scanned in a separate
        process, so one that crashes on load is listed under failed instead of taking Cantina down.
      </p>
      <div className="surface-lcd max-h-80 overflow-y-auto p-1">
        {groups.length === 0 ? (
          <p className="m-0 px-2 py-1 font-lcd text-lg">
            {plugins.scanning ? "Looking for plugins…" : "No plugins found"}
          </p>
        ) : (
          groups.map(({ format, vendors }) => (
            <section key={format} aria-labelledby={`plugins-${format}`} className="mb-1">
              <h3
                id={`plugins-${format}`}
                className="m-0 px-2 pt-1 font-bold text-[11px] uppercase tracking-[0.1em]"
              >
                {FORMAT_NAMES[format]}
              </h3>
              {vendors.map(({ vendor, plugins: entries }) => (
                <ul
                  key={vendor}
                  aria-label={`${vendor} ${FORMAT_NAMES[format]}`}
                  className="m-0 list-none p-0"
                >
                  <li className="px-2 pt-1 font-lcd text-base text-lcd-dim">{vendor}</li>
                  {entries.map((p) => (
                    <li key={p.spec} className="flex items-center gap-2 py-0.5 pr-1 pl-4">
                      <span className="lcd-text min-w-0 flex-1 truncate font-lcd text-lg">
                        {p.name}
                      </span>
                      <span className="lcd-text shrink-0 font-lcd text-base text-lcd-dim">
                        {p.inputs}→{p.outputs}
                        {p.hasEditor ? " · editor" : ""}
                        {p.version ? ` · v${p.version}` : ""}
                      </span>
                      <Button
                        aria-label={`Add ${p.name} to ${trackName}`}
                        onClick={() => run(chainAdd(track, p.spec))}
                        className="min-h-9 px-2"
                      >
                        Add
                      </Button>
                    </li>
                  ))}
                </ul>
              ))}
            </section>
          ))
        )}
      </div>
      {list && list.failed.length > 0 ? (
        <section aria-labelledby="plugins-failed" className="grid gap-1">
          <h3 id="plugins-failed" className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]">
            Failed to load
          </h3>
          <ul className="surface-lcd m-0 max-h-32 list-none overflow-y-auto p-1">
            {list.failed.map((f) => (
              <li key={`${f.source}:${f.name}`} className="px-2 py-0.5 font-lcd text-base">
                <span className="lcd-text text-lcd-error">{f.name}</span>
                <span className="lcd-text text-lcd-dim"> · {f.why}</span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </Sheet>
  );
}
