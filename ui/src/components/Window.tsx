import type { ReactNode } from "react";
import { TitleBar } from "./TitleBar";

type Props = {
  title: string;
  /** Toolbar controls at the left end: the project cluster. */
  start?: ReactNode;
  /** The in and out meters, in the middle of the toolbar. */
  meters?: ReactNode;
  /** Toolbar controls at the right end: the engine cluster and settings. */
  end?: ReactNode;
  /** One line of status under the body: what the transport is waiting for. */
  status?: ReactNode;
  children: ReactNode;
};

/**
 * The skinned frame that fills the shell window: the title bar, one
 * key-high toolbar (the project at the left, the meters in the middle,
 * the engine at the right) that never wraps, the body, and a status line
 * along the bottom. The toolbar is a container, so its children can drop
 * what is optional when the window is narrow.
 */
export function Window({ title, start, meters, end, status, children }: Props) {
  return (
    <div className="window-glow relative flex h-full flex-col overflow-hidden p-2">
      <div className="surface-window flex min-h-0 flex-1 flex-col overflow-hidden">
        <TitleBar title={title} />
        {start || end ? (
          <div className="@container shrink-0 px-window pt-3">
            <div className="flex items-center gap-3">
              {start ? <div className="flex min-w-0 items-center gap-1">{start}</div> : null}
              <div className="flex min-w-28 flex-1 items-center justify-center gap-3">{meters}</div>
              {end ? <div className="flex shrink-0 items-center gap-1">{end}</div> : null}
            </div>
          </div>
        ) : null}
        <div className="relative flex min-h-0 flex-1 flex-col">{children}</div>
        <div
          role="status"
          className="min-h-6 shrink-0 truncate px-window pb-1 text-ink-dim text-label leading-none"
        >
          {status}
        </div>
      </div>
    </div>
  );
}
