import { type ReactNode, useEffect, useId, useRef } from "react";
import { Button } from "./Button";

type Props = {
  title: string;
  onClose: () => void;
  /** A wider panel, for a list with details beside each entry. */
  wide?: boolean;
  children: ReactNode;
};

/**
 * A panel that opens over the window for settings and pickers. It is a
 * modal dialog: focus moves in on open, Escape and the scrim close it,
 * and focus returns to the opener when it goes away.
 */
export function Sheet({ title, onClose, wide = false, children }: Props) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    const first = panel.current?.querySelector<HTMLElement>(
      "button, [href], input, select, [tabindex]:not([tabindex='-1'])",
    );
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      opener?.focus();
    };
  }, [onClose]);

  return (
    <div className="absolute inset-0 z-10 flex items-start justify-center bg-scrim p-6 pt-12">
      <button
        type="button"
        aria-label="Close"
        tabIndex={-1}
        onClick={onClose}
        className="absolute inset-0 cursor-default border-0 bg-transparent"
      />
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={`surface-panel relative w-full overflow-hidden ${wide ? "max-w-2xl" : "max-w-md"}`}
      >
        <header className="surface-titlebar flex min-h-[30px] items-center gap-2 py-1 pr-1 pl-3">
          <h2 id={titleId} className="title-text m-0 flex-1 font-normal text-[1.05rem]">
            {title}
          </h2>
          <Button onClick={onClose} aria-label={`Close ${title}`} className="px-2">
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
            </svg>
          </Button>
        </header>
        <div className="grid gap-3 bg-sheet-bg p-4 text-sheet-ink">{children}</div>
      </div>
    </div>
  );
}
