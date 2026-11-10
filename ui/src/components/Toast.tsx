import type { AriaRole, ReactNode } from "react";
import { Button } from "./Button";

type StackProps = {
  children: ReactNode;
};

/**
 * Where notices, prompts and errors show: stacked in the bottom right
 * corner of the body, over the work and out of the layout, so one
 * arriving never moves the arrangement or the editor. Newest at the
 * bottom; the stack scrolls when it is taller than the body.
 */
export function Toasts({ children }: StackProps) {
  return (
    <div className="pointer-events-none absolute inset-y-2 right-window z-40 flex w-96 max-w-[calc(100%-2rem)] flex-col justify-end">
      <div className="pointer-events-auto flex max-h-full flex-col gap-2 overflow-y-auto p-0.5">
        {children}
      </div>
    </div>
  );
}

type Props = {
  /**
   * How assistive tech hears it: `status` for something to know, `alert`
   * for an error, `alertdialog` for a question that needs an answer.
   */
  role: Extract<AriaRole, "status" | "alert" | "alertdialog">;
  /** An error or a warning: marked with the record light's color. */
  warn?: boolean;
  /** The keys under the message, for one that asks something. */
  actions?: ReactNode;
  /** For a message with nothing to answer: a close key beside it. */
  onDismiss?: () => void;
  children: ReactNode;
};

/** One message in the stack, with its own keys. */
export function Toast({ role, warn = false, actions, onDismiss, children }: Props) {
  return (
    <div role={role} className="surface-panel grid shrink-0 gap-2 p-2 text-body">
      <div className="flex items-start gap-2 pl-1">
        <span
          aria-hidden="true"
          className={`mt-2 size-2 shrink-0 rounded-full ${warn ? "bg-led-rec" : "bg-led-on"}`}
        />
        <p className="m-0 min-w-0 flex-1 select-text break-words py-1">{children}</p>
        {onDismiss ? (
          <Button tier="quiet" square aria-label="Dismiss" title="Dismiss" onClick={onDismiss}>
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3 fill-current">
              <path d="M3 1.6 8 6.6l5-5L14.4 3l-5 5 5 5-1.4 1.4-5-5-5 5L1.6 13l5-5-5-5z" />
            </svg>
          </Button>
        ) : null}
      </div>
      {actions ? <div className="flex flex-wrap justify-end gap-1">{actions}</div> : null}
    </div>
  );
}
