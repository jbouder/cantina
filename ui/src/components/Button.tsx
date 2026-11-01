import type { ButtonHTMLAttributes } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** `transport` is a silver key; `panel` is a key in the frame's material. */
  variant?: "transport" | "panel";
};

/** A skinned push button, 32 px high everywhere. */
export function Button({ variant = "panel", className = "", type = "button", ...rest }: Props) {
  const surface = variant === "transport" ? "surface-button" : "surface-pbutton";
  return (
    <button
      type={type}
      className={`${surface} inline-flex min-h-8 min-w-8 items-center justify-center gap-1.5 whitespace-nowrap border-0 px-2.5 font-bold text-[11px] uppercase leading-none tracking-[0.06em] ${className}`}
      {...rest}
    />
  );
}
