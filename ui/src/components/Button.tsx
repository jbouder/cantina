import type { ComponentProps } from "react";

/**
 * How much a button asks for attention. A view has at most one `primary`:
 * the action it is there for. `standard` is every other action. `quiet`
 * is for low-stakes and repeated ones (Rename, Rescan, a row's Remove),
 * which show no key until hovered.
 */
export type ButtonTier = "primary" | "standard" | "quiet";

type Props = ComponentProps<"button"> & {
  tier?: ButtonTier;
  /** For a key that holds only a glyph: 32 px wide as well as high. */
  square?: boolean;
  /**
   * For a key whose caption drops out in a narrow strip: less padding
   * until the strip is wide, so the glyph alone makes a 32 px key.
   */
  tight?: boolean;
};

const SURFACE: Record<ButtonTier, string> = {
  primary: "button-primary",
  standard: "button-standard",
  quiet: "button-quiet",
};

/**
 * A skinned push button, 32 px high everywhere, labelled in sentence
 * case. Its states come from ARIA, not from classes: `aria-pressed` for a
 * toggle that is on, `aria-selected`, `aria-checked` or `aria-current`
 * for the chosen one of several, and `disabled`.
 */
export function Button({
  tier = "standard",
  square = false,
  tight = false,
  className = "",
  type = "button",
  ...rest
}: Props) {
  return (
    <button
      type={type}
      className={`${SURFACE[tier]} inline-flex min-h-8 min-w-8 shrink-0 items-center justify-center gap-2 whitespace-nowrap border-0 ${square ? "w-8 px-0" : tight ? "@5xl:px-3 px-2" : "px-3"} font-bold text-label leading-none ${className}`}
      {...rest}
    />
  );
}
