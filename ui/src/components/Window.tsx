import type { ReactNode } from "react";
import { TitleBar } from "./TitleBar";

type Props = {
  title: string;
  /** Toolbar controls at the left end: the project cluster. */
  start?: ReactNode;
  /** Toolbar controls at the right end: the engine cluster and settings. */
  end?: ReactNode;
  /** A full-width line under the toolbar: the in and out meters. */
  meters?: ReactNode;
  children: ReactNode;
};

/**
 * The skinned frame that fills the shell window: the title bar, one
 * key-high toolbar with the project at the left and the engine at the
 * right, the meter line across the full width, then the body.
 */
export function Window({ title, start, end, meters, children }: Props) {
  return (
    <div className="window-glow relative flex h-full flex-col overflow-hidden p-2">
      <div className="surface-window flex min-h-0 flex-1 flex-col overflow-hidden">
        <TitleBar title={title} />
        {start || end ? (
          <div className="flex shrink-0 items-center gap-1 px-window pt-window">
            {start ? <div className="flex min-w-0 shrink items-center gap-1">{start}</div> : null}
            {end ? <div className="ml-auto flex shrink-0 items-center gap-1">{end}</div> : null}
          </div>
        ) : null}
        {meters ? (
          <div className="grid shrink-0 grid-cols-2 gap-3 px-window pt-2">{meters}</div>
        ) : null}
        <div className="relative flex min-h-0 flex-1 flex-col">{children}</div>
      </div>
    </div>
  );
}
