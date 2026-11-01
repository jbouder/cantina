import type { ReactNode } from "react";
import { TitleBar } from "./TitleBar";

type Props = {
  title: string;
  /** Title bar controls at the left end. */
  start?: ReactNode;
  /** Title bar controls at the right end. */
  end?: ReactNode;
  children: ReactNode;
};

/** The skinned frame that fills the shell window. */
export function Window({ title, start, end, children }: Props) {
  return (
    <div className="window-glow relative flex h-full flex-col overflow-hidden p-2">
      <div className="surface-window flex min-h-0 flex-1 flex-col overflow-hidden">
        <TitleBar title={title} start={start} end={end} />
        <div className="relative flex min-h-0 flex-1 flex-col">{children}</div>
      </div>
    </div>
  );
}
