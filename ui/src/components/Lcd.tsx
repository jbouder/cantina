import type { HTMLAttributes } from "react";

type Props = HTMLAttributes<HTMLElement> & {
  /** Tone for an error readout; the skin picks the color. */
  error?: boolean;
};

/** A display box in the skin's LCD material. Text inside uses the LCD face. */
export function Lcd({ className = "", error = false, children, ...rest }: Props) {
  return (
    <output
      className={`surface-lcd block px-2 py-1 font-lcd text-lg leading-tight ${error ? "text-lcd-error" : ""} ${className}`}
      {...rest}
    >
      <span className="lcd-text">{children}</span>
    </output>
  );
}
