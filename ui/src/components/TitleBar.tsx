type Props = {
  title: string;
};

/** The strip across the top of a window: grips either side of the title. */
export function TitleBar({ title }: Props) {
  return (
    <header className="surface-titlebar flex min-h-[30px] shrink-0 items-center gap-2 px-1 py-1">
      <span aria-hidden="true" className="titlebar-grip" />
      <h1 className="title-text m-0 whitespace-nowrap font-normal text-[1.05rem]">{title}</h1>
      <span aria-hidden="true" className="titlebar-grip" />
    </header>
  );
}
