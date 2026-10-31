import type { ButtonHTMLAttributes } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** `transport` is a silver key; `panel` is a smaller key in the frame's material. */
  variant?: "transport" | "panel";
};

/** A skinned push button. Always at least 44 px tall so it is easy to hit. */
export function Button({ variant = "panel", className = "", type = "button", ...rest }: Props) {
  const surface = variant === "transport" ? "surface-button" : "surface-pbutton";
  return (
    <button
      type={type}
      className={`${surface} inline-flex min-h-11 min-w-11 items-center justify-center gap-1.5 whitespace-nowrap border-0 px-3 font-bold text-[11px] uppercase leading-none tracking-[0.06em] ${className}`}
      {...rest}
    />
  );
}
