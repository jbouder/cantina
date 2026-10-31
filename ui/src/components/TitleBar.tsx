import type { ReactNode } from "react";

type Props = {
  title: string;
  /** Controls at the right end, such as the settings button. */
  children?: ReactNode;
};

/** The strip across the top of a window: grips, the title and its buttons. */
export function TitleBar({ title, children }: Props) {
  return (
    <header className="surface-titlebar flex min-h-[30px] shrink-0 items-center gap-2 py-1 pr-1 pl-2">
      <span aria-hidden="true" className="titlebar-grip" />
      <h1 className="title-text m-0 whitespace-nowrap font-normal text-[1.05rem]">{title}</h1>
      <span aria-hidden="true" className="titlebar-grip" />
      {children}
    </header>
  );
}
