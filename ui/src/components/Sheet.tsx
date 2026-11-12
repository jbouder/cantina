import { type ReactNode, useEffect, useId, useRef } from "react";
import { Button } from "./Button";

type Props = {
  title: string;
  onClose: () => void;
  /** A wider panel, for a list with details beside each entry. */
  wide?: boolean;
  /**
   * Opens down the right side with nothing dimmed, so what is under it
   * to the left stays in view: the chain a rig is about to change.
   */
  side?: boolean;
  /**
   * The keys along the bottom, right-aligned: the sheet's one primary
   * button last, anything else before it. Left out, the footer is a
   * single Done.
   */
  footer?: ReactNode;
  children: ReactNode;
};

/**
 * A panel that opens over the window for settings and pickers. It is a
 * modal dialog: focus moves in on open, Escape and the scrim close it,
 * and focus returns to the opener when it goes away. It is centered and
 * never taller than the window: the body scrolls between a fixed header
 * and a fixed footer, which holds the sheet's one primary button.
 */
export function Sheet({ title, onClose, wide = false, side = false, footer, children }: Props) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);

  // Focus moves in once, on open: a new `onClose` must not pull it back
  // to the first control, nor hand it to the opener.
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    const first = panel.current?.querySelector<HTMLElement>(
      "button, [href], input, select, [tabindex]:not([tabindex='-1'])",
    );
    first?.focus();
    return () => opener?.focus();
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className={`absolute inset-0 z-10 flex ${side ? "items-stretch justify-end p-2" : "items-center justify-center bg-scrim p-6"}`}
    >
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
        className={`surface-panel relative flex max-h-full w-full flex-col overflow-hidden ${wide ? "max-w-2xl" : "max-w-md"} ${side ? "h-full" : ""}`}
      >
        <header className="surface-titlebar flex min-h-[30px] items-center gap-2 py-1 pr-1 pl-3">
          <h2 id={titleId} className="title-text m-0 flex-1 font-normal text-readout">
            {title}
          </h2>
          <Button
            tier="quiet"
            square
            onClick={onClose}
            aria-label={`Close ${title}`}
            className="text-title-text"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
            </svg>
          </Button>
        </header>
        <div
          className={`scroll-area grid min-h-0 content-start gap-3 bg-sheet-bg p-4 text-sheet-ink ${side ? "flex-1" : ""}`}
        >
          {children}
        </div>
        <footer className="flex shrink-0 flex-wrap items-center justify-end gap-2 border-frame-lo border-t bg-sheet-bg px-4 py-3 text-sheet-ink">
          {footer ?? (
            <Button tier="primary" onClick={onClose}>
              Done
            </Button>
          )}
        </footer>
      </div>
    </div>
  );
}
