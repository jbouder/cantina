import type { ReactNode } from "react";

type Props = {
  title: string;
  /** Controls at the left end: the project cluster. */
  start?: ReactNode;
  /** Controls at the right end: the engine cluster and settings. */
  end?: ReactNode;
};

/**
 * The strip across the top of a window: the project at the left end,
 * grips, the title, grips, and the engine and settings at the right end.
 */
export function TitleBar({ title, start, end }: Props) {
  return (
    <header className="surface-titlebar flex min-h-[30px] shrink-0 items-center gap-2 px-1 py-1">
      {start ? <div className="flex min-w-0 shrink items-center gap-1">{start}</div> : null}
      <span aria-hidden="true" className="titlebar-grip" />
      <h1 className="title-text m-0 whitespace-nowrap font-normal text-[1.05rem]">{title}</h1>
      <span aria-hidden="true" className="titlebar-grip" />
      {end ? <div className="flex shrink-0 items-center gap-1">{end}</div> : null}
    </header>
  );
}
