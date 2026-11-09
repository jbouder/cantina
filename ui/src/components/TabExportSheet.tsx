import { useState } from "react";
import {
  type ClipInfo,
  chooseTabExportPath,
  clipTabExport,
  type Tab,
  type TabExportFormat,
  tabPdfWrite,
} from "../lib/engine";
import { tabPdf } from "../lib/tabPdf";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Sheet } from "./Sheet";

type Props = {
  clip: ClipInfo;
  tab: Tab;
  onClose: () => void;
};

/** The formats, in the order they are offered, and what each is for. */
const FORMATS: { format: TabExportFormat; label: string; detail: string }[] = [
  {
    format: "musicxml",
    label: "MusicXML",
    detail:
      "For notation apps such as MuseScore, Guitar Pro and TuxGuitar: strings, frets and most techniques.",
  },
  {
    format: "ascii",
    label: "ASCII tab",
    detail: "Plain text that pastes cleanly into a forum post or a message.",
  },
  {
    format: "midi",
    label: "MIDI",
    detail: "The notes for any sequencer, a channel per string, bends as pitch bend.",
  },
  {
    format: "pdf",
    label: "PDF",
    detail: "For printing: the page view, four bars a row.",
  },
];

/**
 * Exports a clip's tab: pick a format, then where to save it. The PDF is
 * drawn here from the page layout; the rest are written by the shell.
 */
export function TabExportSheet({ clip, tab, onClose }: Props) {
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const name = tab.name || clip.name;

  const save = async (format: TabExportFormat) => {
    setError(null);
    setSaved(null);
    const path = await chooseTabExportPath(name, format);
    if (!path) return;
    setBusy(true);
    try {
      const written =
        format === "pdf"
          ? await tabPdfWrite(path, tabPdf(tab, name))
          : await clipTabExport(clip.id, format, path);
      setSaved(written);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet title={`Export ${name}`} onClose={onClose}>
      <ul className="m-0 grid list-none gap-2 p-0">
        {FORMATS.map((f) => (
          <li key={f.format} className="grid grid-cols-[8rem_1fr] items-center gap-3">
            <Button disabled={busy} onClick={() => save(f.format)}>
              {f.label}…
            </Button>
            <span className="text-sm">{f.detail}</span>
          </li>
        ))}
      </ul>
      <p role="status" className="m-0 min-h-5 text-sm">
        {saved ? `Saved ${saved.split("/").pop()}` : ""}
      </p>
      {error ? (
        <Lcd role="alert" error className="text-base">
          {error}
        </Lcd>
      ) : null}
      <div className="flex justify-end">
        <Button onClick={onClose}>Done</Button>
      </div>
    </Sheet>
  );
}
