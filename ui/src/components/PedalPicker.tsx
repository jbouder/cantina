import { useCallback, useEffect, useId, useState } from "react";
import {
  chainAdd,
  chooseImportPaths,
  type EngineStatus,
  type PedalKindInfo,
  type PluginEntry,
  type PluginFormat,
  type PluginList,
  type PluginsStatus,
  pedalImport,
  pluginList,
  pluginRescan,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";

type Props = {
  /** The track a chosen pedal is added to. */
  track: number;
  trackName: string;
  /** The stock and library pedals. */
  kinds: PedalKindInfo[];
  plugins: PluginsStatus;
  onClose: () => void;
  /** Starts a new design at the end of the chain and opens the designer. */
  onDesign: () => void;
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

/** Case-insensitive containment, for the filter. */
function matches(filter: string, ...texts: string[]) {
  const f = filter.trim().toLowerCase();
  return f === "" || texts.some((t) => t.toLowerCase().includes(f));
}

const HEADING = "m-0 px-2 pt-1 font-bold text-[11px] uppercase tracking-[0.1em]";
const ROW =
  "flex min-h-9 w-full items-center gap-2 border-0 bg-transparent px-2 text-left text-lcd-fg hover:bg-select focus-visible:bg-select";
/** Stock and library names are short, so they go two to a row. */
const KIND_LIST = "m-0 grid list-none grid-cols-2 gap-x-2 p-0";

/**
 * One picker for everything a chain can take: the stock pedals, the
 * ones in your library, the installed plugins (scanned in a separate
 * process, with Rescan here), and at the bottom a new design or a
 * .pedal.json file. Choosing an entry adds it to the end of the track's
 * chain and closes the sheet. The field at the top filters all three
 * lists at once.
 */
export function PedalPicker({
  track,
  trackName,
  kinds,
  plugins,
  onClose,
  onDesign,
  onStatus,
}: Props) {
  const id = useId();
  const [filter, setFilter] = useState("");
  const [list, setList] = useState<PluginList | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    pluginList().then(setList, (e) => setError(String(e)));
  }, []);

  // Fetch on open and again each time a scan finishes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: scannedAt is the trigger
  useEffect(refresh, [refresh, plugins.scannedAt]);

  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => setError(String(e)));
  const add = (spec: string) =>
    chainAdd(track, spec).then(
      (s) => {
        onStatus(s);
        onClose();
      },
      (e) => setError(String(e)),
    );
  const importFiles = () => {
    chooseImportPaths().then(
      (paths) => {
        if (paths.length === 0) return;
        Promise.all(paths.map((path) => pedalImport(path, track))).then(
          (results) => {
            const last = results[results.length - 1];
            if (last) onStatus(last);
            onClose();
          },
          (e) => setError(String(e)),
        );
      },
      (e) => setError(String(e)),
    );
  };

  const stock = kinds.filter((k) => k.stock && matches(filter, k.name));
  const library = kinds.filter((k) => !k.stock && matches(filter, k.name));
  const found = list ? list.plugins.filter((p) => matches(filter, p.name, p.vendor)) : [];
  const groups = group(found);
  const summary = plugins.scanning
    ? "Scanning…"
    : list === null
      ? "..."
      : list.scannedAt === 0
        ? "Not scanned yet"
        : `${list.plugins.length} plugin${list.plugins.length === 1 ? "" : "s"} · scanned ${ago(list.scannedAt)}`;

  const kindRow = (k: PedalKindInfo) => (
    <li key={k.spec}>
      <button type="button" onClick={() => add(k.spec)} className={ROW}>
        <span className="lcd-text min-w-0 flex-1 truncate font-lcd text-lg">{k.name}</span>
        {k.designable ? (
          <span
            className="lcd-text shrink-0 font-lcd text-base text-lcd-dim"
            title="In the pedal format: the designer can open it"
          >
            json
          </span>
        ) : null}
      </button>
    </li>
  );

  return (
    <Sheet title={`Add a pedal to ${trackName}`} onClose={onClose} wide>
      <label htmlFor={`${id}-filter`} className="sr-only">
        Filter pedals and plugins
      </label>
      <input
        id={`${id}-filter`}
        type="search"
        placeholder="Type to filter"
        autoComplete="off"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        className="surface-lcd min-h-8 w-full px-3 font-lcd text-base placeholder:text-lcd-dim"
      />
      {error ? (
        <Lcd role="alert" error className="text-base">
          {error}
        </Lcd>
      ) : null}
      <div className="surface-lcd max-h-[50vh] overflow-y-auto p-1">
        {stock.length > 0 ? (
          <section aria-labelledby={`${id}-stock`} className="mb-1">
            <h3 id={`${id}-stock`} className={HEADING}>
              Stock
            </h3>
            <ul className={KIND_LIST}>{stock.map(kindRow)}</ul>
          </section>
        ) : null}
        {library.length > 0 ? (
          <section aria-labelledby={`${id}-library`} className="mb-1">
            <h3 id={`${id}-library`} className={HEADING}>
              Your pedals
            </h3>
            <ul className={KIND_LIST}>{library.map(kindRow)}</ul>
          </section>
        ) : null}
        <section aria-labelledby={`${id}-plugins`} className="mb-1">
          <div className="flex flex-wrap items-center gap-2 px-2 pt-1">
            <h3
              id={`${id}-plugins`}
              className="m-0 font-bold text-[11px] uppercase tracking-[0.1em]"
            >
              Plugins
            </h3>
            <span className="lcd-text min-w-0 flex-1 truncate font-lcd text-base text-lcd-dim">
              {summary}
            </span>
            <Button
              disabled={plugins.scanning}
              onClick={() => run(pluginRescan(false))}
              className="min-h-7 px-2"
            >
              Rescan
            </Button>
            <Button
              disabled={plugins.scanning}
              onClick={() => run(pluginRescan(true))}
              title="Examine every plugin again, not just new and changed ones"
              className="min-h-7 px-2"
            >
              Full rescan
            </Button>
          </div>
          {groups.length === 0 ? (
            <p className="m-0 px-2 py-1 font-lcd text-base text-lcd-dim">
              {plugins.scanning
                ? "Looking for plugins…"
                : filter
                  ? "No plugin matches"
                  : "No plugins found"}
            </p>
          ) : (
            groups.map(({ format, vendors }) => (
              <div key={format}>
                <h4 className="m-0 px-2 pt-1 font-lcd text-base text-lcd-dim">
                  {FORMAT_NAMES[format]}
                </h4>
                {vendors.map(({ vendor, plugins: entries }) => (
                  <ul
                    key={vendor}
                    aria-label={`${vendor} ${FORMAT_NAMES[format]}`}
                    className="m-0 list-none p-0"
                  >
                    {entries.map((p) => (
                      <li key={p.spec}>
                        <button
                          type="button"
                          aria-label={`Add ${p.name} by ${vendor}`}
                          onClick={() => add(p.spec)}
                          className={ROW}
                        >
                          <span className="lcd-text min-w-0 flex-1 truncate font-lcd text-lg">
                            {p.name}
                          </span>
                          <span className="lcd-text shrink-0 truncate font-lcd text-base text-lcd-dim">
                            {vendor} · {p.inputs}→{p.outputs}
                            {p.hasEditor ? " · editor" : ""}
                            {p.version ? ` · v${p.version}` : ""}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                ))}
              </div>
            ))
          )}
          {list && list.failed.length > 0 && !filter ? (
            <details className="px-2 py-1">
              <summary className="cursor-pointer font-lcd text-base text-lcd-error">
                {list.failed.length} failed to load
              </summary>
              <ul className="m-0 list-none p-0">
                {list.failed.map((f) => (
                  <li key={`${f.source}:${f.name}`} className="py-0.5 font-lcd text-base">
                    <span className="lcd-text text-lcd-error">{f.name}</span>
                    <span className="lcd-text text-lcd-dim"> · {f.why}</span>
                  </li>
                ))}
              </ul>
            </details>
          ) : null}
        </section>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button
          onClick={() => {
            onClose();
            onDesign();
          }}
          title="Start a new pedal at the end of the chain and open the designer on it"
        >
          New design…
        </Button>
        <Button
          onClick={importFiles}
          title="Copy a .pedal.json file into your library and add it to this chain; dropping one on the window does the same"
        >
          Import a file…
        </Button>
        <span className="text-ink-dim text-xs">
          Plugins are scanned in a separate process, so one that crashes on load is listed as failed
          instead of taking Cantina down.
        </span>
      </div>
    </Sheet>
  );
}
