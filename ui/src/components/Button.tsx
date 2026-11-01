import type { ButtonHTMLAttributes } from "react";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** `transport` is a silver key; `panel` is a smaller key in the frame's material. */
  variant?: "transport" | "panel";
  /** `lg` is a 44 px key for the transport; `md` is the 32 px desktop key. Transport keys default to `lg`. */
  size?: "md" | "lg";
};

/** A skinned push button: 32 px on the desktop, 44 px for the transport keys. */
export function Button({
  variant = "panel",
  size,
  className = "",
  type = "button",
  ...rest
}: Props) {
  const surface = variant === "transport" ? "surface-button" : "surface-pbutton";
  const large = (size ?? (variant === "transport" ? "lg" : "md")) === "lg";
  return (
    <button
      type={type}
      className={`${surface} inline-flex items-center justify-center gap-1.5 whitespace-nowrap border-0 font-bold text-[11px] uppercase leading-none tracking-[0.06em] ${large ? "min-h-11 min-w-11 px-3" : "min-h-8 min-w-8 px-2.5"} ${className}`}
      {...rest}
    />
  );
}
