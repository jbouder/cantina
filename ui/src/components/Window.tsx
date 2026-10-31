import type { ReactNode } from "react";
import { TitleBar } from "./TitleBar";

type Props = {
  title: string;
  /** Title bar controls, at the right end. */
  controls?: ReactNode;
  children: ReactNode;
};

/** The skinned frame that fills the shell window. */
export function Window({ title, controls, children }: Props) {
  return (
    <div className="relative flex h-full flex-col overflow-hidden p-2">
      <div className="surface-window flex min-h-0 flex-1 flex-col overflow-hidden">
        <TitleBar title={title}>{controls}</TitleBar>
        <div className="relative flex min-h-0 flex-1 flex-col">{children}</div>
      </div>
    </div>
  );
}
